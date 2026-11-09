//! Round closing, payout maths and payout execution.
//!
//! Closing is permissionless: the conditions (everyone paid, or the deadline
//! passed) are checked on-chain, so whoever calls it cannot change the result.

use near_sdk::json_types::{U64, U128};
use near_sdk::serde_json::json;
use near_sdk::{AccountId, Gas, NearToken, Promise, PromiseError, env, near, require};

use crate::auction::select_winner;
use crate::events;
use crate::model::*;
use crate::{Contract, ContractExt};

const GAS_FT_TRANSFER: Gas = Gas::from_tgas(10);
const GAS_PAYOUT_CALLBACK: Gas = Gas::from_tgas(5);
/// Gas kept aside for this call's own bookkeeping when sizing a payout batch.
const GAS_RESERVE: Gas = Gas::from_tgas(30);

#[near]
impl Contract {
    /// Close the current round once everyone paid or the deadline passed.
    pub fn close_round(&mut self, circle_id: String) {
        let now = env::block_timestamp();
        let mut circle = self.circle_or_panic(&circle_id).clone();
        require!(
            circle.status == CircleStatus::Active,
            "Circle is not active"
        );
        let key = circle.round_key(circle.current_round);
        let mut round = self.round_or_panic(&circle, circle.current_round).clone();
        require!(
            round.status == RoundStatus::Collecting,
            "Round already closed"
        );
        let all_paid = round.paid.len() == circle.member_count();
        require!(
            all_paid || now >= round.deadline.0,
            "Round is still collecting"
        );
        if all_paid {
            self.settle_round(&mut circle, &mut round, now);
        } else {
            self.void_round(&mut circle, &mut round, now);
        }
        self.rounds.insert(key, round);
        self.circles.insert(circle_id, circle);
    }

    /// Send up to `limit` pending payouts of a closed round. Returns how many
    /// payouts are still pending afterwards. Anyone may call it.
    pub fn execute_payouts(
        &mut self,
        circle_id: String,
        round_index: u32,
        limit: Option<u8>,
    ) -> u32 {
        let circle = self.circle_or_panic(&circle_id);
        let token = circle.token.clone();
        let key = circle.round_key(round_index);
        let round = self
            .rounds
            .get_mut(&key)
            .unwrap_or_else(|| env::panic_str("Round not found"));
        require!(
            round.status != RoundStatus::Collecting,
            "Round is not closed"
        );
        let max = batch_size(limit);
        let mut scheduled: u8 = 0;
        for (idx, payout) in round.payouts.iter_mut().enumerate() {
            if payout.status != PayoutStatus::Pending || scheduled >= max {
                continue;
            }
            payout.status = PayoutStatus::Sent;
            schedule_ft_transfer(&token, payout, &circle_id, round_index, idx as u32);
            scheduled += 1;
        }
        round.pending_payouts()
    }

    /// Retry a failed payout (e.g. after the recipient registered storage).
    pub fn claim_payout(&mut self, circle_id: String, round_index: u32, payout_index: u32) {
        let circle = self.circle_or_panic(&circle_id);
        let token = circle.token.clone();
        let agent = circle.agent_account.clone();
        let key = circle.round_key(round_index);
        let round = self
            .rounds
            .get_mut(&key)
            .unwrap_or_else(|| env::panic_str("Round not found"));
        let payout = round
            .payouts
            .get_mut(payout_index as usize)
            .unwrap_or_else(|| env::panic_str("Payout not found"));
        let caller = env::predecessor_account_id();
        require!(
            caller == payout.recipient || caller == agent,
            "Only the recipient or the agent"
        );
        require!(
            payout.status == PayoutStatus::Failed,
            "Payout is not in Failed state"
        );
        payout.status = PayoutStatus::Sent;
        schedule_ft_transfer(&token, payout, &circle_id, round_index, payout_index);
    }

    #[private]
    pub fn on_payout(
        &mut self,
        circle_id: String,
        round_index: u32,
        payout_index: u32,
        #[callback_result] result: Result<(), PromiseError>,
    ) {
        self.apply_payout_result(&circle_id, round_index, payout_index, result.is_ok());
    }
}

impl Contract {
    fn settle_round(&mut self, circle: &mut Circle, round: &mut Round, now: u64) {
        let n = circle.member_count() as u128;
        let contribution = circle.contribution.0;
        let pot = contribution * n;
        require!(round.pot.0 == pot, "Pot does not match contributions");
        let (winner, discount) = select_winner(circle, round);
        require!(discount <= pot / 2, "Discount exceeds half of the pot");
        round.payouts = build_payouts(&circle.members, &winner, pot, discount);
        round.winner = Some(winner.clone());
        round.discount = U128(discount);
        round.status = RoundStatus::Closed;
        round.closed_at = Some(U64(now));
        if let Some(m) = circle.member_mut(&winner) {
            m.collected = true;
        }
        for payer in round.paid.clone() {
            self.reputation_mut(&payer).apply_on_time(contribution, now);
            let score = self.reputation.get(&payer).map(|r| r.score).unwrap_or(0);
            events::emit(
                "reputation_updated",
                json!({ "account": payer, "score": score, "reason": "on_time" }),
            );
        }
        events::emit(
            "round_closed",
            json!({ "circle_id": circle.id, "round": round.index, "winner": winner,
                    "discount": U128(discount), "pot": U128(pot), "payouts": round.payouts.len() }),
        );
        if circle.all_collected() {
            self.complete_circle(circle, now);
        } else {
            self.open_round(circle, round.index + 1, now);
        }
    }

    fn void_round(&mut self, circle: &mut Circle, round: &mut Round, now: u64) {
        let contribution = circle.contribution;
        round.defaulters = circle
            .members
            .iter()
            .filter(|m| !round.paid.contains(&m.account))
            .map(|m| m.account.clone())
            .collect();
        for d in round.defaulters.clone() {
            self.reputation_mut(&d).apply_default(now);
            let score = self.reputation.get(&d).map(|r| r.score).unwrap_or(0);
            events::emit(
                "reputation_updated",
                json!({ "account": d, "score": score, "reason": "default" }),
            );
        }
        round.payouts = round
            .paid
            .iter()
            .map(|a| Payout {
                recipient: a.clone(),
                amount: contribution,
                kind: PayoutKind::Refund,
                status: PayoutStatus::Pending,
            })
            .collect();
        round.status = RoundStatus::Voided;
        round.closed_at = Some(U64(now));
        circle.status = CircleStatus::Frozen;
        circle.frozen_reason = Some("default".to_string());
        events::emit(
            "round_voided",
            json!({ "circle_id": circle.id, "round": round.index, "defaulters": round.defaulters,
                    "refunds": round.payouts.len() }),
        );
        events::emit(
            "circle_frozen",
            json!({ "circle_id": circle.id, "reason": "default" }),
        );
    }

    pub(crate) fn apply_payout_result(
        &mut self,
        circle_id: &str,
        round_index: u32,
        payout_index: u32,
        ok: bool,
    ) {
        let key = round_key(circle_id, round_index);
        let round = self
            .rounds
            .get_mut(&key)
            .unwrap_or_else(|| env::panic_str("Round not found"));
        let payout = round
            .payouts
            .get_mut(payout_index as usize)
            .unwrap_or_else(|| env::panic_str("Payout not found"));
        let (status, event) = if ok {
            (PayoutStatus::Confirmed, "payout_confirmed")
        } else {
            (PayoutStatus::Failed, "payout_failed")
        };
        payout.status = status;
        events::emit(
            event,
            json!({ "circle_id": circle_id, "round": round_index, "payout_index": payout_index,
                    "recipient": payout.recipient, "amount": payout.amount, "kind": payout.kind }),
        );
    }
}

/// Winner gets `pot - discount` plus any rounding dust; everyone else gets
/// `discount / (N - 1)`. The sum always equals the pot.
pub(crate) fn build_payouts(
    members: &[Member],
    winner: &AccountId,
    pot: u128,
    discount: u128,
) -> Vec<Payout> {
    let others: Vec<&Member> = members.iter().filter(|m| &m.account != winner).collect();
    let share = discount / others.len() as u128;
    let dust = discount - share * others.len() as u128;
    let mut payouts = vec![Payout {
        recipient: winner.clone(),
        amount: U128(pot - discount + dust),
        kind: PayoutKind::Pot,
        status: PayoutStatus::Pending,
    }];
    if share > 0 {
        payouts.extend(others.iter().map(|m| Payout {
            recipient: m.account.clone(),
            amount: U128(share),
            kind: PayoutKind::Dividend,
            status: PayoutStatus::Pending,
        }));
    }
    let total: u128 = payouts.iter().map(|p| p.amount.0).sum();
    require!(total == pot, "Payout invariant violated");
    payouts
}

/// Projection of the current round for UIs and the agent.
pub(crate) fn status_view(circle: &Circle, round: &Round, now: u64) -> RoundStatusView {
    let unpaid: Vec<AccountId> = circle
        .members
        .iter()
        .filter(|m| !round.paid.contains(&m.account))
        .map(|m| m.account.clone())
        .collect();
    let all_paid = unpaid.is_empty();
    let (winner, discount) = if circle.all_collected() {
        (None, 0)
    } else {
        let (w, d) = select_winner(circle, round);
        (Some(w), d)
    };
    let others = circle.member_count().saturating_sub(1).max(1) as u128;
    RoundStatusView {
        circle_id: circle.id.clone(),
        circle_status: circle.status.clone(),
        round_index: round.index,
        round_status: round.status.clone(),
        contribution: circle.contribution,
        pot: round.pot,
        paid: round.paid.clone(),
        unpaid,
        bids: round.bids.clone(),
        deadline: round.deadline,
        now: U64(now),
        can_close: circle.status == CircleStatus::Active
            && round.status == RoundStatus::Collecting
            && (all_paid || now >= round.deadline.0),
        projected_winner: winner,
        projected_discount: U128(discount),
        projected_dividend_each: U128(discount / others),
    }
}

fn batch_size(limit: Option<u8>) -> u8 {
    let requested = limit.unwrap_or(PAYOUT_BATCH_MAX).clamp(1, PAYOUT_BATCH_MAX);
    let per_payout = GAS_FT_TRANSFER.as_gas() + GAS_PAYOUT_CALLBACK.as_gas();
    let available = env::prepaid_gas()
        .as_gas()
        .saturating_sub(env::used_gas().as_gas());
    let by_gas = available.saturating_sub(GAS_RESERVE.as_gas()) / per_payout;
    requested.min(by_gas.clamp(1, u64::from(PAYOUT_BATCH_MAX)) as u8)
}

fn schedule_ft_transfer(
    token: &AccountId,
    payout: &Payout,
    circle_id: &str,
    round_index: u32,
    idx: u32,
) {
    let memo = format!("pasanaku:{circle_id}:{round_index}:{:?}", payout.kind).to_lowercase();
    let args = json!({ "receiver_id": payout.recipient, "amount": payout.amount, "memo": memo })
        .to_string()
        .into_bytes();
    Promise::new(token.clone())
        .function_call(
            "ft_transfer".to_string(),
            args,
            NearToken::from_yoctonear(1),
            GAS_FT_TRANSFER,
        )
        .then(
            Contract::ext(env::current_account_id())
                .with_static_gas(GAS_PAYOUT_CALLBACK)
                .on_payout(circle_id.to_string(), round_index, idx),
        )
        .detach();
    events::emit(
        "payout_sent",
        json!({ "circle_id": circle_id, "round": round_index, "payout_index": idx,
                "recipient": payout.recipient, "amount": payout.amount, "kind": payout.kind }),
    );
}

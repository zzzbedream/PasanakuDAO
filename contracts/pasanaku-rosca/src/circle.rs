//! Circle lifecycle: creation by a member, admission and governance by the DAO.

use near_sdk::json_types::{U64, U128};
use near_sdk::serde_json::json;
use near_sdk::{AccountId, NearToken, Promise, env, near, require};

use crate::events;
use crate::model::*;
use crate::reputation::tier_cap_for_score;
use crate::storage::storage_cost_for_members;
use crate::{Contract, ContractExt};

#[near]
impl Contract {
    /// Create a circle in `Pending` state. The caller must be one of the
    /// members and pays the storage for the whole circle up front.
    #[payable]
    #[allow(clippy::too_many_arguments)]
    pub fn create_circle(
        &mut self,
        circle_id: String,
        dao_account: AccountId,
        agent_account: AccountId,
        token: AccountId,
        contribution: U128,
        round_duration_ns: U64,
        min_reputation: u64,
        max_discount: U128,
        members: Vec<AccountId>,
        payout_mode: Option<PayoutMode>,
    ) {
        let creator = env::predecessor_account_id();
        validate_circle_id(&circle_id);
        require!(
            !self.circles.contains_key(&circle_id),
            "Circle id already exists"
        );
        validate_members(&members, &creator);
        require!(contribution.0 > 0, "Contribution must be positive");
        require!(
            round_duration_ns.0 >= MIN_ROUND_DURATION_NS,
            "Round duration too short"
        );
        let n = members.len() as u128;
        require!(
            max_discount.0 <= contribution.0 * n / 2,
            "max_discount must not exceed half of the pot"
        );
        self.assert_members_eligible(&members, min_reputation, contribution.0);
        let storage_paid = self.charge_storage(members.len() as u64, &creator);

        let circle = Circle {
            id: circle_id.clone(),
            creator: creator.clone(),
            dao_account,
            agent_account,
            token,
            contribution,
            round_duration_ns,
            min_reputation,
            max_discount,
            members: members
                .iter()
                .map(|a| Member {
                    account: a.clone(),
                    collected: false,
                    joined_round: 0,
                })
                .collect(),
            status: CircleStatus::Pending,
            current_round: 0,
            payout_mode: payout_mode.unwrap_or(PayoutMode::Direct),
            created_at: crate::now_u64(),
            frozen_reason: None,
            storage_paid: U128(storage_paid.as_yoctonear()),
        };
        self.circles.insert(circle_id.clone(), circle);
        events::emit(
            "circle_created",
            json!({ "circle_id": circle_id, "creator": creator, "members": members,
                    "contribution": contribution, "max_discount": max_discount }),
        );
    }

    /// DAO admits the circle and opens round 0.
    pub fn activate_circle(&mut self, circle_id: String) {
        let mut circle = self.circle_or_panic(&circle_id).clone();
        Self::assert_dao(&circle);
        require!(
            circle.status == CircleStatus::Pending,
            "Circle is not pending"
        );
        circle.status = CircleStatus::Active;
        let now = env::block_timestamp();
        self.open_round(&mut circle, 0, now);
        self.circles.insert(circle_id.clone(), circle);
        events::emit("circle_activated", json!({ "circle_id": circle_id }));
    }

    pub fn freeze_circle(&mut self, circle_id: String, reason: Option<String>) {
        let circle = self.circle_mut_or_panic(&circle_id);
        Self::assert_dao(circle);
        require!(
            circle.status == CircleStatus::Active,
            "Circle is not active"
        );
        circle.status = CircleStatus::Frozen;
        circle.frozen_reason = Some(reason.clone().unwrap_or_else(|| "dao".to_string()));
        events::emit(
            "circle_frozen",
            json!({ "circle_id": circle_id, "reason": reason }),
        );
    }

    /// Resume a frozen circle. If the current round was voided, a fresh round
    /// opens for the remaining members.
    pub fn unfreeze_circle(&mut self, circle_id: String) {
        let mut circle = self.circle_or_panic(&circle_id).clone();
        Self::assert_dao(&circle);
        require!(
            circle.status == CircleStatus::Frozen,
            "Circle is not frozen"
        );
        require!(
            !circle.all_collected(),
            "Nothing left to run; circle should complete"
        );
        circle.status = CircleStatus::Active;
        circle.frozen_reason = None;
        let current = self
            .round_or_panic(&circle, circle.current_round)
            .status
            .clone();
        if current != RoundStatus::Collecting {
            let now = env::block_timestamp();
            let next_index = circle.current_round + 1;
            self.open_round(&mut circle, next_index, now);
        }
        self.circles.insert(circle_id.clone(), circle);
        events::emit("circle_unfrozen", json!({ "circle_id": circle_id }));
    }

    /// Remove a member while the circle is frozen and no round is collecting.
    pub fn remove_member(&mut self, circle_id: String, account: AccountId) {
        let mut circle = self.circle_or_panic(&circle_id).clone();
        Self::assert_dao(&circle);
        require!(
            circle.status == CircleStatus::Frozen,
            "Circle must be frozen"
        );
        let round_status = self
            .round_or_panic(&circle, circle.current_round)
            .status
            .clone();
        require!(
            round_status != RoundStatus::Collecting,
            "Current round is still collecting"
        );
        require!(circle.is_member(&account), "Not a member");
        require!(
            circle.member_count() > MIN_MEMBERS,
            "Circle would fall below the minimum size"
        );
        circle.members.retain(|m| m.account != account);
        let now = env::block_timestamp();
        self.reputation_mut(&account).apply_removed(now);
        if circle.all_collected() {
            self.complete_circle(&mut circle, now);
        }
        self.circles.insert(circle_id.clone(), circle);
        events::emit(
            "member_removed",
            json!({ "circle_id": circle_id, "account": account }),
        );
    }

    /// Tune parameters; they apply from the next round on.
    pub fn set_params(
        &mut self,
        circle_id: String,
        round_duration_ns: Option<U64>,
        max_discount: Option<U128>,
        min_reputation: Option<u64>,
    ) {
        let circle = self.circle_mut_or_panic(&circle_id);
        Self::assert_dao(circle);
        if let Some(d) = round_duration_ns {
            require!(d.0 >= MIN_ROUND_DURATION_NS, "Round duration too short");
            circle.round_duration_ns = d;
        }
        if let Some(m) = max_discount {
            let n = circle.member_count() as u128;
            require!(
                m.0 <= circle.contribution.0 * n / 2,
                "max_discount too high"
            );
            circle.max_discount = m;
        }
        if let Some(r) = min_reputation {
            circle.min_reputation = r;
        }
        events::emit(
            "params_updated",
            json!({ "circle_id": circle_id, "round_duration_ns": round_duration_ns,
                    "max_discount": max_discount, "min_reputation": min_reputation }),
        );
    }
}

impl Contract {
    pub(crate) fn circle_mut_or_panic(&mut self, circle_id: &str) -> &mut Circle {
        self.circles
            .get_mut(circle_id)
            .unwrap_or_else(|| env::panic_str("Circle not found"))
    }

    fn assert_members_eligible(
        &self,
        members: &[AccountId],
        min_reputation: u64,
        contribution: u128,
    ) {
        for account in members {
            let score = self.reputation.get(account).map(|r| r.score).unwrap_or(0);
            require!(
                score >= min_reputation,
                "A member is below the required reputation"
            );
            require!(
                tier_cap_for_score(score) >= contribution,
                "Contribution exceeds a member's tier cap"
            );
        }
    }

    /// Take the storage deposit for `n` members and refund any excess.
    fn charge_storage(&self, n: u64, payer: &AccountId) -> NearToken {
        let cost = storage_cost_for_members(n);
        let attached = env::attached_deposit();
        require!(attached >= cost, "Insufficient storage deposit");
        let excess = attached.saturating_sub(cost);
        if excess > NearToken::from_yoctonear(0) {
            Promise::new(payer.clone()).transfer(excess).detach();
        }
        cost
    }

    pub(crate) fn complete_circle(&mut self, circle: &mut Circle, now: u64) {
        circle.status = CircleStatus::Completed;
        for member in circle.members.clone() {
            self.reputation_mut(&member.account).apply_completed(now);
        }
        events::emit("circle_completed", json!({ "circle_id": circle.id }));
    }
}

fn validate_circle_id(id: &str) {
    require!(
        (CIRCLE_ID_MIN_LEN..=CIRCLE_ID_MAX_LEN).contains(&id.len()),
        "circle_id must be 3..=32 characters"
    );
    require!(
        id.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
        "circle_id may only contain a-z, 0-9 and '-'"
    );
}

fn validate_members(members: &[AccountId], creator: &AccountId) {
    require!(
        (MIN_MEMBERS..=MAX_MEMBERS).contains(&members.len()),
        "Member count out of bounds"
    );
    require!(members.contains(creator), "Creator must be a member");
    for (i, m) in members.iter().enumerate() {
        require!(!members[..i].contains(m), "Duplicate member");
    }
}

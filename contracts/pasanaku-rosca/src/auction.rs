//! Urgency auction: a member who has not collected yet offers a discount on
//! the pot; the DAO approves bids; the highest approved discount wins.

use near_sdk::json_types::U128;
use near_sdk::serde_json::json;
use near_sdk::{AccountId, env, near, require};

use crate::events;
use crate::model::*;
use crate::{Contract, ContractExt};

#[near]
impl Contract {
    /// Offer `discount` (token units) to receive this round's pot early.
    /// A member may raise their own bid; it must beat every other bid.
    pub fn submit_bid(&mut self, circle_id: String, discount: U128) {
        let bidder = env::predecessor_account_id();
        let circle = self.circle_or_panic(&circle_id);
        require!(
            circle.status == CircleStatus::Active,
            "Circle is not active"
        );
        let member = circle
            .member(&bidder)
            .unwrap_or_else(|| env::panic_str("Only members can bid"));
        require!(!member.collected, "Member already collected a pot");
        require!(
            discount.0 > 0 && discount.0 <= circle.max_discount.0,
            "Discount out of bounds"
        );
        let key = circle.round_key(circle.current_round);
        let round_index = circle.current_round;

        let round = self
            .rounds
            .get_mut(&key)
            .unwrap_or_else(|| env::panic_str("Round not found"));
        require!(
            round.status == RoundStatus::Collecting,
            "Round is not collecting"
        );
        let highest = round.bids.iter().map(|b| b.discount.0).max().unwrap_or(0);
        require!(
            discount.0 > highest,
            "Bid must exceed the current highest bid"
        );
        round.bids.retain(|b| b.bidder != bidder);
        round.bids.push(Bid {
            bidder: bidder.clone(),
            discount,
            approved: false,
            submitted_at: crate::now_u64(),
        });
        events::emit(
            "bid_submitted",
            json!({ "circle_id": circle_id, "round": round_index, "bidder": bidder, "discount": discount }),
        );
    }

    /// DAO approval of an exact (bidder, discount) pair. Executed through a
    /// Sputnik `FunctionCall` proposal, so the predecessor is the DAO itself.
    pub fn approve_bid(&mut self, circle_id: String, bidder: AccountId, discount: U128) {
        let circle = self.circle_or_panic(&circle_id);
        Self::assert_dao(circle);
        let key = circle.round_key(circle.current_round);
        let round_index = circle.current_round;
        let round = self
            .rounds
            .get_mut(&key)
            .unwrap_or_else(|| env::panic_str("Round not found"));
        require!(
            round.status == RoundStatus::Collecting,
            "Round is not collecting"
        );
        let bid = round
            .bids
            .iter_mut()
            .find(|b| b.bidder == bidder && b.discount == discount)
            .unwrap_or_else(|| env::panic_str("No matching bid"));
        bid.approved = true;
        events::emit(
            "bid_approved",
            json!({ "circle_id": circle_id, "round": round_index, "bidder": bidder, "discount": discount }),
        );
    }
}

/// Highest approved bid from a member who has not collected; ties go to the
/// earliest bid. Without approved bids the rotation order decides (discount 0).
pub(crate) fn select_winner(circle: &Circle, round: &Round) -> (AccountId, u128) {
    let best = round
        .bids
        .iter()
        .filter(|b| b.approved && circle.member(&b.bidder).is_some_and(|m| !m.collected))
        .max_by(|a, b| {
            a.discount
                .0
                .cmp(&b.discount.0)
                .then(b.submitted_at.0.cmp(&a.submitted_at.0))
        });
    match best {
        Some(bid) => (bid.bidder.clone(), bid.discount.0),
        None => (
            circle
                .next_in_rotation()
                .unwrap_or_else(|| env::panic_str("No member left to pay")),
            0,
        ),
    }
}

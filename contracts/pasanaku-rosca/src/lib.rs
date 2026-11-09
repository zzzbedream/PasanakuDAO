//! PasanakuDAO: rotating savings & credit circles (ROSCA) on NEAR.
//!
//! One contract hosts many circles. Each circle is governed by its own
//! Sputnik DAO v2 account (admission, bid approval, freeze, parameters), holds
//! the current round's contributions in escrow, and pays the winner and the
//! dividends itself. Reputation is tracked per named account across circles.

pub mod auction;
pub mod circle;
pub mod contribution;
pub mod events;
pub mod model;
pub mod reputation;
pub mod round;
pub mod storage;

#[cfg(test)]
mod tests;

use near_sdk::json_types::{U64, U128};
use near_sdk::store::{IterableMap, LookupMap};
use near_sdk::{AccountId, PanicOnDefault, env, near, require};

use model::*;
use reputation::*;

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    /// Deployer. Has no power over funds; kept for future state migrations.
    pub owner_id: AccountId,
    pub circles: IterableMap<String, Circle>,
    /// Keyed by `"{circle_id}#{round_index}"`.
    pub rounds: LookupMap<String, Round>,
    pub reputation: IterableMap<AccountId, Reputation>,
}

#[near]
impl Contract {
    #[init]
    pub fn new(owner_id: AccountId) -> Self {
        Self {
            owner_id,
            circles: IterableMap::new(StorageKey::Circles),
            rounds: LookupMap::new(StorageKey::Rounds),
            reputation: IterableMap::new(StorageKey::Reputation),
        }
    }

    // ── Views ────────────────────────────────────────────────────────────

    pub fn get_circle(&self, circle_id: String) -> Option<Circle> {
        self.circles.get(&circle_id).cloned()
    }

    pub fn get_circles(&self, from_index: u64, limit: u64) -> Vec<Circle> {
        self.circles
            .iter()
            .skip(from_index as usize)
            .take(limit as usize)
            .map(|(_, c)| c.clone())
            .collect()
    }

    pub fn get_circle_count(&self) -> u32 {
        self.circles.len()
    }

    pub fn get_round(&self, circle_id: String, round_index: u32) -> Option<Round> {
        self.rounds
            .get(&round_key(&circle_id, round_index))
            .cloned()
    }

    /// Rounds `from_index..from_index+limit` of a circle (payout history).
    pub fn get_rounds(&self, circle_id: String, from_index: u32, limit: u32) -> Vec<Round> {
        let Some(circle) = self.circles.get(&circle_id) else {
            return Vec::new();
        };
        (from_index..=circle.current_round)
            .take(limit as usize)
            .filter_map(|i| self.rounds.get(&circle.round_key(i)).cloned())
            .collect()
    }

    /// Live snapshot of the current round with the projected winner and
    /// dividend. `None` when the circle does not exist or is still `Pending`
    /// (no round has been opened yet), so UIs never hit a panic.
    pub fn get_round_status(&self, circle_id: String) -> Option<RoundStatusView> {
        let circle = self.circles.get(&circle_id)?;
        let round = self.rounds.get(&circle.round_key(circle.current_round))?;
        Some(round::status_view(circle, round, env::block_timestamp()))
    }

    pub fn get_reputation(&self, account_id: AccountId) -> Reputation {
        self.reputation
            .get(&account_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_member_tier(&self, account_id: AccountId) -> TierView {
        TierView::for_score(self.get_reputation(account_id).score)
    }

    pub fn storage_cost_for_members(&self, members: u32) -> U128 {
        U128(storage::storage_cost_for_members(u64::from(members)).as_yoctonear())
    }

    pub fn get_config(&self) -> ConfigView {
        ConfigView {
            version: VERSION.to_string(),
            alpha: ALPHA,
            beta: BETA,
            gamma: GAMMA,
            tier_pyme_min_score: TIER_PYME_MIN_SCORE,
            tier_mayorista_min_score: TIER_MAYORISTA_MIN_SCORE,
            cap_semilla: U128(CAP_SEMILLA),
            cap_pyme: U128(CAP_PYME),
            cap_mayorista: U128(CAP_MAYORISTA),
            min_members: MIN_MEMBERS as u32,
            max_members: MAX_MEMBERS as u32,
            payout_batch_max: PAYOUT_BATCH_MAX,
            token_unit: U128(TOKEN_UNIT),
        }
    }
}

// ── Internal helpers shared by the modules ──────────────────────────────

impl Contract {
    pub(crate) fn circle_or_panic(&self, circle_id: &str) -> &Circle {
        self.circles
            .get(circle_id)
            .unwrap_or_else(|| env::panic_str("Circle not found"))
    }

    pub(crate) fn round_or_panic(&self, circle: &Circle, index: u32) -> &Round {
        self.rounds
            .get(&circle.round_key(index))
            .unwrap_or_else(|| env::panic_str("Round not found"))
    }

    pub(crate) fn assert_dao(circle: &Circle) {
        require!(
            env::predecessor_account_id() == circle.dao_account,
            "Only the governing DAO can call this method"
        );
    }

    pub(crate) fn reputation_mut(&mut self, account: &AccountId) -> &mut Reputation {
        if !self.reputation.contains_key(account) {
            self.reputation
                .insert(account.clone(), Reputation::default());
        }
        self.reputation
            .get_mut(account)
            .expect("reputation just inserted")
    }

    /// Open round `index` for `circle` and record it as the current round.
    pub(crate) fn open_round(&mut self, circle: &mut Circle, index: u32, now: u64) {
        let round = Round::open(index, now, circle.round_duration_ns.0);
        let deadline = round.deadline;
        self.rounds.insert(circle.round_key(index), round);
        circle.current_round = index;
        events::emit(
            "round_opened",
            near_sdk::serde_json::json!({
                "circle_id": circle.id, "round": index, "deadline": deadline,
                "members": circle.member_count(),
            }),
        );
    }
}

pub(crate) fn now_u64() -> U64 {
    U64(env::block_timestamp())
}

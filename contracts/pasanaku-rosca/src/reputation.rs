//! On-chain reputation: integer-only scoring and access tiers.
//!
//! `score(t+1) = score(t) + ALPHA*on_time + BETA*ln(1 + C) - GAMMA*default`
//! where `C` is the contribution in whole tokens. `ln` is approximated in
//! milli-nats with a 17-entry table (error below 1.5 %), so the contract never
//! touches floating point.

use near_sdk::json_types::U128;
use near_sdk::near;

use crate::model::{Reputation, TOKEN_UNIT};

pub const ALPHA: u64 = 10;
pub const BETA: u64 = 5;
pub const GAMMA: u64 = 25;

pub const TIER_PYME_MIN_SCORE: u64 = 100;
pub const TIER_MAYORISTA_MIN_SCORE: u64 = 300;
pub const CAP_SEMILLA: u128 = 100 * TOKEN_UNIT;
pub const CAP_PYME: u128 = 500 * TOKEN_UNIT;
pub const CAP_MAYORISTA: u128 = 2_500 * TOKEN_UNIT;

/// round(ln(1 + k/16) * 1000) for k = 0..=16.
const LN_TABLE: [u64; 17] = [
    0, 61, 118, 172, 223, 272, 318, 363, 405, 446, 486, 523, 560, 595, 629, 661, 693,
];
const LN2_MILLI: u64 = 693;

/// Natural logarithm of `x` in milli-nats (about floor(ln(x) * 1000)). `ln(1) = 0`.
pub fn ln_milli(x: u128) -> u64 {
    if x <= 1 {
        return 0;
    }
    let n = x.ilog2();
    let base = 1u128 << n;
    let rem = x - base;
    // k = floor(rem * 16 / 2^n) without overflowing for large x.
    let k = (if n >= 4 {
        rem >> (n - 4)
    } else {
        rem << (4 - n)
    }) as usize;
    u64::from(n) * LN2_MILLI + LN_TABLE[k.min(16)]
}

/// Score gained by a member who paid on time in a round with `contribution` units.
pub fn on_time_delta(contribution: u128) -> u64 {
    ALPHA + BETA * ln_milli(1 + contribution / TOKEN_UNIT) / 1000
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tier {
    Semilla,
    PyME,
    Mayorista,
}

impl Tier {
    pub fn for_score(score: u64) -> Self {
        if score >= TIER_MAYORISTA_MIN_SCORE {
            Tier::Mayorista
        } else if score >= TIER_PYME_MIN_SCORE {
            Tier::PyME
        } else {
            Tier::Semilla
        }
    }

    /// Largest contribution (in token units) a member of this tier may commit to.
    pub fn cap(&self) -> u128 {
        match self {
            Tier::Semilla => CAP_SEMILLA,
            Tier::PyME => CAP_PYME,
            Tier::Mayorista => CAP_MAYORISTA,
        }
    }

    pub fn next_threshold(&self) -> Option<u64> {
        match self {
            Tier::Semilla => Some(TIER_PYME_MIN_SCORE),
            Tier::PyME => Some(TIER_MAYORISTA_MIN_SCORE),
            Tier::Mayorista => None,
        }
    }
}

pub fn tier_cap_for_score(score: u64) -> u128 {
    Tier::for_score(score).cap()
}

#[near(serializers = [json])]
pub struct TierView {
    pub score: u64,
    pub tier: Tier,
    pub cap: U128,
    pub next_tier_at: Option<u64>,
}

impl TierView {
    pub fn for_score(score: u64) -> Self {
        let tier = Tier::for_score(score);
        Self {
            score,
            cap: U128(tier.cap()),
            next_tier_at: tier.next_threshold(),
            tier,
        }
    }
}

impl Reputation {
    pub fn apply_on_time(&mut self, contribution: u128, now: u64) {
        self.score = self.score.saturating_add(on_time_delta(contribution));
        self.on_time = self.on_time.saturating_add(1);
        self.updated_at = now.into();
    }

    pub fn apply_default(&mut self, now: u64) {
        self.score = self.score.saturating_sub(GAMMA);
        self.defaults = self.defaults.saturating_add(1);
        self.updated_at = now.into();
    }

    /// Removal by the DAO costs the same as a default, without counting as one.
    pub fn apply_removed(&mut self, now: u64) {
        self.score = self.score.saturating_sub(GAMMA);
        self.updated_at = now.into();
    }

    pub fn apply_completed(&mut self, now: u64) {
        self.circles_completed = self.circles_completed.saturating_add(1);
        self.updated_at = now.into();
    }
}

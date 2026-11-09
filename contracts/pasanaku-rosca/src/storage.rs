//! Storage accounting for circles. The creator pays once, up front, for the
//! state a circle of `n` members can grow to (rounds, bids, payouts).

use near_sdk::{NearToken, env};

/// Conservative upper bound: fixed overhead + per-member state + per-round
/// payout rows (one round per member, N payouts per round).
pub fn storage_bytes_for_members(n: u64) -> u64 {
    3_000 + 200 * n + 200 * n * n
}

pub fn storage_cost_for_members(n: u64) -> NearToken {
    env::storage_byte_cost().saturating_mul(u128::from(storage_bytes_for_members(n)))
}

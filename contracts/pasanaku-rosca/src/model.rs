//! State types, constants and small pure helpers shared by every module.

use near_sdk::json_types::{U64, U128};
use near_sdk::{AccountId, BorshStorageKey, near};

/// Contract version reported by `get_config`.
pub const VERSION: &str = "0.1.0";
/// NEP-297 event standard name and version.
pub const EVENT_STANDARD: &str = "pasanaku";
pub const EVENT_VERSION: &str = "1.0.0";

pub const MIN_MEMBERS: usize = 2;
pub const MAX_MEMBERS: usize = 20;
/// Shortest round allowed (demo friendly, still protects against zero deadlines).
pub const MIN_ROUND_DURATION_NS: u64 = 60 * 1_000_000_000;
/// Payouts scheduled per `execute_payouts` call (each one is a cross-contract call).
pub const PAYOUT_BATCH_MAX: u8 = 10;
/// Smallest units per whole token (USDC has 6 decimals).
pub const TOKEN_UNIT: u128 = 1_000_000;
pub const CIRCLE_ID_MIN_LEN: usize = 3;
pub const CIRCLE_ID_MAX_LEN: usize = 32;

#[near(serializers = [borsh])]
#[derive(BorshStorageKey)]
pub enum StorageKey {
    Circles,
    Rounds,
    Reputation,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CircleStatus {
    /// Created by a member, waiting for the DAO to admit it.
    Pending,
    Active,
    /// Paused by the DAO or after a default; funds stay claimable.
    Frozen,
    Completed,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PayoutMode {
    /// The contract pays every recipient with `ft_transfer`.
    Direct,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub account: AccountId,
    /// True once this member has received a pot.
    pub collected: bool,
    pub joined_round: u32,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug)]
pub struct Circle {
    pub id: String,
    pub creator: AccountId,
    /// Sputnik DAO that governs this circle (admission, bids, freeze, params).
    pub dao_account: AccountId,
    /// Treasurer agent allowed to retry payouts on behalf of members.
    pub agent_account: AccountId,
    /// NEP-141 token every contribution and payout is denominated in.
    pub token: AccountId,
    pub contribution: U128,
    pub round_duration_ns: U64,
    pub min_reputation: u64,
    pub max_discount: U128,
    pub members: Vec<Member>,
    pub status: CircleStatus,
    pub current_round: u32,
    pub payout_mode: PayoutMode,
    pub created_at: U64,
    pub frozen_reason: Option<String>,
    pub storage_paid: U128,
}

impl Circle {
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    pub fn member(&self, account: &AccountId) -> Option<&Member> {
        self.members.iter().find(|m| &m.account == account)
    }

    pub fn member_mut(&mut self, account: &AccountId) -> Option<&mut Member> {
        self.members.iter_mut().find(|m| &m.account == account)
    }

    pub fn is_member(&self, account: &AccountId) -> bool {
        self.member(account).is_some()
    }

    /// First member (in list order) who has not collected a pot yet.
    pub fn next_in_rotation(&self) -> Option<AccountId> {
        self.members
            .iter()
            .find(|m| !m.collected)
            .map(|m| m.account.clone())
    }

    pub fn all_collected(&self) -> bool {
        self.members.iter().all(|m| m.collected)
    }

    pub fn round_key(&self, index: u32) -> String {
        round_key(&self.id, index)
    }
}

pub fn round_key(circle_id: &str, index: u32) -> String {
    format!("{circle_id}#{index}")
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoundStatus {
    Collecting,
    Closed,
    /// Deadline passed with unpaid members: contributions are refunded.
    Voided,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bid {
    pub bidder: AccountId,
    pub discount: U128,
    /// Set by the DAO through `approve_bid`.
    pub approved: bool,
    pub submitted_at: U64,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PayoutKind {
    Pot,
    Dividend,
    Refund,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PayoutStatus {
    Pending,
    Sent,
    Confirmed,
    Failed,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Payout {
    pub recipient: AccountId,
    pub amount: U128,
    pub kind: PayoutKind,
    pub status: PayoutStatus,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug)]
pub struct Round {
    pub index: u32,
    pub started_at: U64,
    pub deadline: U64,
    pub status: RoundStatus,
    pub paid: Vec<AccountId>,
    pub bids: Vec<Bid>,
    pub winner: Option<AccountId>,
    pub discount: U128,
    pub pot: U128,
    pub payouts: Vec<Payout>,
    pub defaulters: Vec<AccountId>,
    pub closed_at: Option<U64>,
}

impl Round {
    pub fn open(index: u32, now: u64, duration_ns: u64) -> Self {
        Self {
            index,
            started_at: U64(now),
            deadline: U64(now.saturating_add(duration_ns)),
            status: RoundStatus::Collecting,
            paid: Vec::new(),
            bids: Vec::new(),
            winner: None,
            discount: U128(0),
            pot: U128(0),
            payouts: Vec::new(),
            defaulters: Vec::new(),
            closed_at: None,
        }
    }

    pub fn pending_payouts(&self) -> u32 {
        self.payouts
            .iter()
            .filter(|p| p.status == PayoutStatus::Pending)
            .count() as u32
    }
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reputation {
    pub score: u64,
    pub on_time: u32,
    pub defaults: u32,
    pub circles_completed: u32,
    pub updated_at: U64,
}

/// JSON payload accepted in `ft_transfer_call` messages.
#[near(serializers = [json])]
pub struct ContributionMsg {
    pub circle_id: String,
    /// Member being paid for. Defaults to the token sender.
    pub beneficiary: Option<AccountId>,
}

/// Live snapshot of the current round, including the projected outcome.
#[near(serializers = [json])]
pub struct RoundStatusView {
    pub circle_id: String,
    pub circle_status: CircleStatus,
    pub round_index: u32,
    pub round_status: RoundStatus,
    pub contribution: U128,
    pub pot: U128,
    pub paid: Vec<AccountId>,
    pub unpaid: Vec<AccountId>,
    pub bids: Vec<Bid>,
    pub deadline: U64,
    pub now: U64,
    pub can_close: bool,
    pub projected_winner: Option<AccountId>,
    pub projected_discount: U128,
    pub projected_dividend_each: U128,
}

#[near(serializers = [json])]
pub struct ConfigView {
    pub version: String,
    pub alpha: u64,
    pub beta: u64,
    pub gamma: u64,
    pub tier_pyme_min_score: u64,
    pub tier_mayorista_min_score: u64,
    pub cap_semilla: U128,
    pub cap_pyme: U128,
    pub cap_mayorista: U128,
    pub min_members: u32,
    pub max_members: u32,
    pub payout_batch_max: u8,
    pub token_unit: U128,
}

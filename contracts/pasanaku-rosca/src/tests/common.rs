//! Shared fixtures: a 4-member circle governed by `dao()` with token `usdc()`.

use near_sdk::json_types::{U64, U128};
use near_sdk::test_utils::{VMContextBuilder, accounts, get_logs};
use near_sdk::{AccountId, NearToken, testing_env};

use crate::Contract;
use crate::model::*;

pub const CIRCLE: &str = "tanda-demo";
/// 10 USDC per round.
pub const CONTRIBUTION: u128 = 10 * TOKEN_UNIT;
/// 5 USDC maximum discount (pot is 40 USDC with four members).
pub const MAX_DISCOUNT: u128 = 5 * TOKEN_UNIT;
pub const ROUND_NS: u64 = 3_600 * 1_000_000_000;
pub const T0: u64 = 1_700_000_000_000_000_000;

pub fn dao() -> AccountId {
    "dao.pasanaku.testnet".parse().unwrap()
}

pub fn agent() -> AccountId {
    "agent.pasanaku.testnet".parse().unwrap()
}

pub fn usdc() -> AccountId {
    "usdc.fakes.testnet".parse().unwrap()
}

pub fn members() -> Vec<AccountId> {
    vec![accounts(0), accounts(1), accounts(2), accounts(3)]
}

pub fn context(predecessor: AccountId, now: u64) -> VMContextBuilder {
    let mut b = VMContextBuilder::new();
    b.current_account_id("rosca.pasanaku.testnet".parse().unwrap())
        .predecessor_account_id(predecessor.clone())
        .signer_account_id(predecessor)
        .prepaid_gas(near_sdk::Gas::from_tgas(300))
        .block_timestamp(now);
    b
}

pub fn set_caller(predecessor: AccountId, now: u64) {
    testing_env!(context(predecessor, now).build());
}

pub fn set_caller_with_deposit(predecessor: AccountId, now: u64, deposit: NearToken) {
    testing_env!(context(predecessor, now).attached_deposit(deposit).build());
}

pub fn new_contract() -> Contract {
    set_caller(accounts(5), T0);
    Contract::new(accounts(5))
}

pub fn storage_deposit_for(n: u32) -> NearToken {
    set_caller(accounts(0), T0);
    crate::storage::storage_cost_for_members(u64::from(n))
}

/// Create the standard pending circle as `accounts(0)` with the exact deposit.
pub fn create_default_circle(contract: &mut Contract) {
    let deposit = storage_deposit_for(4);
    set_caller_with_deposit(accounts(0), T0, deposit);
    contract.create_circle(
        CIRCLE.to_string(),
        dao(),
        agent(),
        usdc(),
        U128(CONTRIBUTION),
        U64(ROUND_NS),
        0,
        U128(MAX_DISCOUNT),
        members(),
        None,
    );
}

pub fn active_circle() -> Contract {
    let mut c = new_contract();
    create_default_circle(&mut c);
    set_caller(dao(), T0);
    c.activate_circle(CIRCLE.to_string());
    c
}

/// Simulate the token contract delivering `amount` for `beneficiary`.
pub fn pay(
    contract: &mut Contract,
    payer: AccountId,
    beneficiary: Option<AccountId>,
    amount: u128,
    now: u64,
) -> u128 {
    use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
    set_caller(usdc(), now);
    let msg = match beneficiary {
        Some(b) => format!("{{\"circle_id\":\"{CIRCLE}\",\"beneficiary\":\"{b}\"}}"),
        None => format!("{{\"circle_id\":\"{CIRCLE}\"}}"),
    };
    match contract.ft_on_transfer(payer, U128(amount), msg) {
        near_sdk::PromiseOrValue::Value(v) => v.0,
        near_sdk::PromiseOrValue::Promise(_) => panic!("unexpected promise"),
    }
}

pub fn pay_all(contract: &mut Contract, now: u64) {
    for m in members() {
        pay(contract, m, None, CONTRIBUTION, now);
    }
}

pub fn current_round(contract: &Contract) -> Round {
    let circle = contract.get_circle(CIRCLE.to_string()).unwrap();
    contract
        .get_round(CIRCLE.to_string(), circle.current_round)
        .unwrap()
}

pub fn events_named(name: &str) -> Vec<String> {
    get_logs()
        .into_iter()
        .filter(|l| l.starts_with("EVENT_JSON:") && l.contains(&format!("\"event\":\"{name}\"")))
        .collect()
}

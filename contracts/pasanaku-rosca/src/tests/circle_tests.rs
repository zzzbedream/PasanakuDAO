use near_sdk::json_types::{U64, U128};
use near_sdk::test_utils::{accounts, get_created_receipts};
use near_sdk::{AccountId, NearToken};

use super::common::*;
use crate::model::*;

fn create_with(
    contract: &mut crate::Contract,
    members: Vec<AccountId>,
    contribution: u128,
    max_discount: u128,
    min_rep: u64,
) {
    let deposit = storage_deposit_for(members.len() as u32);
    set_caller_with_deposit(accounts(0), T0, deposit);
    contract.create_circle(
        CIRCLE.to_string(),
        dao(),
        agent(),
        usdc(),
        U128(contribution),
        U64(ROUND_NS),
        min_rep,
        U128(max_discount),
        members,
        None,
    );
}

#[test]
fn create_circle_ok_pending() {
    let mut c = new_contract();
    create_default_circle(&mut c);
    let circle = c.get_circle(CIRCLE.to_string()).expect("circle stored");
    assert_eq!(circle.status, CircleStatus::Pending);
    assert_eq!(circle.member_count(), 4);
    assert_eq!(circle.creator, accounts(0));
    assert_eq!(circle.storage_paid.0, storage_deposit_for(4).as_yoctonear());
    assert_eq!(c.get_circle_count(), 1);
    assert!(
        c.get_round(CIRCLE.to_string(), 0).is_none(),
        "no round before activation"
    );
}

#[test]
#[should_panic(expected = "Creator must be a member")]
fn create_circle_creator_must_be_member() {
    let mut c = new_contract();
    create_with(&mut c, vec![accounts(1), accounts(2)], CONTRIBUTION, 0, 0);
}

#[test]
#[should_panic(expected = "Contribution exceeds a member's tier cap")]
fn create_circle_contribution_above_semilla_cap() {
    let mut c = new_contract();
    create_with(&mut c, members(), 150 * TOKEN_UNIT, 0, 0);
}

#[test]
#[should_panic(expected = "A member is below the required reputation")]
fn create_circle_member_below_min_reputation() {
    let mut c = new_contract();
    create_with(&mut c, members(), CONTRIBUTION, 0, 50);
}

#[test]
#[should_panic(expected = "Circle id already exists")]
fn create_circle_duplicate_id() {
    let mut c = new_contract();
    create_default_circle(&mut c);
    create_default_circle(&mut c);
}

#[test]
#[should_panic(expected = "Duplicate member")]
fn create_circle_duplicate_member() {
    let mut c = new_contract();
    create_with(
        &mut c,
        vec![accounts(0), accounts(1), accounts(1)],
        CONTRIBUTION,
        0,
        0,
    );
}

#[test]
#[should_panic(expected = "Member count out of bounds")]
fn create_circle_bounds_min_members() {
    let mut c = new_contract();
    create_with(&mut c, vec![accounts(0)], CONTRIBUTION, 0, 0);
}

#[test]
#[should_panic(expected = "max_discount must not exceed half of the pot")]
fn create_circle_max_discount_bound() {
    let mut c = new_contract();
    create_with(&mut c, members(), CONTRIBUTION, 21 * TOKEN_UNIT, 0);
}

#[test]
#[should_panic(expected = "Insufficient storage deposit")]
fn create_circle_insufficient_storage_panics() {
    let mut c = new_contract();
    set_caller_with_deposit(accounts(0), T0, NearToken::from_yoctonear(1));
    c.create_circle(
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

#[test]
fn create_circle_refunds_excess_storage() {
    let mut c = new_contract();
    let expected_cost = storage_deposit_for(4);
    set_caller_with_deposit(accounts(0), T0, NearToken::from_near(1));
    c.create_circle(
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
    assert_eq!(
        get_created_receipts().len(),
        1,
        "one refund transfer scheduled"
    );
    let circle = c.get_circle(CIRCLE.to_string()).unwrap();
    assert_eq!(circle.storage_paid.0, expected_cost.as_yoctonear());
}

#[test]
#[should_panic(expected = "circle_id may only contain")]
fn create_circle_rejects_invalid_id() {
    let mut c = new_contract();
    set_caller_with_deposit(accounts(0), T0, storage_deposit_for(4));
    c.create_circle(
        "Tanda_Demo".to_string(),
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

#[test]
#[should_panic(expected = "Round duration too short")]
fn create_circle_rejects_short_round() {
    let mut c = new_contract();
    set_caller_with_deposit(accounts(0), T0, storage_deposit_for(4));
    c.create_circle(
        CIRCLE.to_string(),
        dao(),
        agent(),
        usdc(),
        U128(CONTRIBUTION),
        U64(1),
        0,
        U128(MAX_DISCOUNT),
        members(),
        None,
    );
}

#[test]
#[should_panic(expected = "Only the governing DAO")]
fn activate_only_dao() {
    let mut c = new_contract();
    create_default_circle(&mut c);
    set_caller(accounts(0), T0);
    c.activate_circle(CIRCLE.to_string());
}

#[test]
fn activate_opens_round0_with_deadline() {
    let c = active_circle();
    let circle = c.get_circle(CIRCLE.to_string()).unwrap();
    assert_eq!(circle.status, CircleStatus::Active);
    assert_eq!(circle.current_round, 0);
    let round = c.get_round(CIRCLE.to_string(), 0).expect("round 0 opened");
    assert_eq!(round.status, RoundStatus::Collecting);
    assert_eq!(round.deadline.0, T0 + ROUND_NS);
    assert_eq!(round.pot.0, 0);
}

#[test]
#[should_panic(expected = "Circle is not pending")]
fn activate_twice_panics() {
    let mut c = active_circle();
    set_caller(dao(), T0);
    c.activate_circle(CIRCLE.to_string());
}

#[test]
fn freeze_and_unfreeze_by_dao_keep_collecting_round() {
    let mut c = active_circle();
    set_caller(dao(), T0);
    c.freeze_circle(CIRCLE.to_string(), Some("audit".to_string()));
    let frozen = c.get_circle(CIRCLE.to_string()).unwrap();
    assert_eq!(frozen.status, CircleStatus::Frozen);
    assert_eq!(frozen.frozen_reason.as_deref(), Some("audit"));

    set_caller(dao(), T0);
    c.unfreeze_circle(CIRCLE.to_string());
    let active = c.get_circle(CIRCLE.to_string()).unwrap();
    assert_eq!(active.status, CircleStatus::Active);
    assert_eq!(
        active.current_round, 0,
        "collecting round is resumed, not replaced"
    );
    assert!(active.frozen_reason.is_none());
}

#[test]
#[should_panic(expected = "Only the governing DAO")]
fn freeze_requires_dao() {
    let mut c = active_circle();
    set_caller(agent(), T0);
    c.freeze_circle(CIRCLE.to_string(), None);
}

#[test]
#[should_panic(expected = "Current round is still collecting")]
fn remove_member_requires_non_collecting_round() {
    let mut c = active_circle();
    set_caller(dao(), T0);
    c.freeze_circle(CIRCLE.to_string(), None);
    set_caller(dao(), T0);
    c.remove_member(CIRCLE.to_string(), accounts(3));
}

#[test]
fn set_params_updates_fields_for_next_round() {
    let mut c = active_circle();
    set_caller(dao(), T0);
    c.set_params(
        CIRCLE.to_string(),
        Some(U64(2 * ROUND_NS)),
        Some(U128(2 * TOKEN_UNIT)),
        Some(5),
    );
    let circle = c.get_circle(CIRCLE.to_string()).unwrap();
    assert_eq!(circle.round_duration_ns.0, 2 * ROUND_NS);
    assert_eq!(circle.max_discount.0, 2 * TOKEN_UNIT);
    assert_eq!(circle.min_reputation, 5);
    let round = c.get_round(CIRCLE.to_string(), 0).unwrap();
    assert_eq!(
        round.deadline.0,
        T0 + ROUND_NS,
        "open round keeps its deadline"
    );
}

#[test]
#[should_panic(expected = "max_discount too high")]
fn set_params_rejects_discount_above_half_pot() {
    let mut c = active_circle();
    set_caller(dao(), T0);
    c.set_params(CIRCLE.to_string(), None, Some(U128(30 * TOKEN_UNIT)), None);
}

#[test]
fn views_pagination_and_config() {
    let c = active_circle();
    assert_eq!(c.get_circles(0, 10).len(), 1);
    assert!(c.get_circles(1, 10).is_empty());
    assert_eq!(c.get_rounds(CIRCLE.to_string(), 0, 10).len(), 1);
    assert!(c.get_rounds("missing".to_string(), 0, 10).is_empty());
    let config = c.get_config();
    assert_eq!(config.max_members, MAX_MEMBERS as u32);
    assert_eq!(config.cap_semilla.0, 100 * TOKEN_UNIT);
    assert_eq!(
        c.storage_cost_for_members(4).0,
        storage_deposit_for(4).as_yoctonear()
    );
}

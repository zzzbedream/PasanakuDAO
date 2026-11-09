use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
use near_sdk::json_types::U128;
use near_sdk::test_utils::accounts;

use super::common::*;
use crate::model::*;

#[test]
#[should_panic(expected = "Wrong token contract")]
fn ft_on_transfer_wrong_token_panics() {
    let mut c = active_circle();
    set_caller(accounts(4), T0);
    let msg = format!("{{\"circle_id\":\"{CIRCLE}\"}}");
    let _ = c.ft_on_transfer(accounts(0), U128(CONTRIBUTION), msg);
}

#[test]
fn ft_on_transfer_exact_marks_paid() {
    let mut c = active_circle();
    let unused = pay(&mut c, accounts(0), None, CONTRIBUTION, T0 + 1);
    assert_eq!(unused, 0);
    let round = current_round(&c);
    assert_eq!(round.paid, vec![accounts(0)]);
    assert_eq!(round.pot.0, CONTRIBUTION);
    assert_eq!(events_named("contribution_received").len(), 1);
}

#[test]
fn ft_on_transfer_excess_returned() {
    let mut c = active_circle();
    let unused = pay(
        &mut c,
        accounts(1),
        None,
        CONTRIBUTION + 5 * TOKEN_UNIT,
        T0 + 1,
    );
    assert_eq!(unused, 5 * TOKEN_UNIT);
    assert_eq!(current_round(&c).pot.0, CONTRIBUTION);
}

#[test]
#[should_panic(expected = "Amount below the fixed contribution")]
fn ft_on_transfer_insufficient_panics() {
    let mut c = active_circle();
    pay(&mut c, accounts(0), None, CONTRIBUTION - 1, T0 + 1);
}

#[test]
#[should_panic(expected = "Member already paid this round")]
fn ft_on_transfer_double_pay_panics() {
    let mut c = active_circle();
    pay(&mut c, accounts(0), None, CONTRIBUTION, T0 + 1);
    pay(&mut c, accounts(0), None, CONTRIBUTION, T0 + 2);
}

#[test]
#[should_panic(expected = "Beneficiary is not a member")]
fn ft_on_transfer_non_member_beneficiary_panics() {
    let mut c = active_circle();
    pay(&mut c, accounts(4), None, CONTRIBUTION, T0 + 1);
}

#[test]
fn ft_on_transfer_third_party_beneficiary_ok() {
    let mut c = active_circle();
    let unused = pay(&mut c, agent(), Some(accounts(1)), CONTRIBUTION, T0 + 1);
    assert_eq!(unused, 0);
    assert_eq!(current_round(&c).paid, vec![accounts(1)]);
}

#[test]
#[should_panic(expected = "Circle is not active")]
fn ft_on_transfer_pending_circle_panics() {
    let mut c = new_contract();
    create_default_circle(&mut c);
    pay(&mut c, accounts(0), None, CONTRIBUTION, T0 + 1);
}

#[test]
#[should_panic(expected = "Circle is not active")]
fn ft_on_transfer_frozen_circle_panics() {
    let mut c = active_circle();
    set_caller(dao(), T0);
    c.freeze_circle(CIRCLE.to_string(), None);
    pay(&mut c, accounts(0), None, CONTRIBUTION, T0 + 1);
}

#[test]
#[should_panic(expected = "msg must be JSON")]
fn ft_on_transfer_bad_msg_panics() {
    let mut c = active_circle();
    set_caller(usdc(), T0);
    let _ = c.ft_on_transfer(accounts(0), U128(CONTRIBUTION), "tanda-demo".to_string());
}

#[test]
#[should_panic(expected = "Circle not found")]
fn ft_on_transfer_unknown_circle_panics() {
    let mut c = active_circle();
    set_caller(usdc(), T0);
    let _ = c.ft_on_transfer(
        accounts(0),
        U128(CONTRIBUTION),
        "{\"circle_id\":\"nope\"}".to_string(),
    );
}

#[test]
fn round_status_reflects_payments_and_projection() {
    let mut c = active_circle();
    pay(&mut c, accounts(0), None, CONTRIBUTION, T0 + 1);
    pay(&mut c, accounts(2), None, CONTRIBUTION, T0 + 2);
    set_caller(accounts(5), T0 + 3);
    let status = c
        .get_round_status(CIRCLE.to_string())
        .expect("active circle has a round");
    assert_eq!(status.paid.len(), 2);
    assert_eq!(status.unpaid, vec![accounts(1), accounts(3)]);
    assert_eq!(status.pot.0, 2 * CONTRIBUTION);
    assert!(!status.can_close);
    assert_eq!(
        status.projected_winner,
        Some(accounts(0)),
        "rotation order without bids"
    );
    assert_eq!(status.projected_discount.0, 0);
    assert_eq!(status.circle_status, CircleStatus::Active);
}

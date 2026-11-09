use near_sdk::AccountId;
use near_sdk::json_types::U128;
use near_sdk::test_utils::{accounts, get_created_receipts};

use super::common::*;
use crate::model::*;

fn bid(contract: &mut crate::Contract, bidder: AccountId, discount: u128, now: u64) {
    set_caller(bidder, now);
    contract.submit_bid(CIRCLE.to_string(), U128(discount));
}

fn approve(contract: &mut crate::Contract, bidder: AccountId, discount: u128, now: u64) {
    set_caller(dao(), now);
    contract.approve_bid(CIRCLE.to_string(), bidder, U128(discount));
}

fn close(contract: &mut crate::Contract, now: u64) {
    set_caller(agent(), now);
    contract.close_round(CIRCLE.to_string());
}

fn amount_for(round: &Round, account: &AccountId) -> u128 {
    round
        .payouts
        .iter()
        .filter(|p| &p.recipient == account)
        .map(|p| p.amount.0)
        .sum()
}

#[test]
#[should_panic(expected = "Round is still collecting")]
fn close_round_before_conditions_panics() {
    let mut c = active_circle();
    pay(&mut c, accounts(0), None, CONTRIBUTION, T0 + 1);
    close(&mut c, T0 + 2);
}

#[test]
fn close_round_rotation_without_bids() {
    let mut c = active_circle();
    pay_all(&mut c, T0 + 1);
    close(&mut c, T0 + 2);
    let round0 = c.get_round(CIRCLE.to_string(), 0).unwrap();
    assert_eq!(round0.status, RoundStatus::Closed);
    assert_eq!(round0.winner, Some(accounts(0)));
    assert_eq!(round0.discount.0, 0);
    assert_eq!(round0.payouts.len(), 1);
    assert_eq!(round0.payouts[0].kind, PayoutKind::Pot);
    assert_eq!(round0.payouts[0].amount.0, 4 * CONTRIBUTION);
    let circle = c.get_circle(CIRCLE.to_string()).unwrap();
    assert_eq!(circle.current_round, 1);
    assert!(circle.member(&accounts(0)).unwrap().collected);
    assert_eq!(
        c.get_round(CIRCLE.to_string(), 1).unwrap().status,
        RoundStatus::Collecting
    );
    assert_eq!(events_named("round_closed").len(), 1);
}

#[test]
fn close_round_payout_math_n4_d3() {
    let mut c = active_circle();
    bid(&mut c, accounts(2), 3 * TOKEN_UNIT, T0 + 1);
    approve(&mut c, accounts(2), 3 * TOKEN_UNIT, T0 + 2);
    pay_all(&mut c, T0 + 3);
    close(&mut c, T0 + 4);
    let round = c.get_round(CIRCLE.to_string(), 0).unwrap();
    assert_eq!(round.winner, Some(accounts(2)));
    assert_eq!(round.discount.0, 3 * TOKEN_UNIT);
    assert_eq!(amount_for(&round, &accounts(2)), 37 * TOKEN_UNIT);
    for other in [accounts(0), accounts(1), accounts(3)] {
        assert_eq!(amount_for(&round, &other), TOKEN_UNIT);
    }
    let total: u128 = round.payouts.iter().map(|p| p.amount.0).sum();
    assert_eq!(total, 4 * CONTRIBUTION);
    assert_eq!(round.payouts.len(), 4);
}

#[test]
fn close_round_dust_goes_to_winner_n4_d4() {
    let mut c = active_circle();
    bid(&mut c, accounts(1), 4 * TOKEN_UNIT, T0 + 1);
    approve(&mut c, accounts(1), 4 * TOKEN_UNIT, T0 + 2);
    pay_all(&mut c, T0 + 3);
    close(&mut c, T0 + 4);
    let round = c.get_round(CIRCLE.to_string(), 0).unwrap();
    // 4 USDC / 3 = 1.333333 each, 1 micro-unit of dust goes to the winner.
    assert_eq!(
        amount_for(&round, &accounts(1)),
        36 * TOKEN_UNIT + 1,
        "pot - discount + dust"
    );
    for other in [accounts(0), accounts(2), accounts(3)] {
        assert_eq!(amount_for(&round, &other), 1_333_333);
    }
    let total: u128 = round.payouts.iter().map(|p| p.amount.0).sum();
    assert_eq!(total, 4 * CONTRIBUTION);
}

#[test]
fn close_round_highest_approved_beats_higher_unapproved() {
    let mut c = active_circle();
    bid(&mut c, accounts(1), 2 * TOKEN_UNIT, T0 + 1);
    bid(&mut c, accounts(2), 4 * TOKEN_UNIT, T0 + 2);
    approve(&mut c, accounts(1), 2 * TOKEN_UNIT, T0 + 3);
    pay_all(&mut c, T0 + 4);
    close(&mut c, T0 + 5);
    let round = c.get_round(CIRCLE.to_string(), 0).unwrap();
    assert_eq!(round.winner, Some(accounts(1)));
    assert_eq!(round.discount.0, 2 * TOKEN_UNIT);
}

#[test]
#[should_panic(expected = "Only members can bid")]
fn submit_bid_rejects_non_member() {
    let mut c = active_circle();
    bid(&mut c, accounts(4), TOKEN_UNIT, T0 + 1);
}

#[test]
#[should_panic(expected = "Discount out of bounds")]
fn submit_bid_rejects_above_max() {
    let mut c = active_circle();
    bid(&mut c, accounts(1), MAX_DISCOUNT + 1, T0 + 1);
}

#[test]
#[should_panic(expected = "Bid must exceed the current highest bid")]
fn submit_bid_rejects_not_higher() {
    let mut c = active_circle();
    bid(&mut c, accounts(1), 2 * TOKEN_UNIT, T0 + 1);
    bid(&mut c, accounts(2), 2 * TOKEN_UNIT, T0 + 2);
}

#[test]
fn submit_bid_lets_member_raise_own_bid() {
    let mut c = active_circle();
    bid(&mut c, accounts(1), TOKEN_UNIT, T0 + 1);
    bid(&mut c, accounts(1), 2 * TOKEN_UNIT, T0 + 2);
    let round = current_round(&c);
    assert_eq!(round.bids.len(), 1);
    assert_eq!(round.bids[0].discount.0, 2 * TOKEN_UNIT);
}

#[test]
#[should_panic(expected = "Member already collected a pot")]
fn submit_bid_rejects_member_who_collected() {
    let mut c = active_circle();
    pay_all(&mut c, T0 + 1);
    close(&mut c, T0 + 2);
    bid(&mut c, accounts(0), TOKEN_UNIT, T0 + 3);
}

#[test]
#[should_panic(expected = "Only the governing DAO")]
fn approve_bid_requires_dao() {
    let mut c = active_circle();
    bid(&mut c, accounts(1), TOKEN_UNIT, T0 + 1);
    set_caller(agent(), T0 + 2);
    c.approve_bid(CIRCLE.to_string(), accounts(1), U128(TOKEN_UNIT));
}

#[test]
#[should_panic(expected = "No matching bid")]
fn approve_bid_requires_exact_discount() {
    let mut c = active_circle();
    bid(&mut c, accounts(1), TOKEN_UNIT, T0 + 1);
    approve(&mut c, accounts(1), 2 * TOKEN_UNIT, T0 + 2);
}

#[test]
fn close_round_after_deadline_voids_refunds_penalizes_and_freezes() {
    let mut c = active_circle();
    for m in [accounts(0), accounts(1), accounts(2)] {
        pay(&mut c, m, None, CONTRIBUTION, T0 + 1);
    }
    close(&mut c, T0 + ROUND_NS + 1);
    let round = c.get_round(CIRCLE.to_string(), 0).unwrap();
    assert_eq!(round.status, RoundStatus::Voided);
    assert_eq!(round.defaulters, vec![accounts(3)]);
    assert_eq!(round.payouts.len(), 3);
    assert!(
        round
            .payouts
            .iter()
            .all(|p| p.kind == PayoutKind::Refund && p.amount.0 == CONTRIBUTION)
    );
    let circle = c.get_circle(CIRCLE.to_string()).unwrap();
    assert_eq!(circle.status, CircleStatus::Frozen);
    assert_eq!(circle.frozen_reason.as_deref(), Some("default"));
    let rep = c.get_reputation(accounts(3));
    assert_eq!(rep.defaults, 1);
    assert_eq!(rep.score, 0);
    assert_eq!(
        c.get_reputation(accounts(0)).on_time,
        0,
        "voided rounds give no credit"
    );
}

#[test]
fn after_void_dao_removes_member_and_unfreeze_opens_new_round() {
    let mut c = active_circle();
    for m in [accounts(0), accounts(1), accounts(2)] {
        pay(&mut c, m, None, CONTRIBUTION, T0 + 1);
    }
    close(&mut c, T0 + ROUND_NS + 1);
    set_caller(dao(), T0 + ROUND_NS + 2);
    c.remove_member(CIRCLE.to_string(), accounts(3));
    set_caller(dao(), T0 + ROUND_NS + 3);
    c.unfreeze_circle(CIRCLE.to_string());
    let circle = c.get_circle(CIRCLE.to_string()).unwrap();
    assert_eq!(circle.status, CircleStatus::Active);
    assert_eq!(circle.member_count(), 3);
    assert_eq!(circle.current_round, 1);
    let round1 = c.get_round(CIRCLE.to_string(), 1).unwrap();
    assert_eq!(round1.status, RoundStatus::Collecting);
    assert_eq!(round1.deadline.0, T0 + ROUND_NS + 3 + ROUND_NS);
}

#[test]
fn reputation_rises_for_every_payer_after_close() {
    let mut c = active_circle();
    pay_all(&mut c, T0 + 1);
    close(&mut c, T0 + 2);
    for m in members() {
        let rep = c.get_reputation(m);
        assert_eq!(rep.score, 21);
        assert_eq!(rep.on_time, 1);
    }
    assert_eq!(events_named("reputation_updated").len(), 4);
}

#[test]
fn execute_payouts_batches_and_reports_remaining() {
    let mut c = active_circle();
    bid(&mut c, accounts(2), 3 * TOKEN_UNIT, T0 + 1);
    approve(&mut c, accounts(2), 3 * TOKEN_UNIT, T0 + 2);
    pay_all(&mut c, T0 + 3);
    close(&mut c, T0 + 4);
    set_caller(agent(), T0 + 5);
    let remaining = c.execute_payouts(CIRCLE.to_string(), 0, Some(2));
    assert_eq!(remaining, 2);
    assert!(
        get_created_receipts().len() >= 2,
        "ft_transfer promises scheduled"
    );
    let round = c.get_round(CIRCLE.to_string(), 0).unwrap();
    let sent = round
        .payouts
        .iter()
        .filter(|p| p.status == PayoutStatus::Sent)
        .count();
    assert_eq!(sent, 2);
    set_caller(agent(), T0 + 6);
    assert_eq!(c.execute_payouts(CIRCLE.to_string(), 0, None), 0);
    set_caller(agent(), T0 + 7);
    assert_eq!(
        c.execute_payouts(CIRCLE.to_string(), 0, None),
        0,
        "idempotent when nothing pending"
    );
}

#[test]
#[should_panic(expected = "Round is not closed")]
fn execute_payouts_on_collecting_round_panics() {
    let mut c = active_circle();
    set_caller(agent(), T0 + 1);
    c.execute_payouts(CIRCLE.to_string(), 0, None);
}

#[test]
fn payout_results_confirm_fail_and_claim() {
    let mut c = active_circle();
    bid(&mut c, accounts(2), 3 * TOKEN_UNIT, T0 + 1);
    approve(&mut c, accounts(2), 3 * TOKEN_UNIT, T0 + 2);
    pay_all(&mut c, T0 + 3);
    close(&mut c, T0 + 4);
    set_caller(agent(), T0 + 5);
    c.execute_payouts(CIRCLE.to_string(), 0, None);
    c.apply_payout_result(CIRCLE, 0, 0, true);
    c.apply_payout_result(CIRCLE, 0, 1, false);
    let round = c.get_round(CIRCLE.to_string(), 0).unwrap();
    assert_eq!(round.payouts[0].status, PayoutStatus::Confirmed);
    assert_eq!(round.payouts[1].status, PayoutStatus::Failed);
    let failed_recipient = round.payouts[1].recipient.clone();
    set_caller(failed_recipient, T0 + 6);
    c.claim_payout(CIRCLE.to_string(), 0, 1);
    assert_eq!(
        c.get_round(CIRCLE.to_string(), 0).unwrap().payouts[1].status,
        PayoutStatus::Sent
    );
}

#[test]
#[should_panic(expected = "Only the recipient or the agent")]
fn claim_payout_rejects_strangers() {
    let mut c = active_circle();
    pay_all(&mut c, T0 + 1);
    close(&mut c, T0 + 2);
    set_caller(agent(), T0 + 3);
    c.execute_payouts(CIRCLE.to_string(), 0, None);
    c.apply_payout_result(CIRCLE, 0, 0, false);
    set_caller(accounts(4), T0 + 4);
    c.claim_payout(CIRCLE.to_string(), 0, 0);
}

#[test]
#[should_panic(expected = "Payout is not in Failed state")]
fn claim_payout_requires_failed_status() {
    let mut c = active_circle();
    pay_all(&mut c, T0 + 1);
    close(&mut c, T0 + 2);
    set_caller(agent(), T0 + 3);
    c.claim_payout(CIRCLE.to_string(), 0, 0);
}

#[test]
fn circle_completes_after_every_member_collected() {
    let mut c = active_circle();
    let mut now = T0;
    for expected_round in 0..4u32 {
        now += 10;
        pay_all(&mut c, now);
        now += 10;
        close(&mut c, now);
        let round = c.get_round(CIRCLE.to_string(), expected_round).unwrap();
        assert_eq!(
            round.winner,
            Some(accounts(expected_round as usize)),
            "rotation order"
        );
    }
    let circle = c.get_circle(CIRCLE.to_string()).unwrap();
    assert_eq!(circle.status, CircleStatus::Completed);
    assert!(circle.all_collected());
    assert!(
        c.get_round(CIRCLE.to_string(), 4).is_none(),
        "no round after completion"
    );
    for m in members() {
        let rep = c.get_reputation(m);
        assert_eq!(rep.circles_completed, 1);
        assert_eq!(rep.on_time, 4);
        assert_eq!(rep.score, 4 * 21);
    }
    assert_eq!(events_named("circle_completed").len(), 1);
}

#[test]
#[should_panic(expected = "Circle is not active")]
fn close_round_on_completed_circle_panics() {
    let mut c = active_circle();
    let mut now = T0;
    for _ in 0..4 {
        now += 10;
        pay_all(&mut c, now);
        now += 10;
        close(&mut c, now);
    }
    close(&mut c, now + 10);
}

#[test]
fn round_status_projects_approved_bid_and_dividend() {
    let mut c = active_circle();
    bid(&mut c, accounts(3), 3 * TOKEN_UNIT, T0 + 1);
    approve(&mut c, accounts(3), 3 * TOKEN_UNIT, T0 + 2);
    pay_all(&mut c, T0 + 3);
    set_caller(accounts(5), T0 + 4);
    let status = c.get_round_status(CIRCLE.to_string());
    assert!(status.can_close);
    assert_eq!(status.projected_winner, Some(accounts(3)));
    assert_eq!(status.projected_discount.0, 3 * TOKEN_UNIT);
    assert_eq!(status.projected_dividend_each.0, TOKEN_UNIT);
    assert_eq!(status.bids.len(), 1);
}

#[test]
fn events_follow_nep297_format() {
    let mut c = active_circle();
    pay_all(&mut c, T0 + 1);
    close(&mut c, T0 + 2);
    let logs = events_named("round_closed");
    assert_eq!(logs.len(), 1);
    let json = logs[0].trim_start_matches("EVENT_JSON:");
    let value: near_sdk::serde_json::Value = near_sdk::serde_json::from_str(json).unwrap();
    assert_eq!(value["standard"], "pasanaku");
    assert_eq!(value["version"], "1.0.0");
    assert_eq!(value["data"][0]["circle_id"], CIRCLE);
    assert_eq!(value["data"][0]["winner"], accounts(0).to_string());
}

use near_sdk::test_utils::accounts;

use super::common::*;
use crate::model::{Reputation, TOKEN_UNIT};
use crate::reputation::*;

#[test]
fn ln_milli_matches_f64_within_tolerance() {
    for x in [2u128, 3, 5, 10, 11, 50, 101, 500, 1_001, 10_000, 1_000_000] {
        let exact = (x as f64).ln() * 1000.0;
        let approx = ln_milli(x) as f64;
        let rel = ((approx - exact) / exact).abs();
        assert!(
            rel < 0.015,
            "x={x}: approx {approx} vs exact {exact} (rel {rel})"
        );
    }
    assert_eq!(ln_milli(0), 0);
    assert_eq!(ln_milli(1), 0);
    assert_eq!(ln_milli(2), 693);
}

#[test]
fn on_time_delta_for_typical_contributions() {
    assert_eq!(on_time_delta(10 * TOKEN_UNIT), 21);
    assert_eq!(on_time_delta(100 * TOKEN_UNIT), 33);
    assert_eq!(on_time_delta(500 * TOKEN_UNIT), 41);
    assert_eq!(on_time_delta(0), ALPHA);
}

#[test]
fn tier_thresholds_and_caps() {
    assert_eq!(Tier::for_score(0), Tier::Semilla);
    assert_eq!(Tier::for_score(99), Tier::Semilla);
    assert_eq!(Tier::for_score(100), Tier::PyME);
    assert_eq!(Tier::for_score(299), Tier::PyME);
    assert_eq!(Tier::for_score(300), Tier::Mayorista);
    assert_eq!(tier_cap_for_score(0), 100 * TOKEN_UNIT);
    assert_eq!(tier_cap_for_score(150), 500 * TOKEN_UNIT);
    assert_eq!(tier_cap_for_score(1_000), 2_500 * TOKEN_UNIT);
}

#[test]
fn reputation_transitions_saturate() {
    let mut r = Reputation::default();
    r.apply_default(T0);
    assert_eq!(r.score, 0, "score never goes negative");
    assert_eq!(r.defaults, 1);
    r.apply_on_time(10 * TOKEN_UNIT, T0 + 1);
    assert_eq!(r.score, 21);
    assert_eq!(r.on_time, 1);
    r.apply_removed(T0 + 2);
    assert_eq!(r.score, 0);
    r.apply_completed(T0 + 3);
    assert_eq!(r.circles_completed, 1);
    assert_eq!(r.updated_at.0, T0 + 3);
}

#[test]
fn member_tier_view_for_unknown_account_is_semilla() {
    let c = new_contract();
    let view = c.get_member_tier(accounts(4));
    assert_eq!(view.score, 0);
    assert_eq!(view.tier, Tier::Semilla);
    assert_eq!(view.cap.0, 100 * TOKEN_UNIT);
    assert_eq!(view.next_tier_at, Some(TIER_PYME_MIN_SCORE));
    assert_eq!(c.get_reputation(accounts(4)), Reputation::default());
}

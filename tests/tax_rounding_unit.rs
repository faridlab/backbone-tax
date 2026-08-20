//! Rounding unit oracle — pure math, no database.
//!
//! Pins `distribute_delta_smoothly` (the smooth-delta rule behind the
//! `round_globally` policy) and `round2`. The load-bearing contract: the
//! outputs ALWAYS sum to the delta exactly, and no single factor ever absorbs
//! more than one cent beyond its exact proportional share — a per-line
//! "last line absorbs the residual" rule would misstate one line of every
//! multi-line document.

use backbone_tax::{distribute_delta_smoothly, round2};
use rust_decimal::Decimal;
use std::str::FromStr;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

#[test]
fn sd_two_equal_weights_positive_delta() {
    // Two equal weights, delta +0.01: floors give 0.00 each; the tie breaks by
    // original order so the FIRST factor takes the cent.
    let out = distribute_delta_smoothly(&[d("1.00"), d("1.00")], d("0.01"));
    assert_eq!(out, vec![d("0.01"), d("0.00")]);
}

#[test]
fn sd_two_equal_weights_negative_delta() {
    // Negative delta: floor(−0.005) is −0.01 (away), so the residual comes
    // back +0.01 to the first factor — net [0.00, −0.01]. Exact total holds.
    let out = distribute_delta_smoothly(&[d("1.00"), d("1.00")], d("-0.01"));
    assert_eq!(out, vec![d("0.00"), d("-0.01")]);
}

#[test]
fn sd_unequal_60_40_delta_003() {
    // Exact shares 0.018 / 0.012 → floors 0.01 / 0.01 → the heavier factor
    // takes the residual cent: [0.02, 0.01].
    let out = distribute_delta_smoothly(&[Decimal::from(60), Decimal::from(40)], d("0.03"));
    assert_eq!(out, vec![d("0.02"), d("0.01")]);
}

#[test]
fn sd_three_equal_delta_002() {
    // Exact shares 0.00667 → floors 0.00 each; two cents go to the first two.
    let out = distribute_delta_smoothly(
        &[Decimal::from(10), Decimal::from(10), Decimal::from(10)],
        d("0.02"),
    );
    assert_eq!(out, vec![d("0.01"), d("0.01"), d("0.00")]);
}

#[test]
fn sd_single_factor_absorbs_all() {
    let out = distribute_delta_smoothly(&[Decimal::from(7)], d("0.05"));
    assert_eq!(out, vec![d("0.05")]);
}

#[test]
fn sd_zero_weight_gets_nothing() {
    // A zero-weight factor never receives a cent: heavier factors always
    // precede it in the handout order.
    let out = distribute_delta_smoothly(
        &[Decimal::ZERO, Decimal::from(60), Decimal::from(40)],
        d("0.03"),
    );
    assert_eq!(out[0], Decimal::ZERO);
    assert_eq!(out.iter().copied().sum::<Decimal>(), d("0.03"));
}

#[test]
fn sd_sum_is_exact() {
    // 200 pseudo-random weight/delta pairs: the exact-total contract always
    // holds, positive or negative delta, any arity.
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..200 {
        let n = (next() % 5 + 1) as usize;
        let weights: Vec<Decimal> = (0..n)
            .map(|_| Decimal::from((next() % 1000) as i64))
            .collect();
        let delta = Decimal::from_i128_with_scale((next() % 200) as i128 - 100, 2);
        let out = distribute_delta_smoothly(&weights, delta);
        assert_eq!(out.len(), n);
        assert_eq!(
            out.iter().copied().sum::<Decimal>(),
            delta,
            "weights={weights:?} delta={delta}"
        );
    }
}

#[test]
fn sd_empty_zero_total() {
    let out = distribute_delta_smoothly(&[], d("0.07"));
    assert!(out.is_empty());
}

#[test]
fn round2_half_away_from_zero() {
    assert_eq!(round2(d("12.345")), d("12.35"));
    assert_eq!(round2(d("-12.345")), d("-12.35"));
    assert_eq!(round2(d("12.344")), d("12.34"));
    assert_eq!(round2(d("12.34")), d("12.34")); // already exact: unchanged
}

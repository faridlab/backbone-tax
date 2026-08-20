//! Document-grade rounding primitives — hand-authored (user-owned).
//!
//! A per-line-only port mis-totals every multi-line document: rounding each
//! line's tax independently accumulates a per-line bias, and the document total
//! stops matching the sum of its lines (which is what the journal balances
//! against). These primitives implement the two document policies:
//!
//! - `RoundPerLine`: each line is rounded to cents independently (the simple
//!   legacy behavior — kept because some jurisdictions prescribe it).
//! - `RoundGlobally`: raw amounts are summed unrounded, the SUM is rounded
//!   once, and the difference between that and the sum of per-line roundings
//!   is redistributed across the lines in cents.
//!
//! The redistribution is `distribute_delta_smoothly` (the smooth-delta rule):
//! the total is conserved EXACTLY — the outputs always sum to the delta — and
//! each factor receives at most one cent more/less than its exact share, so no
//! single line absorbs the whole rounding residual.

use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy;

/// The per-company rounding policy. Mirrors the `tax_rounding_method` DB enum
/// (`tax.company_tax_settings.rounding_method`); keep the two in sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundingMethod {
    /// Sum raw amounts across the document, round once, redistribute in cents.
    RoundGlobally,
    /// Round every line to cents independently.
    RoundPerLine,
}

impl RoundingMethod {
    /// Parse the DB enum label (`round_globally` / `round_per_line`).
    pub fn from_db(label: &str) -> Option<Self> {
        match label {
            "round_globally" => Some(RoundingMethod::RoundGlobally),
            "round_per_line" => Some(RoundingMethod::RoundPerLine),
            _ => None,
        }
    }

    /// The DB enum label for this policy.
    pub fn as_db(self) -> &'static str {
        match self {
            RoundingMethod::RoundGlobally => "round_globally",
            RoundingMethod::RoundPerLine => "round_per_line",
        }
    }
}

/// Money is exact: 2 decimals, round-half-away-from-zero (MidpointAwayFromZero).
pub fn round2(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

/// Split `delta` (possibly negative) across `weights` so that the outputs sum
/// to `delta` EXACTLY and each output deviates from its exact proportional
/// share by less than one cent. Sub-cent deltas cannot be split at cent
/// granularity and are floored to the nearest cent first (callers produce 2dp
/// deltas, so this never fires in practice).
///
/// Smooth-delta rule: every factor first takes `floor(exact share)` at cent
/// granularity (floor = toward −∞, so negative shares round away); the
/// residual — the sum of the discarded fractions — is then handed out one cent
/// at a time (added when positive, reclaimed when negative) to the factors in
/// order of weight DESC, ties broken by original position ASC. A zero-weight
/// factor never receives a cent: its exact share is zero, and heavier factors
/// always precede it in the handout order. If every weight is zero (degenerate
/// input), the first factor absorbs the whole delta so the exact-total contract
/// still holds.
///
/// Returns one output per weight, aligned by index.
pub fn distribute_delta_smoothly(weights: &[Decimal], delta: Decimal) -> Vec<Decimal> {
    let n = weights.len();
    if n == 0 {
        return Vec::new();
    }
    let hundred = Decimal::from(100);
    let one_cent = Decimal::new(1, 2); // 0.01
    let delta = (delta * hundred).floor() / hundred;

    let weight_sum: Decimal = weights.iter().copied().sum();
    if weight_sum == Decimal::ZERO {
        // Degenerate: no proportional split exists. Keep the exact-total
        // contract by parking everything on the first factor.
        let mut outs = vec![Decimal::ZERO; n];
        outs[0] = delta;
        return outs;
    }

    // Cent-granular floors of the exact proportional shares.
    let mut outs: Vec<Decimal> = weights
        .iter()
        .map(|w| ((*w * delta / weight_sum) * hundred).floor() / hundred)
        .collect();

    // Residual = delta − Σ floors, always a whole number of cents. Hand it out
    // one cent at a time, cycling the order if the residual somehow exceeds
    // one round (it cannot mathematically, but cycling keeps the contract
    // unconditionally true).
    let mut residual = delta - outs.iter().copied().sum::<Decimal>();
    if residual == Decimal::ZERO {
        return outs;
    }
    let step = if residual.is_sign_negative() {
        -one_cent
    } else {
        one_cent
    };
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| weights[b].cmp(&weights[a]).then_with(|| a.cmp(&b)));
    let mut next = 0usize;
    while residual != Decimal::ZERO {
        let i = order[next % n];
        outs[i] += step;
        residual -= step;
        next += 1;
    }
    outs
}

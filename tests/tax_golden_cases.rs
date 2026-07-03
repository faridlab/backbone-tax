//! Golden numeric oracle for the tax engine (region-neutral; sample rates, NOT seeded Indonesia
//! regulation). Proves exclusive/inclusive VAT, effective-dating, cumulative rows, and withholding
//! thresholds against real Postgres. Requires DATABASE_URL (defaults to :5433).

use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::str::FromStr;

use backbone_tax::{
    NewTemplate, NewTemplateRow, NewWithholding, TaxEngine, TaxError, TaxWriteService,
};
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_tax".to_string());
    PgPool::connect(&url).await.unwrap()
}
fn uq(p: &str) -> String {
    format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8])
}
fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}
fn day(y: i32, m: u32, dd: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, dd).unwrap()
}

// TGC-1: exclusive VAT — PPN 11% on 1,000,000 → 110,000 (one line).
#[tokio::test]
async fn exclusive_vat() {
    let pool = pool().await;
    let w = TaxWriteService::new(pool.clone());
    let engine = TaxEngine::new(pool.clone());
    let tid = w.create_template(NewTemplate {
        code: uq("PPN-EXCL"), name: "PPN 11%".into(), template_type: Some("sales".into()),
        tax_category_id: None, is_inclusive: false,
    }).await.unwrap();
    w.add_row(NewTemplateRow {
        template_id: tid, charge_type: None, rate: d("11"), account_id: None, is_withholding: false,
        effective_from: day(2022, 4, 1), effective_to: None, sort_order: 0,
        description: Some("PPN Keluaran 11%".into()),
    }).await.unwrap();

    let lines = engine.calculate(tid, d("1000000"), day(2026, 7, 3)).await.unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].tax_amount, d("110000.00"));
    assert_eq!(lines[0].rate, d("11"));
}

// TGC-2: inclusive VAT — gross 1,110,000 at 11% → tax extracted 110,000 (base 1,000,000).
#[tokio::test]
async fn inclusive_vat() {
    let pool = pool().await;
    let w = TaxWriteService::new(pool.clone());
    let engine = TaxEngine::new(pool.clone());
    let tid = w.create_template(NewTemplate {
        code: uq("PPN-INCL"), name: "PPN 11% incl".into(), template_type: Some("sales".into()),
        tax_category_id: None, is_inclusive: true,
    }).await.unwrap();
    w.add_row(NewTemplateRow {
        template_id: tid, charge_type: None, rate: d("11"), account_id: None, is_withholding: false,
        effective_from: day(2022, 4, 1), effective_to: None, sort_order: 0, description: None,
    }).await.unwrap();

    let lines = engine.calculate(tid, d("1110000"), day(2026, 7, 3)).await.unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].tax_amount, d("110000.00"), "extracted tax from inclusive gross");
}

// TGC-3: effective-dating — 11% before 2025-01-01, 12% on/after. Same template, date picks the row.
#[tokio::test]
async fn effective_dated_rate() {
    let pool = pool().await;
    let w = TaxWriteService::new(pool.clone());
    let engine = TaxEngine::new(pool.clone());
    let tid = w.create_template(NewTemplate {
        code: uq("PPN-EFF"), name: "PPN eff".into(), template_type: Some("sales".into()),
        tax_category_id: None, is_inclusive: false,
    }).await.unwrap();
    w.add_row(NewTemplateRow {
        template_id: tid, charge_type: None, rate: d("11"), account_id: None, is_withholding: false,
        effective_from: day(2022, 4, 1), effective_to: Some(day(2024, 12, 31)), sort_order: 0, description: None,
    }).await.unwrap();
    w.add_row(NewTemplateRow {
        template_id: tid, charge_type: None, rate: d("12"), account_id: None, is_withholding: false,
        effective_from: day(2025, 1, 1), effective_to: None, sort_order: 0, description: None,
    }).await.unwrap();

    let old = engine.calculate(tid, d("1000000"), day(2024, 6, 1)).await.unwrap();
    assert_eq!(old[0].tax_amount, d("110000.00"), "11% before 2025");
    let new = engine.calculate(tid, d("1000000"), day(2025, 6, 1)).await.unwrap();
    assert_eq!(new[0].tax_amount, d("120000.00"), "12% from 2025");
}

// TGC-4: cumulative row — luxury surcharge 10% on (net + PPN 11%).
#[tokio::test]
async fn cumulative_row() {
    let pool = pool().await;
    let w = TaxWriteService::new(pool.clone());
    let engine = TaxEngine::new(pool.clone());
    let tid = w.create_template(NewTemplate {
        code: uq("PPN-CUM"), name: "PPN + surcharge".into(), template_type: Some("sales".into()),
        tax_category_id: None, is_inclusive: false,
    }).await.unwrap();
    w.add_row(NewTemplateRow {
        template_id: tid, charge_type: Some("on_net_total".into()), rate: d("11"), account_id: None,
        is_withholding: false, effective_from: day(2022,4,1), effective_to: None, sort_order: 0, description: None,
    }).await.unwrap();
    w.add_row(NewTemplateRow {
        template_id: tid, charge_type: Some("on_previous_row_total".into()), rate: d("10"), account_id: None,
        is_withholding: false, effective_from: day(2022,4,1), effective_to: None, sort_order: 1, description: None,
    }).await.unwrap();

    let lines = engine.calculate(tid, d("1000000"), day(2026, 7, 3)).await.unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].tax_amount, d("110000.00")); // PPN 11% of 1,000,000
    assert_eq!(lines[1].tax_amount, d("111000.00")); // 10% of (1,000,000 + 110,000)
}

// TGC-5: withholding threshold — PPh 2%: above threshold → -amount; below → None.
#[tokio::test]
async fn withholding_threshold() {
    let pool = pool().await;
    let w = TaxWriteService::new(pool.clone());
    let engine = TaxEngine::new(pool.clone());
    let cid = w.create_withholding(NewWithholding {
        code: uq("PPH23"), name: "PPh 23 services 2%".into(), rate: d("2"),
        threshold_amount: d("1000000"), account_id: None, effective_from: day(2022,1,1), effective_to: None,
    }).await.unwrap();

    let above = engine.resolve_withholding(cid, d("5000000"), day(2026, 7, 3)).await.unwrap();
    let l = above.expect("above threshold yields a line");
    assert_eq!(l.tax_amount, d("-100000.00"), "2% of 5,000,000, negative (deduction)");
    assert!(l.is_withholding);

    let below = engine.resolve_withholding(cid, d("500000"), day(2026, 7, 3)).await.unwrap();
    assert!(below.is_none(), "below threshold yields no line");
}

// TGC-6: engine errors — unknown template, no effective rate, negative base.
#[tokio::test]
async fn engine_errors() {
    let pool = pool().await;
    let w = TaxWriteService::new(pool.clone());
    let engine = TaxEngine::new(pool.clone());
    assert!(matches!(
        engine.calculate(Uuid::new_v4(), d("100"), day(2026,7,3)).await.unwrap_err(),
        TaxError::TemplateNotFound(_)
    ));
    let tid = w.create_template(NewTemplate {
        code: uq("EMPTY"), name: "empty".into(), template_type: None, tax_category_id: None, is_inclusive: false,
    }).await.unwrap();
    // row effective only in 2030 → no effective rate for 2026
    w.add_row(NewTemplateRow {
        template_id: tid, charge_type: None, rate: d("11"), account_id: None, is_withholding: false,
        effective_from: day(2030,1,1), effective_to: None, sort_order: 0, description: None,
    }).await.unwrap();
    assert!(matches!(
        engine.calculate(tid, d("100"), day(2026,7,3)).await.unwrap_err(),
        TaxError::NoEffectiveRate(_)
    ));
    assert!(matches!(
        engine.calculate(tid, d("-1"), day(2030,1,1)).await.unwrap_err(),
        TaxError::NegativeBase
    ));
}

// ── Council 2026-07-03 fixes: overlap prevention + exact inclusive reconciliation ──

// TGC-7: overlapping effective windows at the same sort_order are REJECTED at write time
// (the add-the-new-rate-without-closing-the-old mistake) — and the DB EXCLUDE forbids them too.
#[tokio::test]
async fn overlapping_rows_rejected() {
    let pool = pool().await;
    let w = TaxWriteService::new(pool.clone());
    let tid = w.create_template(NewTemplate {
        code: uq("OVL"), name: "o".into(), template_type: Some("sales".into()),
        tax_category_id: None, is_inclusive: false,
    }).await.unwrap();
    // old 11%, open-ended
    w.add_row(NewTemplateRow {
        template_id: tid, charge_type: None, rate: d("11"), account_id: None, is_withholding: false,
        effective_from: day(2022,4,1), effective_to: None, sort_order: 0, description: None,
    }).await.unwrap();
    // new 12% from 2025 WITHOUT closing the old row → overlaps → must be rejected
    let err = w.add_row(NewTemplateRow {
        template_id: tid, charge_type: None, rate: d("12"), account_id: None, is_withholding: false,
        effective_from: day(2025,1,1), effective_to: None, sort_order: 0, description: None,
    }).await.unwrap_err();
    assert!(matches!(err, TaxError::OverlappingWindow(_)), "got {err:?}");

    // and calculate returns exactly ONE line (no double-charge) — 11% only.
    let lines = engine(&pool).calculate(tid, d("1000000"), day(2025,6,1)).await.unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].tax_amount, d("110000.00"));
}

// TGC-8: inclusive extraction reconciles EXACTLY — Σ lines == gross, even on an odd gross.
#[tokio::test]
async fn inclusive_reconciles_exactly() {
    let pool = pool().await;
    let w = TaxWriteService::new(pool.clone());
    let e = TaxEngine::new(pool.clone());
    let tid = w.create_template(NewTemplate {
        code: uq("INC-ODD"), name: "i".into(), template_type: Some("sales".into()),
        tax_category_id: None, is_inclusive: true,
    }).await.unwrap();
    w.add_row(NewTemplateRow {
        template_id: tid, charge_type: None, rate: d("11"), account_id: None, is_withholding: false,
        effective_from: day(2022,4,1), effective_to: None, sort_order: 0, description: None,
    }).await.unwrap();
    for gross in ["1111111", "1000000", "999999", "1234567"] {
        let g = d(gross);
        let lines = e.calculate(tid, g, day(2026,7,3)).await.unwrap();
        let tax: rust_decimal::Decimal = lines.iter().map(|l| l.tax_amount).sum();
        let net = (g / (rust_decimal::Decimal::ONE + d("0.11")))
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
        assert_eq!(net + tax, g, "gross {gross}: net {net} + tax {tax} must equal gross");
    }
}

// TGC-9: an inclusive template with a cumulative row is rejected (undefined grossing-up basis).
#[tokio::test]
async fn inclusive_with_cumulative_rejected() {
    let pool = pool().await;
    let w = TaxWriteService::new(pool.clone());
    let e = TaxEngine::new(pool.clone());
    let tid = w.create_template(NewTemplate {
        code: uq("INC-CUM"), name: "ic".into(), template_type: Some("sales".into()),
        tax_category_id: None, is_inclusive: true,
    }).await.unwrap();
    w.add_row(NewTemplateRow {
        template_id: tid, charge_type: Some("on_net_total".into()), rate: d("11"), account_id: None,
        is_withholding: false, effective_from: day(2022,4,1), effective_to: None, sort_order: 0, description: None,
    }).await.unwrap();
    w.add_row(NewTemplateRow {
        template_id: tid, charge_type: Some("on_previous_row_total".into()), rate: d("10"), account_id: None,
        is_withholding: false, effective_from: day(2022,4,1), effective_to: None, sort_order: 1, description: None,
    }).await.unwrap();
    assert!(matches!(
        e.calculate(tid, d("1000000"), day(2026,7,3)).await.unwrap_err(),
        TaxError::InclusiveUnsupported
    ));
}

fn engine(pool: &PgPool) -> TaxEngine { TaxEngine::new(pool.clone()) }

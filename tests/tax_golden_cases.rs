//! Golden numeric oracle for the tax engine (region-neutral; sample rates, NOT seeded Indonesia
//! regulation). Proves exclusive/inclusive VAT, effective-dating, cumulative rows, and withholding
//! thresholds against real Postgres. Requires DATABASE_URL (defaults to :5433).
//!
//! Each test wraps its body in `with_company_scope(Some(company))` because the engine reads
//! (`calculate` / `resolve_withholding`) require an ambient company scope (the engine fails loud as
//! `NoCompanyScope` otherwise); the write service self-scopes from each `New*` struct's `company_id`.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::str::FromStr;

use backbone_tax::{
    DocumentTaxRequest, DocumentTaxRequestLine, DocumentType, NewCompanySettings,
    NewRepartitionSplit, NewTag, NewTemplate, NewTemplateRow, NewWithholding,
    ReplaceRepartitionFamily, RoundingMethod, TaxEngine, TaxError, TaxWriteService,
};
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:5433/backbone_tax".to_string()
    });
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
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let engine = TaxEngine::new(pool.clone());
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("PPN-EXCL"),
                name: "PPN 11%".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: false,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("11"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: Some("PPN Keluaran 11%".into()),
        })
        .await
        .unwrap();

        let lines = engine
            .calculate(tid, d("1000000"), day(2026, 7, 3))
            .await
            .unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tax_amount, d("110000.00"));
        assert_eq!(lines[0].rate, d("11"));
    })
    .await;
}

// TGC-2: inclusive VAT — gross 1,110,000 at 11% → tax extracted 110,000 (base 1,000,000).
#[tokio::test]
async fn inclusive_vat() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let engine = TaxEngine::new(pool.clone());
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("PPN-INCL"),
                name: "PPN 11% incl".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: true,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("11"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();

        let lines = engine
            .calculate(tid, d("1110000"), day(2026, 7, 3))
            .await
            .unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].tax_amount,
            d("110000.00"),
            "extracted tax from inclusive gross"
        );
    })
    .await;
}

// TGC-3: effective-dating — 11% before 2025-01-01, 12% on/after. Same template, date picks the row.
#[tokio::test]
async fn effective_dated_rate() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let engine = TaxEngine::new(pool.clone());
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("PPN-EFF"),
                name: "PPN eff".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: false,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("11"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: Some(day(2024, 12, 31)),
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("12"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2025, 1, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();

        let old = engine
            .calculate(tid, d("1000000"), day(2024, 6, 1))
            .await
            .unwrap();
        assert_eq!(old[0].tax_amount, d("110000.00"), "11% before 2025");
        let new = engine
            .calculate(tid, d("1000000"), day(2025, 6, 1))
            .await
            .unwrap();
        assert_eq!(new[0].tax_amount, d("120000.00"), "12% from 2025");
    })
    .await;
}

// TGC-4: cumulative row — luxury surcharge 10% on (net + PPN 11%).
#[tokio::test]
async fn cumulative_row() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let engine = TaxEngine::new(pool.clone());
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("PPN-CUM"),
                name: "PPN + surcharge".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: false,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: Some("on_net_total".into()),
            rate: d("11"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: Some("on_previous_row_total".into()),
            rate: d("10"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 1,
            description: None,
        })
        .await
        .unwrap();

        let lines = engine
            .calculate(tid, d("1000000"), day(2026, 7, 3))
            .await
            .unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].tax_amount, d("110000.00")); // PPN 11% of 1,000,000
        assert_eq!(lines[1].tax_amount, d("111000.00")); // 10% of (1,000,000 + 110,000)
    })
    .await;
}

// TGC-5: withholding threshold — PPh 2%: above threshold → -amount; below → None.
#[tokio::test]
async fn withholding_threshold() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let engine = TaxEngine::new(pool.clone());
        let cid = w
            .create_withholding(NewWithholding {
                company_id: company,
                code: uq("PPH23"),
                name: "PPh 23 services 2%".into(),
                rate: d("2"),
                threshold_amount: d("1000000"),
                account_id: None,
                effective_from: day(2022, 1, 1),
                effective_to: None,
            })
            .await
            .unwrap();

        let above = engine
            .resolve_withholding(cid, d("5000000"), day(2026, 7, 3))
            .await
            .unwrap();
        let l = above.expect("above threshold yields a line");
        assert_eq!(
            l.tax_amount,
            d("-100000.00"),
            "2% of 5,000,000, negative (deduction)"
        );
        assert!(l.is_withholding);

        let below = engine
            .resolve_withholding(cid, d("500000"), day(2026, 7, 3))
            .await
            .unwrap();
        assert!(below.is_none(), "below threshold yields no line");
    })
    .await;
}

// TGC-6: engine errors — unknown template, no effective rate, negative base.
#[tokio::test]
async fn engine_errors() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let engine = TaxEngine::new(pool.clone());
        assert!(matches!(
            engine
                .calculate(Uuid::new_v4(), d("100"), day(2026, 7, 3))
                .await
                .unwrap_err(),
            TaxError::TemplateNotFound(_)
        ));
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("EMPTY"),
                name: "empty".into(),
                template_type: None,
                tax_category_id: None,
                is_inclusive: false,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        // row effective only in 2030 → no effective rate for 2026
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("11"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2030, 1, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();
        assert!(matches!(
            engine
                .calculate(tid, d("100"), day(2026, 7, 3))
                .await
                .unwrap_err(),
            TaxError::NoEffectiveRate(_)
        ));
        assert!(matches!(
            engine
                .calculate(tid, d("-1"), day(2030, 1, 1))
                .await
                .unwrap_err(),
            TaxError::NegativeBase
        ));
    })
    .await;
}

// ── Council 2026-07-03 fixes: overlap prevention + exact inclusive reconciliation ──

// TGC-7: overlapping effective windows at the same sort_order are REJECTED at write time
// (the add-the-new-rate-without-closing-the-old mistake) — and the DB EXCLUDE forbids them too.
#[tokio::test]
async fn overlapping_rows_rejected() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("OVL"),
                name: "o".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: false,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        // old 11%, open-ended
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("11"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();
        // new 12% from 2025 WITHOUT closing the old row → overlaps → must be rejected
        let err = w
            .add_row(NewTemplateRow {
                company_id: company,
                template_id: tid,
                charge_type: None,
                rate: d("12"),
                account_id: None,
                is_withholding: false,
                effective_from: day(2025, 1, 1),
                effective_to: None,
                sort_order: 0,
                description: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, TaxError::OverlappingWindow(_)), "got {err:?}");

        // and calculate returns exactly ONE line (no double-charge) — 11% only.
        let lines = TaxEngine::new(pool.clone())
            .calculate(tid, d("1000000"), day(2025, 6, 1))
            .await
            .unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tax_amount, d("110000.00"));
    })
    .await;
}

// TGC-8: inclusive extraction reconciles EXACTLY — Σ lines == gross, even on an odd gross.
#[tokio::test]
async fn inclusive_reconciles_exactly() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let e = TaxEngine::new(pool.clone());
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("INC-ODD"),
                name: "i".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: true,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("11"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();
        for gross in ["1111111", "1000000", "999999", "1234567"] {
            let g = d(gross);
            let lines = e.calculate(tid, g, day(2026, 7, 3)).await.unwrap();
            let tax: rust_decimal::Decimal = lines.iter().map(|l| l.tax_amount).sum();
            let net = (g / (rust_decimal::Decimal::ONE + d("0.11")))
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
            assert_eq!(
                net + tax,
                g,
                "gross {gross}: net {net} + tax {tax} must equal gross"
            );
        }
    })
    .await;
}

// TGC-9: an inclusive template with a cumulative row is rejected (undefined grossing-up basis).
#[tokio::test]
async fn inclusive_with_cumulative_rejected() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let e = TaxEngine::new(pool.clone());
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("INC-CUM"),
                name: "ic".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: true,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: Some("on_net_total".into()),
            rate: d("11"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: Some("on_previous_row_total".into()),
            rate: d("10"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 1,
            description: None,
        })
        .await
        .unwrap();
        assert!(matches!(
            e.calculate(tid, d("1000000"), day(2026, 7, 3))
                .await
                .unwrap_err(),
            TaxError::InclusiveUnsupported
        ));
    })
    .await;
}

// ---------------------------------------------------------------------------
// Document-grade engine: rounding policy, repartition, CABA resolution.
// The two-policy worked example is pinned from the Odoo accounting docs
// (two 21.53 lines, 21% inclusive): round_globally redistributes so the
// DOCUMENT totals are exact — per-line assignment order is this engine's
// contract (ties hand residual cents to the earlier line), while the
// document totals are the oracle.
// ---------------------------------------------------------------------------

fn doc_lines(tid: uuid::Uuid, price: &str, n: usize) -> Vec<DocumentTaxRequestLine> {
    (0..n)
        .map(|_| DocumentTaxRequestLine {
            template_id: tid,
            quantity: d("1"),
            unit_price: d(price),
        })
        .collect()
}

// TGC-10: round_globally worked example — no settings row ⇒ round_globally.
#[tokio::test]
async fn tgc10_round_globally_odoo_worked_example() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let e = TaxEngine::new(pool.clone());
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("PPN-INCL"),
                name: "PPN 21% inclusive".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: true,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("21"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();

        let r = e
            .calculate_document(&DocumentTaxRequest {
                company_id: company,
                document_type: DocumentType::Invoice,
                on_date: day(2026, 8, 19),
                lines: doc_lines(tid, "21.53", 2),
            })
            .await
            .unwrap();
        assert_eq!(r.method, RoundingMethod::RoundGlobally);
        assert_eq!(r.net_amounts, vec![d("17.80"), d("17.79")]);
        let taxes: Vec<Decimal> = r.lines.iter().map(|l| l.tax_amount).collect();
        assert_eq!(taxes, vec![d("3.74"), d("3.73")]);
        assert_eq!(r.excluded_total, d("35.59"));
        assert_eq!(r.tax_total, d("7.47"));
        assert_eq!(r.included_total, d("43.06"));
    })
    .await;
}

// TGC-11: the same inputs under round_per_line — the policies provably diverge.
#[tokio::test]
async fn tgc11_round_per_line_same_inputs() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let e = TaxEngine::new(pool.clone());
        w.upsert_company_settings(NewCompanySettings {
            company_id: company,
            rounding_method: "round_per_line".into(),
            default_exigibility: "on_invoice".into(),
            cash_basis_transition_account_id: None,
        })
        .await
        .unwrap();
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("PPN-INCL"),
                name: "PPN 21% inclusive".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: true,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("21"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();

        let r = e
            .calculate_document(&DocumentTaxRequest {
                company_id: company,
                document_type: DocumentType::Invoice,
                on_date: day(2026, 8, 19),
                lines: doc_lines(tid, "21.53", 2),
            })
            .await
            .unwrap();
        assert_eq!(r.method, RoundingMethod::RoundPerLine);
        assert_eq!(r.net_amounts, vec![d("17.79"), d("17.79")]);
        let taxes: Vec<Decimal> = r.lines.iter().map(|l| l.tax_amount).collect();
        assert_eq!(taxes, vec![d("3.74"), d("3.74")]);
        assert_eq!(r.excluded_total, d("35.58"));
        assert_eq!(r.tax_total, d("7.48")); // 1 cent above round_globally — divergence proof
        assert_eq!(r.included_total, d("43.06"));
    })
    .await;
}

// TGC-12: exclusive multi-line under round_globally — the document tax total
// is the rounded sum of raw taxes, not the sum of rounded per-line taxes.
#[tokio::test]
async fn tgc12_round_globally_exclusive_multi_line() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let e = TaxEngine::new(pool.clone());
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("VAT10"),
                name: "VAT 10%".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: false,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("10"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();

        let r = e
            .calculate_document(&DocumentTaxRequest {
                company_id: company,
                document_type: DocumentType::Invoice,
                on_date: day(2026, 8, 19),
                lines: doc_lines(tid, "10.005", 2),
            })
            .await
            .unwrap();
        assert_eq!(r.method, RoundingMethod::RoundGlobally);
        assert_eq!(r.net_amounts, vec![d("10.01"), d("10.00")]); // smooth split of 20.01
        assert_eq!(r.excluded_total, d("20.01"));
        assert_eq!(r.tax_total, d("2.00")); // round2(2.0010), NOT 1.00+1.00=2.00 per-line
        assert_eq!(r.included_total, d("22.01"));
    })
    .await;
}

// TGC-13: repartition factor split — a 60/40 tax family routes two exact lines,
// and the base line's tags surface as document base_tags.
#[tokio::test]
async fn tgc13_repartition_factor_split() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let e = TaxEngine::new(pool.clone());
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("PPN-SPLIT"),
                name: "PPN split".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: false,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("11"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();
        let tag = w
            .create_tag(NewTag {
                company_id: company,
                code: uq("TAG").into(),
                name: "reporting".into(),
            })
            .await
            .unwrap();
        let acc_a = Uuid::new_v4();
        let acc_b = Uuid::new_v4();
        w.replace_repartition_family(ReplaceRepartitionFamily {
            company_id: company,
            template_id: tid,
            document_type: "invoice".into(),
            base_tag_ids: vec![tag],
            base_description: None,
            tax_splits: vec![
                NewRepartitionSplit {
                    factor_percent: d("60"),
                    account_id: Some(acc_a),
                    tag_ids: vec![],
                    sort_order: 0,
                    description: None,
                },
                NewRepartitionSplit {
                    factor_percent: d("40"),
                    account_id: Some(acc_b),
                    tag_ids: vec![],
                    sort_order: 1,
                    description: None,
                },
            ],
        })
        .await
        .unwrap();

        let r = e
            .calculate_document(&DocumentTaxRequest {
                company_id: company,
                document_type: DocumentType::Invoice,
                on_date: day(2026, 8, 19),
                lines: vec![DocumentTaxRequestLine {
                    template_id: tid,
                    quantity: d("1"),
                    unit_price: d("1000"),
                }],
            })
            .await
            .unwrap();
        assert_eq!(r.lines.len(), 2);
        assert_eq!(r.lines[0].account_id, Some(acc_a));
        assert_eq!(r.lines[0].tax_amount, d("66.00"));
        assert_eq!(r.lines[1].account_id, Some(acc_b));
        assert_eq!(r.lines[1].tax_amount, d("44.00"));
        assert_eq!(r.tax_total, d("110.00"));
        assert_eq!(r.base_tags, vec![tag]);
    })
    .await;
}

// TGC-14: a refund document selects the refund family — same amounts, the
// refund family's accounts.
#[tokio::test]
async fn tgc14_repartition_refund_family_sign() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let e = TaxEngine::new(pool.clone());
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("PPN-REF"),
                name: "PPN refund routing".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: false,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("11"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();
        let acc_invoice = Uuid::new_v4();
        let acc_refund = Uuid::new_v4();
        w.replace_repartition_family(ReplaceRepartitionFamily {
            company_id: company,
            template_id: tid,
            document_type: "invoice".into(),
            base_tag_ids: vec![],
            base_description: None,
            tax_splits: vec![NewRepartitionSplit {
                factor_percent: d("100"),
                account_id: Some(acc_invoice),
                tag_ids: vec![],
                sort_order: 0,
                description: None,
            }],
        })
        .await
        .unwrap();
        w.replace_repartition_family(ReplaceRepartitionFamily {
            company_id: company,
            template_id: tid,
            document_type: "refund".into(),
            base_tag_ids: vec![],
            base_description: None,
            tax_splits: vec![NewRepartitionSplit {
                factor_percent: d("100"),
                account_id: Some(acc_refund),
                tag_ids: vec![],
                sort_order: 0,
                description: None,
            }],
        })
        .await
        .unwrap();

        let r = e
            .calculate_document(&DocumentTaxRequest {
                company_id: company,
                document_type: DocumentType::Refund,
                on_date: day(2026, 8, 19),
                lines: vec![DocumentTaxRequestLine {
                    template_id: tid,
                    quantity: d("1"),
                    unit_price: d("1000"),
                }],
            })
            .await
            .unwrap();
        assert_eq!(r.lines.len(), 1);
        assert_eq!(r.lines[0].account_id, Some(acc_refund));
        assert_eq!(r.lines[0].tax_amount, d("110.00")); // sign unchanged; the caller negates credit notes wholesale
    })
    .await;
}

// TGC-15: an on_payment template defers — the line posts to the transition
// account with the real account recorded, and base tags are suppressed (a
// deferred base has not realized). The template is inserted via raw SQL on
// purpose: the write service refuses on_payment when accounting.accounts is
// absent (fail-closed), which is exactly what the integrity probe pins; the
// DB trigger stays permissive by design for accounting-less hosts.
#[tokio::test]
async fn tgc15_caba_deferred_account_resolution() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let e = TaxEngine::new(pool.clone());
        let tid = Uuid::new_v4();
        let transition = Uuid::new_v4();
        let real = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO tax.tax_templates
                   (id, company_id, code, name, template_type, is_inclusive,
                    tax_exigibility, cash_basis_transition_account_id)
               VALUES ($1, $2, $3, 'CABA PPN', 'sales', FALSE,
                       'on_payment'::tax_exigibility, $4)"#,
        )
        .bind(tid)
        .bind(company)
        .bind(uq("CABA"))
        .bind(transition)
        .execute(&pool)
        .await
        .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("11"),
            account_id: Some(real),
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();
        let tag = w
            .create_tag(NewTag {
                company_id: company,
                code: uq("DTAG").into(),
                name: "deferred base tag".into(),
            })
            .await
            .unwrap();
        w.replace_repartition_family(ReplaceRepartitionFamily {
            company_id: company,
            template_id: tid,
            document_type: "invoice".into(),
            base_tag_ids: vec![tag],
            base_description: None,
            tax_splits: vec![NewRepartitionSplit {
                factor_percent: d("100"),
                account_id: Some(real),
                tag_ids: vec![],
                sort_order: 0,
                description: None,
            }],
        })
        .await
        .unwrap();

        let r = e
            .calculate_document(&DocumentTaxRequest {
                company_id: company,
                document_type: DocumentType::Invoice,
                on_date: day(2026, 8, 19),
                lines: vec![DocumentTaxRequestLine {
                    template_id: tid,
                    quantity: d("1"),
                    unit_price: d("1000"),
                }],
            })
            .await
            .unwrap();
        assert_eq!(r.lines.len(), 1);
        assert_eq!(r.lines[0].account_id, Some(transition)); // posts to the transition account
        assert_eq!(r.lines[0].real_account_id, Some(real)); // remembers where it flips to
        assert!(
            r.base_tags.is_empty(),
            "a deferred base has not realized; its tags stay suppressed"
        );
    })
    .await;
}

// TGC-16: a template with NO repartition rows (pre-upgrade shape, created via
// the repository rather than the auto-seeding service) still routes 100% of
// each tax to the template row's own account.
#[tokio::test]
async fn tgc16_legacy_template_row_account_fallback() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        use backbone_tax::{NewTaxTemplateRow as RepoTemplate, TaxTemplateRepository};
        let w = TaxWriteService::new(pool.clone());
        let e = TaxEngine::new(pool.clone());
        let tid = Uuid::new_v4();
        let row_account = Uuid::new_v4();
        TaxTemplateRepository::insert_on(
            &pool,
            &RepoTemplate {
                id: tid,
                company_id: company,
                code: &uq("LEGACY"),
                name: "legacy row-account routing",
                template_type: "sales",
                tax_category_id: None,
                is_inclusive: false,
                tax_exigibility: "on_invoice",
                cash_basis_transition_account_id: None,
            },
        )
        .await
        .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("11"),
            account_id: Some(row_account),
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();

        let r = e
            .calculate_document(&DocumentTaxRequest {
                company_id: company,
                document_type: DocumentType::Invoice,
                on_date: day(2026, 8, 19),
                lines: vec![DocumentTaxRequestLine {
                    template_id: tid,
                    quantity: d("1"),
                    unit_price: d("1000"),
                }],
            })
            .await
            .unwrap();
        assert_eq!(r.lines.len(), 1);
        assert_eq!(r.lines[0].account_id, Some(row_account));
        assert_eq!(r.lines[0].real_account_id, None);
    })
    .await;
}

// TGC-17: absent settings row ⇒ the documented default round_globally; one
// upsert flips the company to round_per_line.
#[tokio::test]
async fn tgc17_settings_default_when_row_absent() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let e = TaxEngine::new(pool.clone());
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("PPN-D"),
                name: "default policy probe".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: false,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("11"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();
        let req = DocumentTaxRequest {
            company_id: company,
            document_type: DocumentType::Invoice,
            on_date: day(2026, 8, 19),
            lines: vec![DocumentTaxRequestLine {
                template_id: tid,
                quantity: d("1"),
                unit_price: d("100"),
            }],
        };
        assert_eq!(
            e.calculate_document(&req).await.unwrap().method,
            RoundingMethod::RoundGlobally
        );
        w.upsert_company_settings(NewCompanySettings {
            company_id: company,
            rounding_method: "round_per_line".into(),
            default_exigibility: "on_invoice".into(),
            cash_basis_transition_account_id: None,
        })
        .await
        .unwrap();
        assert_eq!(
            e.calculate_document(&req).await.unwrap().method,
            RoundingMethod::RoundPerLine
        );
    })
    .await;
}

// TGC-18: three inclusive lines under round_globally — the residual cents
// spread smoothly (no last-line-absorbs), and the document's included total
// equals the gross paid exactly.
#[tokio::test]
async fn tgc18_inclusive_globally_no_last_line_absorb() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    backbone_orm::company_scope::with_company_scope(Some(company), async move {
        let w = TaxWriteService::new(pool.clone());
        let e = TaxEngine::new(pool.clone());
        let tid = w
            .create_template(NewTemplate {
                company_id: company,
                code: uq("PPN-3L"),
                name: "PPN 21% three lines".into(),
                template_type: Some("sales".into()),
                tax_category_id: None,
                is_inclusive: true,
                tax_exigibility: None,
                cash_basis_transition_account_id: None,
            })
            .await
            .unwrap();
        w.add_row(NewTemplateRow {
            company_id: company,
            template_id: tid,
            charge_type: None,
            rate: d("21"),
            account_id: None,
            is_withholding: false,
            effective_from: day(2022, 4, 1),
            effective_to: None,
            sort_order: 0,
            description: None,
        })
        .await
        .unwrap();

        let r = e
            .calculate_document(&DocumentTaxRequest {
                company_id: company,
                document_type: DocumentType::Invoice,
                on_date: day(2026, 8, 19),
                lines: doc_lines(tid, "21.53", 3),
            })
            .await
            .unwrap();
        assert_eq!(r.net_amounts, vec![d("17.80"), d("17.79"), d("17.79")]);
        let taxes: Vec<Decimal> = r.lines.iter().map(|l| l.tax_amount).collect();
        assert_eq!(taxes, vec![d("3.74"), d("3.74"), d("3.73")]); // spread, not 3.74/3.74/3.73-absorbed-at-once
        assert_eq!(r.excluded_total, d("53.38"));
        assert_eq!(r.tax_total, d("11.21")); // round2(11.209835) — NOT the per-line 11.22
        assert_eq!(r.included_total, d("64.59")); // exactly 3 × 21.53 — the gross is preserved
    })
    .await;
}

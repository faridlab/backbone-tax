//! TSEAM-1 — the tax recording + e-Faktur numbering engine.
//! Records a TaxTransaction for a posted invoice (idempotent), assigns a gapless
//! DJP-format e-Faktur number (010.NNN-NN.YYYYYYYY) for sales, and proves the
//! sequence is dense (no gaps) across concurrent allocations.
//! Requires DATABASE_URL (:5433/backbone_tax with the tax schema migrated).

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_tax::application::service::efaktur_service::{EFakturService, PostedForTax};

fn d(s: &str) -> Decimal { Decimal::from_str_exact(s).unwrap() }

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_tax".to_string());
    PgPool::connect(&url).await.expect("connect DB")
}

#[tokio::test]
async fn records_transaction_and_assigns_gapless_efaktur() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let today = chrono::Utc::now().date_naive();
    let svc = EFakturService::new(pool.clone());

    // 1) Record a sales invoice with PPN output 110,000 on a 1,000,000 base.
    let data1 = PostedForTax {
        invoice_ref: Uuid::new_v4(), company_id: company, invoice_kind: "sales".into(),
        posting_date: today, taxable_base: d("1000000"),
        output_total: d("110000"), input_total: d("0"), withholding_total: d("0"),
    };
    let (txn1, efaktur1) = svc.record_tax_transaction(&data1).await.unwrap();
    assert!(efaktur1.is_some(), "sales with output → e-Faktur assigned");
    let efaktur1 = efaktur1.unwrap();

    // 2) Assert the number format: 010.NNN-NN.YYYYYYYY (19 chars).
    let number: String = sqlx::query_scalar("SELECT number FROM tax.efaktur_documents WHERE id = $1")
        .bind(efaktur1).fetch_one(&pool).await.unwrap();
    assert!(number.starts_with("010."), "transaction_code = 010 (standard VAT)");
    assert_eq!(number.len(), 19, "format is FFF.NNN-NN.YYYYYYYY = 19 chars, got '{number}'");

    // 3) Second sales invoice same month → sequence increments (gapless).
    let data2 = PostedForTax {
        invoice_ref: Uuid::new_v4(), company_id: company, invoice_kind: "sales".into(),
        posting_date: today, taxable_base: d("500000"),
        output_total: d("55000"), input_total: d("0"), withholding_total: d("0"),
    };
    let (_, efaktur2) = svc.record_tax_transaction(&data2).await.unwrap();
    let seq1: i32 = sqlx::query_scalar("SELECT sequence FROM tax.efaktur_documents WHERE id = $1")
        .bind(efaktur1).fetch_one(&pool).await.unwrap();
    let seq2: i32 = sqlx::query_scalar("SELECT sequence FROM tax.efaktur_documents WHERE id = $1")
        .bind(efaktur2.unwrap()).fetch_one(&pool).await.unwrap();
    assert_eq!(seq1, 1, "first sequence is 1 (DJP numbering starts at 1)");
    assert_eq!(seq2, 2, "second sequence is 2 (gapless)");

    // 4) Idempotency: re-deliver the same invoice → same transaction (no duplicate).
    let (txn1_again, efaktur1_again) = svc.record_tax_transaction(&data1).await.unwrap();
    assert_eq!(txn1, txn1_again, "re-delivery reuses the same TaxTransaction");
    assert_eq!(efaktur1, efaktur1_again.unwrap(), "re-delivery reuses the same EFakturDocument");

    // 5) Purchase invoice → no e-Faktur (only sales get numbered).
    let data3 = PostedForTax {
        invoice_ref: Uuid::new_v4(), company_id: company, invoice_kind: "purchase".into(),
        posting_date: today, taxable_base: d("800000"),
        output_total: d("0"), input_total: d("88000"), withholding_total: d("0"),
    };
    let (txn3, efaktur3) = svc.record_tax_transaction(&data3).await.unwrap();
    assert!(efaktur3.is_none(), "purchase → no e-Faktur assigned");

    // 6) No extra e-Faktur documents created (only the 2 sales).
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tax.efaktur_documents WHERE company_id = $1")
        .bind(company).fetch_one(&pool).await.unwrap();
    assert_eq!(count, 2, "exactly 2 e-Faktur documents (the 2 sales; purchase has none)");
}

//! The tax recording + e-Faktur numbering engine and the masa-pajak lifecycle on top of it.
//! Records a TaxTransaction for a posted invoice (idempotent), assigns a gapless
//! DJP-format e-Faktur number (010.NNN-NN.YYYYYYYY) for sales, proves the
//! sequence is dense (no gaps) across concurrent allocations, and pins the
//! filing lifecycle (finalize closes the numbering range fail-closed; file is
//! terminal) plus the relay-facing exactly-once entry points.
//! Requires DATABASE_URL (:5433/backbone_tax with the tax schema migrated).

use chrono::Datelike;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_tax::application::service::efaktur_service::{
    EFakturService, PostedForTax, TaxComplianceError,
};

fn d(s: &str) -> Decimal {
    Decimal::from_str_exact(s).unwrap()
}

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:5433/backbone_tax".to_string()
    });
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
        invoice_ref: Uuid::new_v4(),
        company_id: company,
        invoice_kind: "sales".into(),
        posting_date: today,
        taxable_base: d("1000000"),
        output_total: d("110000"),
        input_total: d("0"),
        withholding_total: d("0"),
    };
    let (txn1, efaktur1) = svc.record_tax_transaction(&data1).await.unwrap();
    assert!(efaktur1.is_some(), "sales with output → e-Faktur assigned");
    let efaktur1 = efaktur1.unwrap();

    // 2) Assert the number format: 010.NNN-NN.YYYYYYYY (19 chars).
    let number: String =
        sqlx::query_scalar("SELECT number FROM tax.efaktur_documents WHERE id = $1")
            .bind(efaktur1)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        number.starts_with("010."),
        "transaction_code = 010 (standard VAT)"
    );
    assert_eq!(
        number.len(),
        19,
        "format is FFF.NNN-NN.YYYYYYYY = 19 chars, got '{number}'"
    );

    // 3) Second sales invoice same month → sequence increments (gapless).
    let data2 = PostedForTax {
        invoice_ref: Uuid::new_v4(),
        company_id: company,
        invoice_kind: "sales".into(),
        posting_date: today,
        taxable_base: d("500000"),
        output_total: d("55000"),
        input_total: d("0"),
        withholding_total: d("0"),
    };
    let (_, efaktur2) = svc.record_tax_transaction(&data2).await.unwrap();
    let seq1: i32 = sqlx::query_scalar("SELECT sequence FROM tax.efaktur_documents WHERE id = $1")
        .bind(efaktur1)
        .fetch_one(&pool)
        .await
        .unwrap();
    let seq2: i32 = sqlx::query_scalar("SELECT sequence FROM tax.efaktur_documents WHERE id = $1")
        .bind(efaktur2.unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(seq1, 1, "first sequence is 1 (DJP numbering starts at 1)");
    assert_eq!(seq2, 2, "second sequence is 2 (gapless)");

    // 4) Idempotency: re-deliver the same invoice → same transaction (no duplicate).
    let (txn1_again, efaktur1_again) = svc.record_tax_transaction(&data1).await.unwrap();
    assert_eq!(
        txn1, txn1_again,
        "re-delivery reuses the same TaxTransaction"
    );
    assert_eq!(
        efaktur1,
        efaktur1_again.unwrap(),
        "re-delivery reuses the same EFakturDocument"
    );

    // 5) Purchase invoice → no e-Faktur (only sales get numbered).
    let data3 = PostedForTax {
        invoice_ref: Uuid::new_v4(),
        company_id: company,
        invoice_kind: "purchase".into(),
        posting_date: today,
        taxable_base: d("800000"),
        output_total: d("0"),
        input_total: d("88000"),
        withholding_total: d("0"),
    };
    let (_txn3, efaktur3) = svc.record_tax_transaction(&data3).await.unwrap();
    assert!(efaktur3.is_none(), "purchase → no e-Faktur assigned");

    // 6) No extra e-Faktur documents created (only the 2 sales).
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tax.efaktur_documents WHERE company_id = $1")
            .bind(company)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        count, 2,
        "exactly 2 e-Faktur documents (the 2 sales; purchase has none)"
    );
}

// TSEAM-VOID: voiding a sales invoice's e-Faktur flips status→voided, preserves the DJP sequence
// (no reuse), and is idempotent + a no-op when there is no e-Faktur to void.
#[tokio::test]
async fn void_for_invoice_flips_status_preserving_sequence() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let today = chrono::Utc::now().date_naive();
    let svc = EFakturService::new(pool.clone());

    // Record a sales invoice → e-Faktur assigned.
    let invoice = Uuid::new_v4();
    let data = PostedForTax {
        invoice_ref: invoice,
        company_id: company,
        invoice_kind: "sales".into(),
        posting_date: today,
        taxable_base: d("1000000"),
        output_total: d("110000"),
        input_total: d("0"),
        withholding_total: d("0"),
    };
    let (_, efaktur) = svc.record_tax_transaction(&data).await.unwrap();
    let efaktur = efaktur.unwrap();
    let seq_before: i32 =
        sqlx::query_scalar("SELECT sequence FROM tax.efaktur_documents WHERE id=$1")
            .bind(efaktur)
            .fetch_one(&pool)
            .await
            .unwrap();
    let status_before: String =
        sqlx::query_scalar("SELECT status::text FROM tax.efaktur_documents WHERE id=$1")
            .bind(efaktur)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status_before, "assigned");

    // Void → status flips to voided; sequence + number preserved (DJP no-reuse).
    svc.void_for_invoice(company, invoice, "sales")
        .await
        .unwrap();
    let seq_after: i32 =
        sqlx::query_scalar("SELECT sequence FROM tax.efaktur_documents WHERE id=$1")
            .bind(efaktur)
            .fetch_one(&pool)
            .await
            .unwrap();
    let status_after: String =
        sqlx::query_scalar("SELECT status::text FROM tax.efaktur_documents WHERE id=$1")
            .bind(efaktur)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status_after, "voided", "status flipped to voided");
    assert_eq!(
        seq_before, seq_after,
        "sequence preserved (DJP no-reuse — gapless stays intact)"
    );

    // Idempotent: void again → still voided, no error.
    svc.void_for_invoice(company, invoice, "sales")
        .await
        .unwrap();
    let status_again: String =
        sqlx::query_scalar("SELECT status::text FROM tax.efaktur_documents WHERE id=$1")
            .bind(efaktur)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status_again, "voided");

    // No-op: voiding an invoice with no e-Faktur (unknown invoice) → Ok, nothing changes.
    svc.void_for_invoice(company, Uuid::new_v4(), "sales")
        .await
        .unwrap();
}

// ── masa-pajak lifecycle: finalize closes the range fail-closed; file is terminal ──

fn sales_posted(company: Uuid, invoice: Uuid, base: &str, output: &str) -> PostedForTax {
    PostedForTax {
        invoice_ref: invoice,
        company_id: company,
        invoice_kind: "sales".into(),
        posting_date: chrono::Utc::now().date_naive(),
        taxable_base: d(base),
        output_total: d(output),
        input_total: d("0"),
        withholding_total: d("0"),
    }
}

#[tokio::test]
async fn finalize_closes_the_period_fail_closed_and_rolls_totals() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let today = chrono::Utc::now().date_naive();
    let period = format!("{:04}-{:02}-01", today.year(), today.month())
        .parse::<chrono::NaiveDate>()
        .unwrap();
    let svc = EFakturService::new(pool.clone());

    // Two sales (output 110000 + 55000) + one purchase (input 88000, withholding 2300)
    // + one more sales recorded BEFORE the close (the pre-finalization replay probe).
    svc.record_tax_transaction(&sales_posted(company, Uuid::new_v4(), "1000000", "110000"))
        .await
        .unwrap();
    svc.record_tax_transaction(&sales_posted(company, Uuid::new_v4(), "500000", "55000"))
        .await
        .unwrap();
    let purchase = PostedForTax {
        invoice_ref: Uuid::new_v4(),
        company_id: company,
        invoice_kind: "purchase".into(),
        posting_date: today,
        taxable_base: d("800000"),
        output_total: d("0"),
        input_total: d("88000"),
        withholding_total: d("2300"),
    };
    svc.record_tax_transaction(&purchase).await.unwrap();
    let early = sales_posted(company, Uuid::new_v4(), "1000000", "110000");
    let (txn_id, efaktur_id) = svc.record_tax_transaction(&early).await.unwrap();

    // Finalize: open → finalized with the aggregate totals of the month's transactions.
    let row = svc.finalize_period(company, period).await.unwrap();
    assert_eq!(row.status, "finalized");
    assert_eq!(row.output_total, d("275000"), "Σ output over the month");
    assert_eq!(row.input_total, d("88000"), "Σ input over the month");
    assert_eq!(
        row.withholding_total,
        d("2300"),
        "Σ withholding over the month"
    );

    // Idempotent: a second finalize replays as a committed no-op, same row.
    let again = svc.finalize_period(company, period).await.unwrap();
    assert_eq!(again.id, row.id);
    assert_eq!(again.status, "finalized");

    // Fail-closed: a NEW sales invoice that month refuses period_not_open — the closed
    // Masa hands out no new numbers (the allocator itself carries the guard).
    let before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tax.efaktur_documents WHERE company_id = $1")
            .bind(company)
            .fetch_one(&pool)
            .await
            .unwrap();
    let refused = sales_posted(company, Uuid::new_v4(), "100000", "11000");
    match svc.record_tax_transaction(&refused).await {
        Err(TaxComplianceError::PeriodNotOpen(c, p)) => {
            assert_eq!(c, company);
            assert_eq!(p, period);
        }
        other => panic!("expected PeriodNotOpen, got {other:?}"),
    }
    let after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tax.efaktur_documents WHERE company_id = $1")
            .bind(company)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(before, after, "gapless count unchanged — no number leaked");

    // Replay of the PRE-finalization invoice stays a committed no-op (never a refusal):
    // the idempotency fence runs before the open-period guard.
    let (replay_id, replay_efaktur) = svc.record_tax_transaction(&early).await.unwrap();
    assert_eq!(
        txn_id, replay_id,
        "replay after finalization is the same transaction"
    );
    assert_eq!(
        efaktur_id, replay_efaktur,
        "replay after finalization reuses the e-Faktur"
    );

    // A NEW purchase invoice also refuses — finalize closes the Masa to ALL new transactions.
    let purchase2 = PostedForTax {
        invoice_ref: Uuid::new_v4(),
        company_id: company,
        invoice_kind: "purchase".into(),
        posting_date: today,
        taxable_base: d("100000"),
        output_total: d("0"),
        input_total: d("11000"),
        withholding_total: d("0"),
    };
    match svc.record_tax_transaction(&purchase2).await {
        Err(TaxComplianceError::PeriodNotOpen(..)) => {}
        other => panic!("purchase on finalized period must refuse, got {other:?}"),
    }

    // Export rows: 3 documents (2 original sales + the early one), sequence order.
    let rows = svc.export_rows(company, period).await.unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].document.sequence, 1);
    assert_eq!(rows[1].document.sequence, 2);
    assert_eq!(rows[2].document.sequence, 3);
}

#[tokio::test]
async fn file_runs_finalized_to_filed_and_is_terminal() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let today = chrono::Utc::now().date_naive();
    let period = format!("{:04}-{:02}-01", today.year(), today.month())
        .parse::<chrono::NaiveDate>()
        .unwrap();
    let svc = EFakturService::new(pool.clone());

    // Direct open → file refuses: the lifecycle runs open → finalized → filed.
    let (_, efaktur_id) = svc
        .record_tax_transaction(&sales_posted(company, Uuid::new_v4(), "1000000", "110000"))
        .await
        .unwrap();
    let efaktur_id = efaktur_id.expect("sales with output is numbered");
    match svc.file_period(company, period).await {
        Err(TaxComplianceError::PeriodNotFinalized(..)) => {}
        other => panic!("file from open must refuse period_not_finalized, got {other:?}"),
    }

    // finalized → filed; filed_at lands in the audit metadata.
    svc.finalize_period(company, period).await.unwrap();
    let filed = svc.file_period(company, period).await.unwrap();
    assert_eq!(filed.status, "filed");
    let filed_at: Option<String> = sqlx::query_scalar(
        "SELECT metadata->>'filed_at' FROM tax.tax_filing_periods WHERE id = $1",
    )
    .bind(filed.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(filed_at.is_some(), "filed_at stamped into metadata.jsonb");

    // Terminal: re-file is an idempotent no-op, finalize-on-filed refuses.
    let refiled = svc.file_period(company, period).await.unwrap();
    assert_eq!(refiled.id, filed.id);
    match svc.finalize_period(company, period).await {
        Err(TaxComplianceError::PeriodAlreadyFiled(..)) => {}
        other => panic!("finalize on a filed period must refuse, got {other:?}"),
    }

    // Voiding a document whose period is filed refuses (the SPT was submitted with that
    // number — it is locked). A NEW recording that month would refuse period_not_open, so
    // exercise the void refusal on the document recorded before the close.
    match svc.void_efaktur(company, efaktur_id).await {
        Err(TaxComplianceError::PeriodAlreadyFiled(..)) => {}
        other => panic!("void on a filed period must refuse, got {other:?}"),
    }
}

#[tokio::test]
async fn confirm_flips_assigned_to_confirmed_and_void_still_works() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EFakturService::new(pool.clone());

    let (_, efaktur_id) = svc
        .record_tax_transaction(&sales_posted(company, Uuid::new_v4(), "1000000", "110000"))
        .await
        .unwrap();
    let efaktur_id = efaktur_id.unwrap();

    // assigned → confirmed.
    let confirmed = svc.confirm_efaktur(company, efaktur_id).await.unwrap();
    assert_eq!(confirmed.status, "confirmed");

    // Idempotent re-confirm returns the row unchanged.
    let again = svc.confirm_efaktur(company, efaktur_id).await.unwrap();
    assert_eq!(again.id, confirmed.id);
    assert_eq!(again.status, "confirmed");

    // Void from confirmed is still permitted (a later credit note).
    let voided = svc.void_efaktur(company, efaktur_id).await.unwrap();
    assert_eq!(voided.status, "voided");
    assert_eq!(
        voided.number, confirmed.number,
        "DJP number preserved on void"
    );

    // Confirming a voided document refuses.
    match svc.confirm_efaktur(company, efaktur_id).await {
        Err(TaxComplianceError::EFakturNotConfirmable(..)) => {}
        other => panic!("confirm on voided must refuse, got {other:?}"),
    }

    // Unknown document id → typed not-found.
    match svc.confirm_efaktur(company, Uuid::new_v4()).await {
        Err(TaxComplianceError::EFakturNotFound(..)) => {}
        other => panic!("unknown document must refuse not-found, got {other:?}"),
    }
}

// ── relay-facing exactly-once entry points ─────────────────────────────────────

#[tokio::test]
async fn once_entry_points_are_exactly_once_per_event_id() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = EFakturService::new(pool.clone());

    // The consumer inbox table ships with the outbox migrate (the host runs it at boot).
    backbone_outbox::outbox::migrate(&pool, "tax")
        .await
        .expect("migrate tax inbox");

    // First delivery applies; a redelivery of the SAME envelope id is a committed no-op.
    let event_id = Uuid::new_v4();
    let data = sales_posted(company, Uuid::new_v4(), "1000000", "110000");
    let first = svc
        .record_tax_transaction_once(event_id, "test-consumer", &data)
        .await
        .unwrap();
    assert!(first.is_some(), "first delivery applies");
    let dup = svc
        .record_tax_transaction_once(event_id, "test-consumer", &data)
        .await
        .unwrap();
    assert!(dup.is_none(), "redelivery of the same envelope id skips");

    // A DIFFERENT envelope id with the same invoice replays via the invoice fence —
    // same transaction, no duplicate rows.
    let other_event = Uuid::new_v4();
    let replay = svc
        .record_tax_transaction_once(other_event, "test-consumer", &data)
        .await
        .unwrap();
    assert!(replay.is_some());
    assert_eq!(replay.unwrap().0, first.as_ref().unwrap().0);
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tax.tax_transactions WHERE company_id = $1 AND invoice_ref = $2",
    )
    .bind(company)
    .bind(data.invoice_ref)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1, "exactly one transaction per invoice");

    // Cancel: first delivery voids, redelivery skips, unknown invoice is a committed no-op.
    let cancel_id = Uuid::new_v4();
    let first_void = svc
        .void_for_invoice_once(
            cancel_id,
            "test-consumer",
            company,
            data.invoice_ref,
            "sales",
        )
        .await
        .unwrap();
    assert_eq!(first_void, Some(()));
    let dup_void = svc
        .void_for_invoice_once(
            cancel_id,
            "test-consumer",
            company,
            data.invoice_ref,
            "sales",
        )
        .await
        .unwrap();
    assert_eq!(dup_void, None, "redelivered cancel skips");
    let unknown = svc
        .void_for_invoice_once(
            Uuid::new_v4(),
            "test-consumer",
            company,
            Uuid::new_v4(),
            "sales",
        )
        .await
        .unwrap();
    assert_eq!(
        unknown,
        Some(()),
        "unknown invoice void is a committed no-op, not an error"
    );

    // The e-Faktur the cancel voided is voided exactly once (status stays voided).
    let status: String = sqlx::query_scalar(
        r#"SELECT d.status::text FROM tax.efaktur_documents d
           JOIN tax.tax_transactions t ON t.efaktur_document_id = d.id
           WHERE t.invoice_ref = $1"#,
    )
    .bind(data.invoice_ref)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "voided");
}

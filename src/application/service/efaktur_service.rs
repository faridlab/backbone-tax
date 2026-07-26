//! The e-Faktur + tax-recording engine (hand-authored, user-owned).
//!
//! `record_tax_transaction` records an immutable TaxTransaction for a posted billing invoice
//! (idempotent on company+invoice_ref+invoice_kind). For SALES invoices, it also assigns an
//! EFakturDocument with a gapless DJP-format number (010.NNN-NN.YYYYYYYY). The composition layer
//! calls this when billing emits SalesInvoicePosted/PurchaseInvoicePosted.
//!
//! Zero cargo edges: tax never imports billing. The composition ACL passes the invoice data.

use backbone_orm::company_scope;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub enum TaxComplianceError {
    NoFilingPeriod(Uuid, NaiveDate),
    Db(sqlx::Error),
}
impl std::fmt::Display for TaxComplianceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaxComplianceError::NoFilingPeriod(c, d) => write!(f, "no open TaxFilingPeriod for company {c} in {d}"),
            TaxComplianceError::Db(e) => write!(f, "db: {e}"),
        }
    }
}
impl std::error::Error for TaxComplianceError {}
impl From<sqlx::Error> for TaxComplianceError {
    fn from(e: sqlx::Error) -> Self { TaxComplianceError::Db(e) }
}

#[derive(Clone)]
pub struct EFakturService {
    db_pool: PgPool,
}

/// The tax data the composition ACL extracts from a billing posted event.
#[derive(Debug, Clone)]
pub struct PostedForTax {
    pub invoice_ref: Uuid,
    pub company_id: Uuid,
    pub invoice_kind: String, // "sales" | "purchase"
    pub posting_date: NaiveDate,
    pub taxable_base: Decimal,
    pub output_total: Decimal,
    pub input_total: Decimal,
    pub withholding_total: Decimal,
}

impl EFakturService {
    pub fn new(db_pool: PgPool) -> Self { Self { db_pool } }

    /// Record a TaxTransaction for a posted invoice. For SALES, also assigns an e-Faktur number.
    /// Idempotent: the unique (company, invoice_ref, invoice_kind) fence means a re-delivery of the
    /// same posted event is a no-op (returns the existing transaction).
    pub async fn record_tax_transaction(
        &self, data: &PostedForTax,
    ) -> Result<(Uuid, Option<Uuid>), TaxComplianceError> {
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, data.company_id).await?;

        // Idempotent insert (unique company + invoice_ref + invoice_kind).
        let row = sqlx::query(
            r#"INSERT INTO tax.tax_transactions
                 (id, company_id, invoice_ref, invoice_kind, posting_date, taxable_base,
                  output_total, input_total, withholding_total, status)
               VALUES ($1, $2, $3, $4::invoice_kind, $5, $6, $7, $8, $9, 'recorded'::tax_transaction_status)
               ON CONFLICT (company_id, invoice_ref, invoice_kind) WHERE (metadata->>'deleted_at') IS NULL
               DO UPDATE SET status = tax.tax_transactions.status
               RETURNING id"#,
        )
        .bind(Uuid::new_v4()).bind(data.company_id).bind(data.invoice_ref)
        .bind(&data.invoice_kind).bind(data.posting_date).bind(data.taxable_base)
        .bind(data.output_total).bind(data.input_total).bind(data.withholding_total)
        .fetch_one(&mut *tx).await?;
        let txn_id: Uuid = row.get("id");

        // For sales with output: assign an e-Faktur number (gapless, DJP format) — idempotent:
        // if the transaction already has one (re-delivery), reuse it.
        let efaktur_id = if data.invoice_kind == "sales" && data.output_total > Decimal::ZERO {
            let existing: Option<Uuid> = sqlx::query_scalar(
                "SELECT efaktur_document_id FROM tax.tax_transactions WHERE id = $1")
                .bind(txn_id).fetch_one(&mut *tx).await?;
            if let Some(eid) = existing {
                Some(eid) // reuse the existing e-Faktur (idempotent re-delivery)
            } else {
                let eid = self.assign_efaktur_in_tx(&mut tx, txn_id, data.company_id, data.posting_date).await?;
                sqlx::query("UPDATE tax.tax_transactions SET efaktur_document_id = $2 WHERE id = $1")
                    .bind(txn_id).bind(eid).execute(&mut *tx).await?;
                Some(eid)
            }
        } else {
            None
        };

        tx.commit().await?;
        Ok((txn_id, efaktur_id))
    }

    /// Allocate a gapless e-Faktur number (010.NNN-NN.YYYYYYYY) via the TaxFilingPeriod sequence.
    /// Concurrent calls serialize on the per-period row (the UPDATE ... RETURNING is atomic).
    async fn assign_efaktur_in_tx(
        &self, tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        txn_id: Uuid, company_id: Uuid, posting_date: NaiveDate,
    ) -> Result<Uuid, TaxComplianceError> {
        let period_start = posting_date.format("%Y-%m-01").to_string().parse::<NaiveDate>().unwrap();

        // Ensure a TaxFilingPeriod exists for this month (auto-open if missing).
        sqlx::query(
            r#"INSERT INTO tax.tax_filing_periods (id, company_id, period, status)
               VALUES ($1, $2, $3, 'open'::tax_filing_status)
               ON CONFLICT (company_id, period) WHERE (metadata->>'deleted_at') IS NULL
               DO NOTHING"#,
        )
        .bind(Uuid::new_v4()).bind(company_id).bind(period_start)
        .execute(&mut **tx).await?;

        // Atomically allocate the next sequence (gapless — serializes on the row lock).
        let row = sqlx::query(
            r#"UPDATE tax.tax_filing_periods
                 SET next_sequence = next_sequence + 1
               WHERE company_id = $1 AND period = $2 AND (metadata->>'deleted_at') IS NULL
               RETURNING next_sequence - 1 AS seq, COALESCE(taxpayer_segment, '000') AS seg"#,
        )
        .bind(company_id).bind(period_start)
        .fetch_one(&mut **tx).await?;
        let seq: i32 = row.get("seq");
        let seg: String = row.get("seg");
        let month = posting_date.format("%m").to_string();
        let number = format!("010.{}-{}.{:08}", seg, month, seq);

        // Insert the EFakturDocument.
        let eid = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO tax.efaktur_documents
                 (id, company_id, tax_transaction_id, number, transaction_code,
                  taxpayer_segment, period, sequence, assignment_date, status)
               VALUES ($1, $2, $3, $4, '010', $5, $6, $7, $8, 'assigned'::e_faktur_status)"#,
        )
        .bind(eid).bind(company_id).bind(txn_id).bind(&number)
        .bind(&seg).bind(period_start).bind(seq).bind(posting_date)
        .execute(&mut **tx).await?;

        Ok(eid)
    }
}

//! The e-Faktur + tax-recording engine (hand-authored, user-owned).
//!
//! `record_tax_transaction` records an immutable TaxTransaction for a posted billing invoice
//! (idempotent on company+invoice_ref+invoice_kind). For SALES invoices, it also assigns an
//! EFakturDocument with a gapless DJP-format number (010.NNN-NN.YYYYYYYY). The composition layer
//! calls this when billing emits SalesInvoicePosted/PurchaseInvoicePosted.
//!
//! Zero cargo edges: tax never imports billing. The composition ACL passes the invoice data.
//!
//! All SQL lives in the repositories (tax_transaction / tax_filing_period / e_faktur_document);
//! this service only orchestrates. 4-layer rule.

use backbone_orm::company_scope;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    AllocatedSequence, NewEFakturDocumentRow, NewTaxTransactionRow, EFakturDocumentRepository,
    TaxFilingPeriodRepository, TaxTransactionRepository,
};

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

        // Idempotent insert (unique company + invoice_ref + invoice_kind). Repository returns the
        // row's id whether the insert succeeded (fresh) or the ON CONFLICT DO UPDATE branch fired
        // (re-delivery) — same observable behavior as the raw-SQL original.
        let txn_id = {
            let id = Uuid::new_v4();
            let txns = TaxTransactionRepository::new(self.db_pool.clone());
            txns.upsert_recorded(&mut *tx, &NewTaxTransactionRow {
                id, company_id: data.company_id, invoice_ref: data.invoice_ref,
                invoice_kind: &data.invoice_kind, posting_date: data.posting_date,
                taxable_base: data.taxable_base, output_total: data.output_total,
                input_total: data.input_total, withholding_total: data.withholding_total,
            }).await?
        };

        // For sales with output: assign an e-Faktur number (gapless, DJP format) — idempotent:
        // if the transaction already has one (re-delivery), reuse it.
        let efaktur_id = if data.invoice_kind == "sales" && data.output_total > Decimal::ZERO {
            let txns = TaxTransactionRepository::new(self.db_pool.clone());
            let existing = txns.find_efaktur_id(&mut *tx, txn_id).await?;
            if let Some(eid) = existing {
                Some(eid) // reuse the existing e-Faktur (idempotent re-delivery)
            } else {
                let eid = self.assign_efaktur_in_tx(&mut tx, txn_id, data.company_id, data.posting_date).await?;
                txns.attach_efaktur(&mut *tx, txn_id, eid).await?;
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
        let periods = TaxFilingPeriodRepository::new(self.db_pool.clone());
        periods.ensure_open(&mut **tx, Uuid::new_v4(), company_id, period_start).await?;

        // Atomically allocate the next sequence (gapless — serializes on the row lock).
        let AllocatedSequence { seq, seg } = periods
            .allocate_sequence(&mut **tx, company_id, period_start)
            .await?;
        let month = posting_date.format("%m").to_string();
        let number = format!("010.{}-{}.{:08}", seg, month, seq);

        // Insert the EFakturDocument.
        let eid = Uuid::new_v4();
        let docs = EFakturDocumentRepository::new(self.db_pool.clone());
        docs.insert(&mut **tx, &NewEFakturDocumentRow {
            id: eid, company_id, tax_transaction_id: txn_id, number: &number,
            taxpayer_segment: &seg, period: period_start, sequence: seq,
            assignment_date: posting_date,
        }).await?;

        Ok(eid)
    }
}

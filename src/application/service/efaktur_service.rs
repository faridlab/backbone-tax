//! The e-Faktur + tax-recording engine (hand-authored, user-owned).
//!
//! `record_tax_transaction` records an immutable TaxTransaction for a posted billing invoice
//! (idempotent on company+invoice_ref+invoice_kind). For SALES invoices, it also assigns an
//! EFakturDocument with a gapless DJP-format number (010.NNN-NN.YYYYYYYY). The composition layer
//! calls this when billing emits SalesInvoicePosted/PurchaseInvoicePosted.
//!
//! The masa-pajak lifecycle on top of the numbering:
//! - `finalize_period` CAS-flips a period open→finalized and writes the aggregate VAT totals
//!   (the regulatory close: a finalized period no longer accepts new tax transactions or numbers).
//! - `file_period` CAS-flips finalized→filed and stamps `filed_at` into the audit metadata
//!   (the SPT submission record; filed is terminal).
//! - `confirm_efaktur` / `void_efaktur` manage the document lifecycle (assigned→confirmed; void
//!   preserves the DJP number forever).
//!
//! Zero cargo edges to other domain modules: tax never imports billing. The composition ACL
//! passes the invoice data; the only infra dependency is the framework outbox crate, whose
//! inbox dedup the `_once` entry points claim INSIDE the effect's transaction (the relay's
//! at-least-once delivery becomes an exactly-once effect — the same posture as billing's and
//! payment's settlement consumers).
//!
//! All SQL lives in the repositories (tax_transaction / tax_filing_period / e_faktur_document);
//! this service only orchestrates. 4-layer rule.

use backbone_orm::company_scope;
use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    AllocatedSequence, EFakturDocumentRepository, EFakturDocumentRow, EFakturExportRow,
    FilingPeriodRow, NewEFakturDocumentRow, NewTaxTransactionRow, TaxFilingPeriodRepository,
    TaxTransactionRepository,
};

/// The schema holding this module's consumer inbox (`tax.inbox_consumed`). Created by the
/// composing host via `backbone_outbox::outbox::migrate(pool, EFAKTUR_INBOX_SCHEMA)`; tax is a
/// pure consumer of billing's events and never stages outbox rows of its own.
pub const EFAKTUR_INBOX_SCHEMA: &str = "tax";

#[derive(Debug)]
pub enum TaxComplianceError {
    NoFilingPeriod(Uuid, NaiveDate),
    /// The masa pajak is closed (finalized or filed) — it cannot accept a new tax transaction or
    /// hand out a new e-Faktur number. Existing transactions replay idempotently; only NEW ones
    /// refuse.
    PeriodNotOpen(Uuid, NaiveDate),
    /// Filing was attempted on a period that is not finalized (the lifecycle runs
    /// open → finalized → filed; filing skips no step).
    PeriodNotFinalized(Uuid, NaiveDate),
    /// The period was already filed — terminal, no lifecycle verb transitions out.
    PeriodAlreadyFiled(Uuid, NaiveDate),
    /// No e-Faktur document with the given id exists in scope.
    EFakturNotFound(Uuid),
    /// The document's status cannot take the requested transition (e.g. confirming a voided
    /// document). Carries the current status text for the error envelope.
    EFakturNotConfirmable(Uuid, String),
    Db(sqlx::Error),
}
impl std::fmt::Display for TaxComplianceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaxComplianceError::NoFilingPeriod(c, d) => {
                write!(f, "no open TaxFilingPeriod for company {c} in {d}")
            }
            TaxComplianceError::PeriodNotOpen(c, d) => write!(
                f,
                "the masa pajak for company {c} starting {d} is closed (finalized or filed) — \
                 it cannot accept new tax transactions"
            ),
            TaxComplianceError::PeriodNotFinalized(c, d) => write!(
                f,
                "the masa pajak for company {c} starting {d} is not finalized — finalize before filing"
            ),
            TaxComplianceError::PeriodAlreadyFiled(c, d) => write!(
                f,
                "the masa pajak for company {c} starting {d} is already filed — filed is terminal"
            ),
            TaxComplianceError::EFakturNotFound(id) => {
                write!(f, "no e-Faktur document {id} in scope")
            }
            TaxComplianceError::EFakturNotConfirmable(id, status) => write!(
                f,
                "e-Faktur document {id} is '{status}' and cannot take that transition"
            ),
            TaxComplianceError::Db(e) => write!(f, "db: {e}"),
        }
    }
}
impl std::error::Error for TaxComplianceError {}
impl From<sqlx::Error> for TaxComplianceError {
    fn from(e: sqlx::Error) -> Self {
        TaxComplianceError::Db(e)
    }
}
impl TaxComplianceError {
    /// Stable error code for the HTTP envelope (same shape as the tax engine's `TaxError::code`).
    pub fn code(&self) -> &'static str {
        match self {
            TaxComplianceError::NoFilingPeriod(..) => "no_filing_period",
            TaxComplianceError::PeriodNotOpen(..) => "period_not_open",
            TaxComplianceError::PeriodNotFinalized(..) => "period_not_finalized",
            TaxComplianceError::PeriodAlreadyFiled(..) => "period_already_filed",
            TaxComplianceError::EFakturNotFound(..) => "efaktur_not_found",
            TaxComplianceError::EFakturNotConfirmable(..) => "efaktur_not_confirmable",
            TaxComplianceError::Db(..) => "db_error",
        }
    }

    /// HTTP status for the error envelope.
    pub fn http_status(&self) -> u16 {
        match self {
            TaxComplianceError::NoFilingPeriod(..) => 404,
            TaxComplianceError::PeriodNotOpen(..) => 409,
            TaxComplianceError::PeriodNotFinalized(..) => 409,
            TaxComplianceError::PeriodAlreadyFiled(..) => 409,
            TaxComplianceError::EFakturNotFound(..) => 404,
            TaxComplianceError::EFakturNotConfirmable(..) => 409,
            TaxComplianceError::Db(..) => 500,
        }
    }
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
    pub fn new(db_pool: PgPool) -> Self {
        Self { db_pool }
    }

    /// Record a TaxTransaction for a posted invoice. For SALES, also assigns an e-Faktur number.
    /// Idempotent: the unique (company, invoice_ref, invoice_kind) fence means a re-delivery of the
    /// same posted event is a no-op (returns the existing transaction) — including re-deliveries
    /// that arrive after the period was finalized (the fence probe runs BEFORE the open-period
    /// guard, so a closed period refuses only NEW transactions, never replays).
    pub async fn record_tax_transaction(
        &self,
        data: &PostedForTax,
    ) -> Result<(Uuid, Option<Uuid>), TaxComplianceError> {
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, data.company_id).await?;
        let out = self.record_in_tx(&mut tx, data).await?;
        tx.commit().await?;
        Ok(out)
    }

    /// The relay-facing twin of [`Self::record_tax_transaction`]: claims the bus `event_id` in the
    /// consumer inbox and applies the effect in the SAME transaction, so the relay's at-least-once
    /// delivery becomes an exactly-once effect. `Ok(None)` = the event was already consumed (the
    /// caller skips); `Ok(Some(..))` carries the recording outcome; `Err` leaves both the claim
    /// and the effect rolled back for redelivery.
    pub async fn record_tax_transaction_once(
        &self,
        event_id: Uuid,
        consumer: &str,
        data: &PostedForTax,
    ) -> Result<Option<(Uuid, Option<Uuid>)>, TaxComplianceError> {
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, data.company_id).await?;
        let first =
            backbone_outbox::inbox::once(&mut *tx, EFAKTUR_INBOX_SCHEMA, consumer, event_id)
                .await
                .map_err(|e| {
                    TaxComplianceError::Db(sqlx::Error::Protocol(format!("inbox claim: {e}")))
                })?;
        if !first {
            tx.commit().await?;
            return Ok(None);
        }
        let out = self.record_in_tx(&mut tx, data).await?;
        tx.commit().await?;
        Ok(Some(out))
    }

    /// The recording unit of work, on a caller-provided transaction with the company already
    /// bound.
    async fn record_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        data: &PostedForTax,
    ) -> Result<(Uuid, Option<Uuid>), TaxComplianceError> {
        let txns = TaxTransactionRepository::new(self.db_pool.clone());

        // 1) Idempotency probe BEFORE the period guard: a re-delivery of an event whose
        //    transaction already landed (possibly before the period closed) replays as a no-op.
        if let Some(existing_id) = txns
            .find_id_by_invoice(
                &mut **tx,
                data.company_id,
                data.invoice_ref,
                &data.invoice_kind,
            )
            .await?
        {
            let efaktur = txns.find_efaktur_id(&mut **tx, existing_id).await?;
            return Ok((existing_id, efaktur));
        }

        // 2) The masa-pajak guard: a NEW transaction refuses when the posting month's period is
        //    finalized or filed. A missing period row is fine — the assignment path opens one.
        let period_start = month_start(data.posting_date);
        let periods = TaxFilingPeriodRepository::new(self.db_pool.clone());
        if let Some(row) = periods
            .find_by_company_period(&mut **tx, data.company_id, period_start)
            .await?
        {
            if row.status != "open" {
                return Err(TaxComplianceError::PeriodNotOpen(
                    data.company_id,
                    period_start,
                ));
            }
        }

        // 3) Idempotent insert (unique company + invoice_ref + invoice_kind). Repository returns the
        //    row's id whether the insert succeeded (fresh) or the ON CONFLICT DO UPDATE branch fired
        //    (re-delivery) — same observable behavior as the raw-SQL original.
        let txn_id = {
            let id = Uuid::new_v4();
            txns.upsert_recorded(
                &mut **tx,
                &NewTaxTransactionRow {
                    id,
                    company_id: data.company_id,
                    invoice_ref: data.invoice_ref,
                    invoice_kind: &data.invoice_kind,
                    posting_date: data.posting_date,
                    taxable_base: data.taxable_base,
                    output_total: data.output_total,
                    input_total: data.input_total,
                    withholding_total: data.withholding_total,
                },
            )
            .await?
        };

        // 4) For sales with output: assign an e-Faktur number (gapless, DJP format) — idempotent:
        //    if the transaction already has one (re-delivery), reuse it.
        let efaktur_id = if data.invoice_kind == "sales" && data.output_total > Decimal::ZERO {
            let existing = txns.find_efaktur_id(&mut **tx, txn_id).await?;
            if let Some(eid) = existing {
                Some(eid) // reuse the existing e-Faktur (idempotent re-delivery)
            } else {
                let eid = self
                    .assign_efaktur_in_tx(tx, txn_id, data.company_id, data.posting_date)
                    .await?;
                txns.attach_efaktur(&mut **tx, txn_id, eid).await?;
                Some(eid)
            }
        } else {
            None
        };

        Ok((txn_id, efaktur_id))
    }

    /// Void the e-Faktur assigned to a sales invoice (a credit note / cancellation). The DJP number
    /// is preserved (never reused) — only `status` flips to Voided, so the gapless sequence stays
    /// intact. Idempotent and a no-op when there is no e-Faktur for the invoice (purchase invoices,
    /// or a sales invoice not yet numbered). `invoice_kind` filters the tax transaction.
    pub async fn void_for_invoice(
        &self,
        company_id: Uuid,
        invoice_ref: Uuid,
        invoice_kind: &str,
    ) -> Result<(), TaxComplianceError> {
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        self.void_for_invoice_in_tx(&mut tx, company_id, invoice_ref, invoice_kind)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The relay-facing twin of [`Self::void_for_invoice`]: claims the bus `event_id` in the
    /// consumer inbox and applies the void in the same transaction (exactly-once effect).
    /// `Ok(None)` = already consumed.
    pub async fn void_for_invoice_once(
        &self,
        event_id: Uuid,
        consumer: &str,
        company_id: Uuid,
        invoice_ref: Uuid,
        invoice_kind: &str,
    ) -> Result<Option<()>, TaxComplianceError> {
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let first =
            backbone_outbox::inbox::once(&mut *tx, EFAKTUR_INBOX_SCHEMA, consumer, event_id)
                .await
                .map_err(|e| {
                    TaxComplianceError::Db(sqlx::Error::Protocol(format!("inbox claim: {e}")))
                })?;
        if !first {
            tx.commit().await?;
            return Ok(None);
        }
        self.void_for_invoice_in_tx(&mut tx, company_id, invoice_ref, invoice_kind)
            .await?;
        tx.commit().await?;
        Ok(Some(()))
    }

    /// The void unit of work, on a caller-provided transaction with the company already bound.
    async fn void_for_invoice_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        company_id: Uuid,
        invoice_ref: Uuid,
        invoice_kind: &str,
    ) -> Result<(), TaxComplianceError> {
        let txns = TaxTransactionRepository::new(self.db_pool.clone());
        if let Some(efaktur_id) = txns
            .find_efaktur_id_by_invoice(&mut **tx, company_id, invoice_ref, invoice_kind)
            .await?
        {
            let docs = EFakturDocumentRepository::new(self.db_pool.clone());
            docs.void_on(&mut **tx, efaktur_id).await?;
        }
        Ok(())
    }

    /// Void one e-Faktur document by id (the operator surface): flips the document to `voided`,
    /// preserving the DJP number and sequence. Idempotent on an already-voided document. Refuses
    /// with [`TaxComplianceError::PeriodAlreadyFiled`] when the document's masa pajak was already
    /// filed — numbers reported on a submitted SPT are locked.
    pub async fn void_efaktur(
        &self,
        company_id: Uuid,
        efaktur_id: Uuid,
    ) -> Result<EFakturDocumentRow, TaxComplianceError> {
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let docs = EFakturDocumentRepository::new(self.db_pool.clone());
        let doc = docs
            .find_on(&mut *tx, efaktur_id)
            .await?
            .ok_or(TaxComplianceError::EFakturNotFound(efaktur_id))?;
        if doc.status == "voided" {
            tx.commit().await?;
            return Ok(doc); // idempotent re-void
        }
        let periods = TaxFilingPeriodRepository::new(self.db_pool.clone());
        if let Some(period) = periods
            .find_by_company_period(&mut *tx, company_id, doc.period)
            .await?
        {
            if period.status == "filed" {
                return Err(TaxComplianceError::PeriodAlreadyFiled(
                    company_id, doc.period,
                ));
            }
        }
        docs.void_on(&mut *tx, efaktur_id).await?;
        let voided = docs
            .find_on(&mut *tx, efaktur_id)
            .await?
            .ok_or(TaxComplianceError::EFakturNotFound(efaktur_id))?;
        tx.commit().await?;
        Ok(voided)
    }

    /// Confirm one e-Faktur document (assigned → confirmed: the operator / downstream-system
    /// acknowledgement). Idempotent — confirming an already-confirmed document returns it.
    /// Refuses on a voided document.
    pub async fn confirm_efaktur(
        &self,
        company_id: Uuid,
        efaktur_id: Uuid,
    ) -> Result<EFakturDocumentRow, TaxComplianceError> {
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let docs = EFakturDocumentRepository::new(self.db_pool.clone());
        let flipped = docs.confirm_on(&mut *tx, efaktur_id).await?;
        if flipped == 0 {
            // Not in `assigned` — read back to distinguish an idempotent re-confirm from a refusal.
            let doc = docs
                .find_on(&mut *tx, efaktur_id)
                .await?
                .ok_or(TaxComplianceError::EFakturNotFound(efaktur_id))?;
            if doc.status != "confirmed" {
                return Err(TaxComplianceError::EFakturNotConfirmable(
                    efaktur_id, doc.status,
                ));
            }
            tx.commit().await?;
            return Ok(doc);
        }
        let confirmed = docs
            .find_on(&mut *tx, efaktur_id)
            .await?
            .ok_or(TaxComplianceError::EFakturNotFound(efaktur_id))?;
        tx.commit().await?;
        Ok(confirmed)
    }

    /// Finalize a masa pajak: open → finalized, writing the aggregate VAT totals (Σ output /
    /// input / withholding over the month's tax transactions) into the period row in the same
    /// statement. A finalized period refuses new tax transactions and new numbers (see
    /// [`Self::record_tax_transaction`]). Idempotent — re-finalizing a finalized period returns
    /// its row unchanged. Refuses on a filed period (terminal).
    pub async fn finalize_period(
        &self,
        company_id: Uuid,
        period: NaiveDate,
    ) -> Result<FilingPeriodRow, TaxComplianceError> {
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let periods = TaxFilingPeriodRepository::new(self.db_pool.clone());
        // An empty month (no period row yet) is finalizable: open the row first, idempotently.
        periods
            .ensure_open(&mut *tx, Uuid::new_v4(), company_id, period)
            .await?;
        let row = match periods.finalize_open(&mut *tx, company_id, period).await? {
            Some(r) => r,
            None => match periods
                .find_by_company_period(&mut *tx, company_id, period)
                .await?
            {
                // The CAS missed while the row exists and is finalized ⇒ an earlier finalize
                // already landed; replay is a committed no-op.
                Some(r) if r.status == "finalized" => r,
                Some(r) if r.status == "filed" => {
                    return Err(TaxComplianceError::PeriodAlreadyFiled(company_id, period))
                }
                // open would have matched the CAS; a missing row here means the period was
                // soft-deleted between ensure_open and the CAS — treat as absent.
                _ => return Err(TaxComplianceError::NoFilingPeriod(company_id, period)),
            },
        };
        tx.commit().await?;
        Ok(row)
    }

    /// File a masa pajak: finalized → filed, stamping `filed_at` into the audit metadata. Filed is
    /// terminal. Idempotent — re-filing a filed period returns its row unchanged. Refuses
    /// [`TaxComplianceError::PeriodNotFinalized`] when the period is still open.
    pub async fn file_period(
        &self,
        company_id: Uuid,
        period: NaiveDate,
    ) -> Result<FilingPeriodRow, TaxComplianceError> {
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let periods = TaxFilingPeriodRepository::new(self.db_pool.clone());
        let row = match periods.file_finalized(&mut *tx, company_id, period).await? {
            Some(r) => r,
            None => match periods
                .find_by_company_period(&mut *tx, company_id, period)
                .await?
            {
                Some(r) if r.status == "filed" => r,
                Some(r) if r.status == "open" => {
                    return Err(TaxComplianceError::PeriodNotFinalized(company_id, period))
                }
                _ => return Err(TaxComplianceError::NoFilingPeriod(company_id, period)),
            },
        };
        tx.commit().await?;
        Ok(row)
    }

    /// List a company's masa pajak rows oldest-first (the operator's SPT overview read). Rides the
    /// caller's company scope.
    pub async fn list_filing_periods(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<FilingPeriodRow>, TaxComplianceError> {
        let periods = TaxFilingPeriodRepository::new(self.db_pool.clone());
        company_scope::with_company_scope(
            Some(company_id),
            periods.list_for_company(&self.db_pool, company_id),
        )
        .await
        .map_err(Into::into)
    }

    /// List a company's e-Faktur documents for one masa pajak (sequence order). `status` filters
    /// by the enum text when given. Rides the caller's company scope.
    pub async fn list_period_documents(
        &self,
        company_id: Uuid,
        period: NaiveDate,
        status: Option<&str>,
    ) -> Result<Vec<EFakturDocumentRow>, TaxComplianceError> {
        let docs = EFakturDocumentRepository::new(self.db_pool.clone());
        company_scope::with_company_scope(
            Some(company_id),
            docs.list_for_period(&self.db_pool, company_id, period, status),
        )
        .await
        .map_err(Into::into)
    }

    /// The DJP export projection: every live document of the masa pajak joined to its tax
    /// transaction (invoice ref, posting date, totals), sequence order. The composing host joins
    /// buyer identity + per-line detail from billing on top. Rides the caller's company scope.
    pub async fn export_rows(
        &self,
        company_id: Uuid,
        period: NaiveDate,
    ) -> Result<Vec<EFakturExportRow>, TaxComplianceError> {
        let docs = EFakturDocumentRepository::new(self.db_pool.clone());
        company_scope::with_company_scope(
            Some(company_id),
            docs.list_export_rows(&self.db_pool, company_id, period),
        )
        .await
        .map_err(Into::into)
    }

    /// Allocate a gapless e-Faktur number (010.NNN-NN.YYYYYYYY) via the TaxFilingPeriod sequence.
    /// Concurrent calls serialize on the per-period row (the UPDATE ... RETURNING is atomic). The
    /// allocator only matches an OPEN period; a closed masa pajak refuses with
    /// [`TaxComplianceError::PeriodNotOpen`] — never a gap, never a number out of a closed range.
    async fn assign_efaktur_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        txn_id: Uuid,
        company_id: Uuid,
        posting_date: NaiveDate,
    ) -> Result<Uuid, TaxComplianceError> {
        let period_start = month_start(posting_date);

        // Ensure a TaxFilingPeriod exists for this month (auto-open if missing). An existing
        // finalized/filed row is left untouched by the ON CONFLICT DO NOTHING — the guarded
        // allocate below then refuses, which is exactly the fail-closed behavior.
        let periods = TaxFilingPeriodRepository::new(self.db_pool.clone());
        periods
            .ensure_open(&mut **tx, Uuid::new_v4(), company_id, period_start)
            .await?;

        // Atomically allocate the next sequence (gapless — serializes on the row lock).
        let AllocatedSequence { seq, seg } = periods
            .allocate_sequence(&mut **tx, company_id, period_start)
            .await?
            .ok_or(TaxComplianceError::PeriodNotOpen(company_id, period_start))?;
        let month = posting_date.format("%m").to_string();
        let number = format!("010.{}-{}.{:08}", seg, month, seq);

        // Insert the EFakturDocument.
        let eid = Uuid::new_v4();
        let docs = EFakturDocumentRepository::new(self.db_pool.clone());
        docs.insert(
            &mut **tx,
            &NewEFakturDocumentRow {
                id: eid,
                company_id,
                tax_transaction_id: txn_id,
                number: &number,
                taxpayer_segment: &seg,
                period: period_start,
                sequence: seq,
                assignment_date: posting_date,
            },
        )
        .await?;

        Ok(eid)
    }
}

/// The first day of the posting date's month (the masa pajak key).
fn month_start(d: NaiveDate) -> NaiveDate {
    format!("{:04}-{:02}-01", d.year(), d.month())
        .parse::<NaiveDate>()
        .expect("a valid date reformats to a valid month start")
}

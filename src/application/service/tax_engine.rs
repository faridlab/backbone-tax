//! Tax engine — hand-authored (user-owned). Region-neutral: it computes tax LINES from a template
//! applied to a taxable base, and resolves withholding. It never posts to the GL — the caller
//! attaches the returned lines to an `AccountingPost`. Indonesia rates/rules are seeded data
//! (deferred); this is the transcribed engine that consumes them. See docs/erp/tax-compliance.md.
//!
//! Tenant-scoped read path (ADR-0010 Decision B1): every SELECT runs through
//! `company_scope::{fetch_all_scoped, fetch_optional_scoped, fetch_optional_scalar_scoped}` so the
//! ADR-0008 RLS fence on `tax.tax_*` sees `app.company_id` and returns the caller's rows. A missed
//! scope fails loud as `NoCompanyScope` (not a misleading `NoEffectiveRate`); a correct scope with
//! no effective row still returns `NoEffectiveRate`/`CategoryNotFound` as before.

use backbone_orm::company_scope;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy;
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

use super::tax_rounding::{distribute_delta_smoothly, round2, RoundingMethod};

#[derive(Debug)]
pub enum TaxError {
    TemplateNotFound(Uuid),
    CategoryNotFound(Uuid),
    NoEffectiveRate(Uuid), // template with no row effective on the date
    InvalidDateRange,
    NegativeBase,
    DuplicateCode(String),
    /// A live template of the same type already carries this display name in the company.
    DuplicateName(String),
    /// A caller-supplied enum label or pairing is not one of the accepted values.
    InvalidValue(String),
    /// A row/category whose effective window overlaps an existing sibling (council 2026-07-03).
    OverlappingWindow(String),
    /// An inclusive template contains a non-`on_net_total` row — the grossing-up basis is undefined.
    InclusiveUnsupported,
    /// A template's repartition family is structurally invalid (no tax line, or factors not
    /// summing to 100.00) — routing the split would silently misstate the journal.
    RepartitionInvalid(Uuid),
    /// A cash-basis posture names a transition account that is missing from `accounting.accounts`
    /// or is not reconcilable — the payment-time flip pairs against that account, so a
    /// non-reconcilable one dead-ends the deferral. Raised by the write path (the engine never
    /// reads across schemas).
    CabaTransitionNotReconcilable(Uuid),
    /// A read/compute path needed the caller's company but the request scope was unset
    /// (missing `with_company_scope` / `with_request_scope` middleware). Distinct from
    /// `NoEffectiveRate`/`CategoryNotFound` so operators can tell a missed scope from a genuine
    /// "no row applies on this date" (ADR-0010 B1).
    NoCompanyScope,
    Db(sqlx::Error),
}
impl TaxError {
    pub fn code(&self) -> &'static str {
        match self {
            TaxError::TemplateNotFound(_) => "template_not_found",
            TaxError::CategoryNotFound(_) => "category_not_found",
            TaxError::NoEffectiveRate(_) => "no_effective_rate",
            TaxError::InvalidDateRange => "invalid_date_range",
            TaxError::NegativeBase => "negative_base",
            TaxError::DuplicateCode(_) => "duplicate_code",
            TaxError::DuplicateName(_) => "duplicate_name",
            TaxError::InvalidValue(_) => "invalid_value",
            TaxError::OverlappingWindow(_) => "overlapping_effective_window",
            TaxError::InclusiveUnsupported => "inclusive_cumulative_unsupported",
            TaxError::RepartitionInvalid(_) => "repartition_invalid",
            TaxError::CabaTransitionNotReconcilable(_) => "caba_transition_not_reconcilable",
            TaxError::NoCompanyScope => "no_company_scope",
            TaxError::Db(_) => "internal_error",
        }
    }
    pub fn http_status(&self) -> u16 {
        match self {
            TaxError::Db(_) => 500,
            TaxError::NoCompanyScope => 401,
            _ => 422,
        }
    }
}
impl std::fmt::Display for TaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code())?;
        match self {
            TaxError::TemplateNotFound(id)
            | TaxError::CategoryNotFound(id)
            | TaxError::NoEffectiveRate(id)
            | TaxError::RepartitionInvalid(id)
            | TaxError::CabaTransitionNotReconcilable(id) => write!(f, ": {id}"),
            TaxError::DuplicateCode(v) | TaxError::OverlappingWindow(v) => write!(f, ": {v}"),
            TaxError::DuplicateName(v) | TaxError::InvalidValue(v) => write!(f, ": {v}"),
            _ => Ok(()),
        }
    }
}
impl std::error::Error for TaxError {}
impl From<sqlx::Error> for TaxError {
    fn from(e: sqlx::Error) -> Self {
        TaxError::Db(e)
    }
}

/// A computed tax line — the engine's output. The caller maps it to an AccountingPost line.
#[derive(Debug, Clone, PartialEq)]
pub struct TaxLine {
    pub account_id: Option<Uuid>,
    pub rate: Decimal,
    pub tax_amount: Decimal,
    pub is_withholding: bool,
    pub description: Option<String>,
}

/// Money is exact IDR: 2 decimals, round-half-up.
fn money(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

#[derive(Clone)]
pub struct TaxEngine {
    db_pool: PgPool,
}

impl TaxEngine {
    pub fn new(db_pool: PgPool) -> Self {
        Self { db_pool }
    }

    /// Compute the tax lines for `template_id` applied to `base_amount` on `on_date`.
    ///
    /// - `on_net_total`: rate% of the net base.
    /// - `on_previous_row_total`: rate% of (net + sum of prior charge amounts) — cumulative.
    /// - `actual`: `rate` is a fixed amount, not a percentage.
    /// If the template `is_inclusive`, the base is treated as already containing the tax and the
    /// tax is extracted (base is the tax-inclusive gross). Withholding rows produce negative lines.
    ///
    /// **Tenant-scoped read path (ADR-0010 B1).** Both the template lookup and the row fetch run
    /// through the scoped execute helpers so the RLS fence sees `app.company_id`. The caller's
    /// company is read from the ambient request scope (`with_company_scope` /
    /// `with_request_scope`); if no scope is set the engine fails loud as `NoCompanyScope`
    /// instead of the misleading `NoEffectiveRate`.
    pub async fn calculate(
        &self,
        template_id: Uuid,
        base_amount: Decimal,
        on_date: NaiveDate,
    ) -> Result<Vec<TaxLine>, TaxError> {
        if base_amount < Decimal::ZERO {
            return Err(TaxError::NegativeBase);
        }
        if company_scope::current_company().is_none() {
            // Fail loud on a missed scope rather than returning NoEffectiveRate from the fenced
            // SELECT (which would be indistinguishable from a genuine "no row applies").
            return Err(TaxError::NoCompanyScope);
        }

        let inclusive: Option<bool> = company_scope::fetch_optional_scalar_scoped(
            &self.db_pool,
            sqlx::query_scalar(
                "SELECT is_inclusive FROM tax.tax_templates \
                 WHERE id=$1 AND (metadata->>'deleted_at') IS NULL",
            )
            .bind(template_id),
        )
        .await?;
        let inclusive = inclusive.ok_or(TaxError::TemplateNotFound(template_id))?;

        // Exactly ONE row per sort_order — the newest-effective whose window contains the date.
        // `DISTINCT ON (sort_order)` makes overlapping effective windows deterministic (never a
        // double-charge on the read path); `add_row` also rejects overlaps at write time and an
        // EXCLUDE constraint forbids them in the DB (council 2026-07-03).
        let rows: Vec<(i32, String, Decimal, Option<Uuid>, bool, Option<String>)> =
            company_scope::fetch_all_scoped(
                &self.db_pool,
                sqlx::query_as(
                    r#"SELECT DISTINCT ON (sort_order)
                           sort_order, charge_type::text, rate, account_id, is_withholding, description
                       FROM tax.tax_template_rows
                       WHERE template_id=$1
                         AND (metadata->>'deleted_at') IS NULL
                         AND effective_from <= $2
                         AND (effective_to IS NULL OR effective_to >= $2)
                       ORDER BY sort_order, effective_from DESC"#,
                )
                .bind(template_id)
                .bind(on_date),
            )
            .await?;
        if rows.is_empty() {
            return Err(TaxError::NoEffectiveRate(template_id));
        }

        let hundred = Decimal::from(100);

        if inclusive {
            // Inclusive templates only support `on_net_total` charge rows — the grossing-up basis
            // is otherwise undefined. Reject cumulative/`actual` non-withholding rows.
            if rows
                .iter()
                .any(|(_, ct, _, _, wh, _)| !*wh && ct != "on_net_total")
            {
                return Err(TaxError::InclusiveUnsupported);
            }
            let pct_sum: Decimal = rows
                .iter()
                .filter(|(_, ct, _, _, wh, _)| ct == "on_net_total" && !*wh)
                .map(|(_, _, r, _, _, _)| *r)
                .sum();
            let net = if pct_sum > Decimal::ZERO {
                money(base_amount / (Decimal::ONE + pct_sum / hundred))
            } else {
                base_amount
            };
            // The on_net tax lines must sum EXACTLY to (gross - net) — the last one absorbs the
            // rounding residual so `Σ lines == gross` for the caller's balanced AccountingPost.
            let total_tax = base_amount - net;
            let last_on_net = rows
                .iter()
                .rposition(|(_, ct, _, _, wh, _)| ct == "on_net_total" && !*wh);
            let mut on_net_acc = Decimal::ZERO;
            let mut lines = Vec::with_capacity(rows.len());
            for (i, (_, _ct, rate, account_id, is_withholding, description)) in
                rows.iter().enumerate()
            {
                let amount = if *is_withholding {
                    money(net * rate / hundred)
                } else if Some(i) == last_on_net {
                    total_tax - on_net_acc
                } else {
                    let a = money(net * rate / hundred);
                    on_net_acc += a;
                    a
                };
                let signed = if *is_withholding { -amount } else { amount };
                lines.push(TaxLine {
                    account_id: *account_id,
                    rate: *rate,
                    tax_amount: signed,
                    is_withholding: *is_withholding,
                    description: description.clone(),
                });
            }
            return Ok(lines);
        }

        // Exclusive: rate applies to net (or, for cumulative rows, net + prior charge amounts).
        let net = base_amount;
        let mut running = net;
        let mut lines = Vec::with_capacity(rows.len());
        for (_, charge_type, rate, account_id, is_withholding, description) in rows {
            let amount = match charge_type.as_str() {
                "actual" => rate,
                "on_previous_row_total" => money(running * rate / hundred),
                _ => money(net * rate / hundred), // on_net_total
            };
            let signed = if is_withholding { -amount } else { amount };
            running += amount;
            lines.push(TaxLine {
                account_id,
                rate,
                tax_amount: signed,
                is_withholding,
                description,
            });
        }
        Ok(lines)
    }

    /// Resolve a withholding line for `category_id` on `base_amount` — `None` if under threshold.
    ///
    /// **Tenant-scoped read path (ADR-0010 B1).** Same fence/scope rules as `calculate`.
    pub async fn resolve_withholding(
        &self,
        category_id: Uuid,
        base_amount: Decimal,
        on_date: NaiveDate,
    ) -> Result<Option<TaxLine>, TaxError> {
        if base_amount < Decimal::ZERO {
            return Err(TaxError::NegativeBase);
        }
        if company_scope::current_company().is_none() {
            return Err(TaxError::NoCompanyScope);
        }
        let row: Option<(Decimal, Decimal, Option<Uuid>, Option<String>)> =
            company_scope::fetch_optional_scoped(
                &self.db_pool,
                sqlx::query_as(
                    r#"SELECT rate, threshold_amount, account_id, name
                       FROM tax.withholding_categories
                       WHERE id=$1 AND (metadata->>'deleted_at') IS NULL
                         AND effective_from <= $2 AND (effective_to IS NULL OR effective_to >= $2)
                       ORDER BY effective_from DESC LIMIT 1"#,
                )
                .bind(category_id)
                .bind(on_date),
            )
            .await?;
        let (rate, threshold, account_id, name) =
            row.ok_or(TaxError::CategoryNotFound(category_id))?;
        if base_amount < threshold {
            return Ok(None);
        }
        let amount = money(base_amount * rate / Decimal::from(100));
        Ok(Some(TaxLine {
            account_id,
            rate,
            tax_amount: -amount, // withholding is a deduction
            is_withholding: true,
            description: name,
        }))
    }
}

// ---------------------------------------------------------------------------
// Document-grade calculation: per-company rounding policy, repartition
// routing, and cash-basis (deferred) exigibility.
// ---------------------------------------------------------------------------

/// The kind of document a computation is for. Selects the repartition family
/// (invoice vs refund routing). Amounts use the same sign convention for both
/// kinds; callers negate wholesale when posting credit notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentType {
    Invoice,
    Refund,
}

impl DocumentType {
    /// Parse the `repartition_document_type` DB label.
    pub fn from_db(label: &str) -> Option<Self> {
        match label {
            "invoice" => Some(DocumentType::Invoice),
            "refund" => Some(DocumentType::Refund),
            _ => None,
        }
    }

    /// The `repartition_document_type` DB label for this kind.
    pub fn as_db(self) -> &'static str {
        match self {
            DocumentType::Invoice => "invoice",
            DocumentType::Refund => "refund",
        }
    }
}

/// One taxable document line: a tax template applied to `quantity × unit_price`.
/// The raw product is NEVER rounded before the policy runs.
#[derive(Debug, Clone)]
pub struct DocumentTaxRequestLine {
    pub template_id: Uuid,
    pub quantity: Decimal,
    pub unit_price: Decimal,
}

#[derive(Debug, Clone)]
pub struct DocumentTaxRequest {
    pub company_id: Uuid,
    pub document_type: DocumentType,
    pub on_date: NaiveDate,
    pub lines: Vec<DocumentTaxRequestLine>,
}

/// One routed tax split of the document. `source_index` ties it back to the
/// input line that generated it.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentTaxLine {
    pub source_index: usize,
    pub template_id: Uuid,
    /// Posting account — the cash-basis transition account when the template
    /// defers (see `real_account_id`).
    pub account_id: Option<Uuid>,
    /// `Some(real)` iff the amount is deferred (cash-basis template): it posts
    /// to the transition account now and flips to `real` as payment reconciles.
    pub real_account_id: Option<Uuid>,
    pub rate: Decimal,
    /// Signed: withholding components are negative (a deduction).
    pub tax_amount: Decimal,
    pub is_withholding: bool,
    pub description: Option<String>,
    pub tag_ids: Vec<Uuid>,
    /// The repartition line whose factor split produced this amount
    /// (`None` for legacy templates routing to the template row's account).
    pub repartition_line_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentTaxResult {
    /// Per input line: the net (tax-excluded) base AFTER the rounding policy.
    /// Under `round_globally` this is the redistributed base and OVERWRITES the
    /// caller's own per-line money() total — the caller's journal balances
    /// against these nets plus the returned tax lines, not against raw products.
    pub net_amounts: Vec<Decimal>,
    pub lines: Vec<DocumentTaxLine>,
    /// Σ net_amounts — the document's tax-excluded total.
    pub excluded_total: Decimal,
    /// Σ signed tax lines.
    pub tax_total: Decimal,
    /// excluded_total + tax_total (for inclusive documents, the AR/AP amount).
    pub included_total: Decimal,
    /// The policy actually applied (from `tax.company_tax_settings`; absent row
    /// ⇒ round_globally).
    pub method: RoundingMethod,
    /// Reporting tags from the repartition base lines. Suppressed while a
    /// template defers to its transition account — a deferred base has not
    /// realized yet.
    pub base_tags: Vec<Uuid>,
}

/// Template posture needed for document computation. `tax_exigibility` is
/// materialized on the row at create time (a later company posture change never
/// rewrites existing templates).
struct TemplateMeta {
    is_inclusive: bool,
    exigibility: String,
    transition_account_id: Option<Uuid>,
}

/// One unrounded tax component of a template applied to one line's gross —
/// the raw amount only; the component's rate/account/metadata come from the
/// effective row it is aligned with (`fetch_effective_rows` order).
/// Unsigned (the withholding sign is applied when lines emit).
struct RawComponent {
    raw_amount: Decimal,
}

/// The unrounded per-line computation for one template.
struct LineRaw {
    /// Unrounded net base (gross for exclusive templates; gross/(1+Σrates) for inclusive).
    raw_net: Decimal,
    /// Components aligned with the template's effective rows (by row order).
    components: Vec<RawComponent>,
}

/// One tax-side repartition line (factor split).
struct TaxSplit {
    id: Uuid,
    factor_percent: Decimal,
    account_id: Option<Uuid>,
    tag_ids: Vec<Uuid>,
    description: Option<String>,
}

/// The resolved repartition family for a template + document type.
struct ResolvedRepartition {
    base_tags: Vec<Uuid>,
    tax_splits: Vec<TaxSplit>,
}

type EffectiveRow = (i32, String, Decimal, Option<Uuid>, bool, Option<String>);

/// Apply the effective rows to one line's gross — the shared raw math for both
/// rounding policies. Pure: no DB, no rounding (except Decimal division precision).
fn raw_from_rows(
    rows: &[EffectiveRow],
    gross: Decimal,
    is_inclusive: bool,
) -> Result<LineRaw, TaxError> {
    let hundred = Decimal::from(100);
    if is_inclusive {
        // Inclusive templates only support `on_net_total` charge rows — the grossing-up
        // basis is otherwise undefined (same rule as the per-line `calculate`).
        if rows
            .iter()
            .any(|(_, ct, _, _, wh, _)| !*wh && ct != "on_net_total")
        {
            return Err(TaxError::InclusiveUnsupported);
        }
        let pct_sum: Decimal = rows
            .iter()
            .filter(|(_, ct, _, _, wh, _)| ct == "on_net_total" && !*wh)
            .map(|(_, _, r, _, _, _)| *r)
            .sum();
        let raw_net = if pct_sum > Decimal::ZERO {
            gross / (Decimal::ONE + pct_sum / hundred)
        } else {
            gross
        };
        let components = rows
            .iter()
            .map(|(_, _, rate, _, _, _)| RawComponent {
                raw_amount: raw_net * *rate / hundred,
            })
            .collect();
        Ok(LineRaw {
            raw_net,
            components,
        })
    } else {
        let mut running = gross;
        let mut components = Vec::with_capacity(rows.len());
        for (_, charge_type, rate, _, _, _) in rows {
            let raw = match charge_type.as_str() {
                "actual" => *rate,
                "on_previous_row_total" => running * *rate / hundred,
                _ => gross * *rate / hundred, // on_net_total
            };
            running += raw;
            components.push(RawComponent { raw_amount: raw });
        }
        Ok(LineRaw {
            raw_net: gross,
            components,
        })
    }
}

/// Round a set of raw per-line amounts to their rounded per-line values plus the
/// smooth-delta redistribution of the aggregate residual — the heart of
/// `round_globally`. Σ outputs == round2(Σ raws) exactly.
fn redistribute_rounded(raws: &[Decimal]) -> Vec<Decimal> {
    let per: Vec<Decimal> = raws.iter().map(|r| round2(*r)).collect();
    let target = round2(raws.iter().copied().sum());
    let delta = target - per.iter().copied().sum::<Decimal>();
    if delta == Decimal::ZERO {
        return per;
    }
    let extra = distribute_delta_smoothly(raws, delta);
    per.iter().zip(extra).map(|(a, b)| *a + b).collect()
}

impl TaxEngine {
    /// The company's rounding policy (`tax.company_tax_settings`; absent row ⇒
    /// the safe default `round_globally`). Fails loud on DB errors — a missing
    /// table means un-migrated, not "no policy".
    async fn company_rounding_method(&self, company_id: Uuid) -> Result<RoundingMethod, TaxError> {
        let label: Option<String> = company_scope::fetch_optional_scalar_scoped(
            &self.db_pool,
            sqlx::query_scalar(
                "SELECT rounding_method::text FROM tax.company_tax_settings \
                 WHERE company_id=$1 AND (metadata->>'deleted_at') IS NULL",
            )
            .bind(company_id),
        )
        .await?;
        Ok(label
            .as_deref()
            .and_then(RoundingMethod::from_db)
            .unwrap_or(RoundingMethod::RoundGlobally))
    }

    async fn template_meta(&self, template_id: Uuid) -> Result<TemplateMeta, TaxError> {
        let row: Option<(bool, String, Option<Uuid>)> = company_scope::fetch_optional_scoped(
            &self.db_pool,
            sqlx::query_as(
                "SELECT is_inclusive, tax_exigibility::text, cash_basis_transition_account_id \
                 FROM tax.tax_templates \
                 WHERE id=$1 AND (metadata->>'deleted_at') IS NULL",
            )
            .bind(template_id),
        )
        .await?;
        let (is_inclusive, exigibility, transition_account_id) =
            row.ok_or(TaxError::TemplateNotFound(template_id))?;
        Ok(TemplateMeta {
            is_inclusive,
            exigibility,
            transition_account_id,
        })
    }

    /// The template's effective rows on `on_date` (the per-line `calculate`'s
    /// fetch, factored out for the document path).
    async fn fetch_effective_rows(
        &self,
        template_id: Uuid,
        on_date: NaiveDate,
    ) -> Result<Vec<EffectiveRow>, TaxError> {
        let rows: Vec<EffectiveRow> = company_scope::fetch_all_scoped(
            &self.db_pool,
            sqlx::query_as(
                r#"SELECT DISTINCT ON (sort_order)
                       sort_order, charge_type::text, rate, account_id, is_withholding, description
                   FROM tax.tax_template_rows
                   WHERE template_id=$1
                     AND (metadata->>'deleted_at') IS NULL
                     AND effective_from <= $2
                     AND (effective_to IS NULL OR effective_to >= $2)
                   ORDER BY sort_order, effective_from DESC"#,
            )
            .bind(template_id)
            .bind(on_date),
        )
        .await?;
        Ok(rows)
    }

    /// Resolve the repartition family for `document_type`. `None` ⇒ the template
    /// predates repartition: callers fall back to routing 100% of each component
    /// to the template row's own `account_id`.
    async fn repartition_for(
        &self,
        template_id: Uuid,
        document_type: DocumentType,
    ) -> Result<Option<ResolvedRepartition>, TaxError> {
        let rows: Vec<(Uuid, String, Decimal, Option<Uuid>, Vec<Uuid>, Option<String>)> =
            company_scope::fetch_all_scoped(
                &self.db_pool,
                sqlx::query_as(
                    r#"SELECT id, repartition_type::text, factor_percent, account_id, tag_ids, description
                       FROM tax.tax_repartition_lines
                       WHERE template_id=$1 AND document_type::text=$2
                         AND (metadata->>'deleted_at') IS NULL
                       ORDER BY sort_order"#,
                )
                .bind(template_id)
                .bind(document_type.as_db()),
            )
            .await?;
        if rows.is_empty() {
            return Ok(None); // legacy template: no repartition rows at all
        }
        let mut base_tags = Vec::new();
        let mut tax_splits = Vec::new();
        for (id, rt, factor_percent, account_id, tag_ids, description) in rows {
            if rt == "base" {
                base_tags = tag_ids;
            } else {
                tax_splits.push(TaxSplit {
                    id,
                    factor_percent,
                    account_id,
                    tag_ids,
                    description,
                });
            }
        }
        let factor_sum: Decimal = tax_splits.iter().map(|s| s.factor_percent).sum();
        if tax_splits.is_empty() || round2(factor_sum) != Decimal::from(100) {
            return Err(TaxError::RepartitionInvalid(template_id));
        }
        Ok(Some(ResolvedRepartition {
            base_tags,
            tax_splits,
        }))
    }

    /// Compute the tax lines for a whole document under the company's rounding
    /// policy, with repartition routing and cash-basis deferral resolution.
    ///
    /// - Bases are the raw `quantity × unit_price` products — never rounded
    ///   before the policy runs.
    /// - `round_per_line`: every line's nets and component amounts round to
    ///   cents independently (the legacy per-line behavior, document-shaped).
    /// - `round_globally`: per template, per component: the SUM is rounded once
    ///   and the per-line residual redistributed in cents (smooth-delta), so
    ///   `Σ per-line == round2(Σ raw)` exactly and no line absorbs the residual.
    ///   The redistributed nets go into `net_amounts` — callers MUST use them as
    ///   the document's line nets (overwriting their own per-line money() totals)
    ///   or the journal mis-balances.
    /// - Repartition splits inside a component always use smooth-delta over the
    ///   factor weights (in BOTH policies) — the splits must sum to the component
    ///   exactly or the journal breaks.
    /// - A template materialized `on_payment` (cash basis) posts every split to
    ///   its transition account and carries the real routing account in
    ///   `real_account_id`; the flip to the real account is the reconciliation
    ///   seam's job, not the engine's.
    ///
    /// **Tenant-scoped read path (ADR-0010 B1)** — same fence/scope rules as
    /// `calculate`.
    pub async fn calculate_document(
        &self,
        req: &DocumentTaxRequest,
    ) -> Result<DocumentTaxResult, TaxError> {
        if company_scope::current_company().is_none() {
            return Err(TaxError::NoCompanyScope);
        }
        for l in &req.lines {
            if l.quantity < Decimal::ZERO || l.unit_price < Decimal::ZERO {
                return Err(TaxError::NegativeBase);
            }
        }
        let method = self.company_rounding_method(req.company_id).await?;

        // Group input lines by template (first-appearance order preserved) so
        // the global rounding aggregates per template across its lines.
        let mut group_order: Vec<Uuid> = Vec::new();
        let mut groups: HashMap<Uuid, Vec<usize>> = HashMap::new();
        for (i, l) in req.lines.iter().enumerate() {
            if !groups.contains_key(&l.template_id) {
                group_order.push(l.template_id);
            }
            groups.entry(l.template_id).or_default().push(i);
        }

        let mut net_amounts: Vec<Decimal> = vec![Decimal::ZERO; req.lines.len()];
        let mut out_lines: Vec<DocumentTaxLine> = Vec::new();
        let mut base_tags: Vec<Uuid> = Vec::new();

        for tid in group_order {
            let idxs = &groups[&tid];
            let meta = self.template_meta(tid).await?;
            let rows = self.fetch_effective_rows(tid, req.on_date).await?;
            if rows.is_empty() {
                return Err(TaxError::NoEffectiveRate(tid));
            }
            let repartition = self.repartition_for(tid, req.document_type).await?;
            let deferred = meta.exigibility == "on_payment";

            let raws: Vec<LineRaw> = idxs
                .iter()
                .map(|&i| {
                    raw_from_rows(
                        &rows,
                        req.lines[i].quantity * req.lines[i].unit_price,
                        meta.is_inclusive,
                    )
                })
                .collect::<Result<_, _>>()?;

            // Per-line nets under the policy.
            let nets: Vec<Decimal> = match method {
                RoundingMethod::RoundPerLine => raws.iter().map(|r| round2(r.raw_net)).collect(),
                RoundingMethod::RoundGlobally => {
                    let raw_nets: Vec<Decimal> = raws.iter().map(|r| r.raw_net).collect();
                    redistribute_rounded(&raw_nets)
                }
            };
            for (&line_idx, net) in idxs.iter().zip(&nets) {
                net_amounts[line_idx] = *net;
            }

            // Per component (aligned with `rows` order), per line.
            for (ci, row) in rows.iter().enumerate() {
                let (_, _, rate, row_account, is_wh, row_desc) = row;
                let raws_c: Vec<Decimal> =
                    raws.iter().map(|r| r.components[ci].raw_amount).collect();
                let amounts: Vec<Decimal> = match method {
                    RoundingMethod::RoundPerLine => raws_c.iter().map(|a| round2(*a)).collect(),
                    RoundingMethod::RoundGlobally => redistribute_rounded(&raws_c),
                };

                for (&line_idx, amt) in idxs.iter().zip(&amounts) {
                    if *amt == Decimal::ZERO {
                        continue;
                    }
                    match &repartition {
                        None => {
                            // Legacy template: route 100% to the row account.
                            out_lines.push(DocumentTaxLine {
                                source_index: line_idx,
                                template_id: tid,
                                account_id: if deferred {
                                    meta.transition_account_id
                                } else {
                                    *row_account
                                },
                                real_account_id: if deferred { *row_account } else { None },
                                rate: *rate,
                                tax_amount: if *is_wh { -*amt } else { *amt },
                                is_withholding: *is_wh,
                                description: row_desc.clone(),
                                tag_ids: Vec::new(),
                                repartition_line_id: None,
                            });
                        }
                        Some(rep) => {
                            let weights: Vec<Decimal> =
                                rep.tax_splits.iter().map(|s| s.factor_percent).collect();
                            let parts = distribute_delta_smoothly(&weights, *amt);
                            for (split, part) in rep.tax_splits.iter().zip(parts) {
                                if part == Decimal::ZERO {
                                    continue;
                                }
                                // A tax split with no account of its own falls back to the
                                // aligned row's account — the same routing a pre-repartition
                                // template used. Service-created templates seed their family
                                // with a NULL-account split (rows don't exist yet at seed
                                // time), so without this they would emit unroutable lines.
                                let resolved = split.account_id.or(*row_account);
                                out_lines.push(DocumentTaxLine {
                                    source_index: line_idx,
                                    template_id: tid,
                                    account_id: if deferred {
                                        meta.transition_account_id
                                    } else {
                                        resolved
                                    },
                                    real_account_id: if deferred { resolved } else { None },
                                    rate: *rate,
                                    tax_amount: if *is_wh { -part } else { part },
                                    is_withholding: *is_wh,
                                    description: split
                                        .description
                                        .clone()
                                        .or_else(|| row_desc.clone()),
                                    tag_ids: split.tag_ids.clone(),
                                    repartition_line_id: Some(split.id),
                                });
                            }
                        }
                    }
                }
            }

            if !deferred {
                if let Some(rep) = &repartition {
                    for t in &rep.base_tags {
                        if !base_tags.contains(t) {
                            base_tags.push(*t);
                        }
                    }
                }
            }
        }

        let excluded_total: Decimal = net_amounts.iter().copied().sum();
        let tax_total: Decimal = out_lines.iter().map(|l| l.tax_amount).sum();
        Ok(DocumentTaxResult {
            net_amounts,
            lines: out_lines,
            excluded_total,
            tax_total,
            included_total: excluded_total + tax_total,
            method,
            base_tags,
        })
    }
}

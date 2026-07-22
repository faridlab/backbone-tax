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
use uuid::Uuid;

#[derive(Debug)]
pub enum TaxError {
    TemplateNotFound(Uuid),
    CategoryNotFound(Uuid),
    NoEffectiveRate(Uuid), // template with no row effective on the date
    InvalidDateRange,
    NegativeBase,
    DuplicateCode(String),
    /// A row/category whose effective window overlaps an existing sibling (council 2026-07-03).
    OverlappingWindow(String),
    /// An inclusive template contains a non-`on_net_total` row — the grossing-up basis is undefined.
    InclusiveUnsupported,
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
            TaxError::OverlappingWindow(_) => "overlapping_effective_window",
            TaxError::InclusiveUnsupported => "inclusive_cumulative_unsupported",
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
            | TaxError::NoEffectiveRate(id) => write!(f, ": {id}"),
            TaxError::DuplicateCode(v) | TaxError::OverlappingWindow(v) => write!(f, ": {v}"),
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
            if rows.iter().any(|(_, ct, _, _, wh, _)| !*wh && ct != "on_net_total") {
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
            for (i, (_, _ct, rate, account_id, is_withholding, description)) in rows.iter().enumerate() {
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

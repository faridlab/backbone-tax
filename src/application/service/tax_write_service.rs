//! Validated write path for tax config — hand-authored (user-owned). Closes the CRUD-bypass:
//! templates/categories are config master data; here creates are validated (unique code, template
//! existence for a row, sane effective-date window). The `TaxEngine` reads this config.
//!
//! Tenant-scoped (ADR-0010 Decision B1): every entity carries a `company_id` and every create is
//! wrapped in `company_scope::with_company_scope(Some(company), …)`, with `company_id` bound into
//! both the INSERT and every existence SELECT — defense-in-depth on top of the RLS fence. Follows
//! the catalog_write_service / pos_write_service exemplar.

use backbone_orm::company_scope;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use super::tax_engine::TaxError;

#[derive(Debug, Clone)]
pub struct NewCategory {
    pub company_id: Uuid,
    pub code: String,
    pub name: String,
    pub tax_kind: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewTemplate {
    pub company_id: Uuid,
    pub code: String,
    pub name: String,
    pub template_type: Option<String>,
    pub tax_category_id: Option<Uuid>,
    pub is_inclusive: bool,
}

#[derive(Debug, Clone)]
pub struct NewTemplateRow {
    pub company_id: Uuid,
    pub template_id: Uuid,
    pub charge_type: Option<String>,
    pub rate: Decimal,
    pub account_id: Option<Uuid>,
    pub is_withholding: bool,
    pub effective_from: NaiveDate,
    pub effective_to: Option<NaiveDate>,
    pub sort_order: i32,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewWithholding {
    pub company_id: Uuid,
    pub code: String,
    pub name: String,
    pub rate: Decimal,
    pub threshold_amount: Decimal,
    pub account_id: Option<Uuid>,
    pub effective_from: NaiveDate,
    pub effective_to: Option<NaiveDate>,
}

#[derive(Clone)]
pub struct TaxWriteService {
    db_pool: PgPool,
}

impl TaxWriteService {
    pub fn new(db_pool: PgPool) -> Self {
        Self { db_pool }
    }

    fn is_dup(e: &sqlx::Error) -> bool {
        e.as_database_error().map(|d| d.is_unique_violation()).unwrap_or(false)
    }
    fn valid_window(from: NaiveDate, to: Option<NaiveDate>) -> bool {
        to.map(|t| t >= from).unwrap_or(true)
    }

    /// Existence check filtered by the caller's company. `table` is a fixed literal from this
    /// module, never user input. The `company_id = $2` shape preserves fail-closed behavior
    /// under RLS even if the request scope wasn't set (missed scope → no rows returned).
    async fn exists_in(
        &self,
        table: &str,
        id: Uuid,
        company: Uuid,
    ) -> Result<bool, TaxError> {
        let sql = format!(
            "SELECT id FROM tax.{table} \
             WHERE id = $1 AND company_id = $2 AND (metadata->>'deleted_at') IS NULL"
        );
        let found: Option<Uuid> = sqlx::query_scalar(&sql)
            .bind(id)
            .bind(company)
            .fetch_optional(&self.db_pool)
            .await?;
        Ok(found.is_some())
    }

    pub async fn create_category(&self, c: NewCategory) -> Result<Uuid, TaxError> {
        let company = c.company_id;
        company_scope::with_company_scope(Some(company), async move {
            let id = Uuid::new_v4();
            let kind = c.tax_kind.clone().unwrap_or_else(|| "vat".to_string());
            let r = sqlx::query(
                r#"INSERT INTO tax.tax_categories (id, company_id, code, name, tax_kind, status)
                   VALUES ($1,$2,$3,$4,$5::tax_kind,'active'::tax_status)"#,
            )
            .bind(id).bind(company).bind(&c.code).bind(&c.name).bind(&kind)
            .execute(&self.db_pool).await;
            match r {
                Ok(_) => Ok(id),
                Err(e) if Self::is_dup(&e) => Err(TaxError::DuplicateCode(c.code)),
                Err(e) => Err(e.into()),
            }
        }).await
    }

    pub async fn create_template(&self, t: NewTemplate) -> Result<Uuid, TaxError> {
        let company = t.company_id;
        company_scope::with_company_scope(Some(company), async move {
            if let Some(cid) = t.tax_category_id {
                if !self.exists_in("tax_categories", cid, company).await? {
                    return Err(TaxError::CategoryNotFound(cid));
                }
            }
            let id = Uuid::new_v4();
            let tt = t.template_type.clone().unwrap_or_else(|| "sales".to_string());
            let r = sqlx::query(
                r#"INSERT INTO tax.tax_templates (id, company_id, code, name, template_type, tax_category_id, is_inclusive, status)
                   VALUES ($1,$2,$3,$4,$5::template_type,$6,$7,'active'::tax_status)"#,
            )
            .bind(id).bind(company).bind(&t.code).bind(&t.name).bind(&tt).bind(t.tax_category_id).bind(t.is_inclusive)
            .execute(&self.db_pool).await;
            match r {
                Ok(_) => Ok(id),
                Err(e) if Self::is_dup(&e) => Err(TaxError::DuplicateCode(t.code)),
                Err(e) => Err(e.into()),
            }
        }).await
    }

    pub async fn add_row(&self, row: NewTemplateRow) -> Result<Uuid, TaxError> {
        let company = row.company_id;
        company_scope::with_company_scope(Some(company), async move {
            if !Self::valid_window(row.effective_from, row.effective_to) {
                return Err(TaxError::InvalidDateRange);
            }
            if !self.exists_in("tax_templates", row.template_id, company).await? {
                return Err(TaxError::TemplateNotFound(row.template_id));
            }
            // Reject an overlapping sibling at the same sort_order (council 2026-07-03): two rows
            // effective on the same date would double-charge. `[from, to]` inclusive, open-ended =
            // infinity. Scoped to this template (which is itself company-scoped via its
            // template_id), so the check is per-tenant.
            let overlap: Option<Uuid> = sqlx::query_scalar(
                r#"SELECT id FROM tax.tax_template_rows
                   WHERE template_id=$1 AND sort_order=$2 AND (metadata->>'deleted_at') IS NULL
                     AND daterange(effective_from, COALESCE(effective_to, 'infinity'::date), '[]')
                         && daterange($3, COALESCE($4, 'infinity'::date), '[]')
                   LIMIT 1"#,
            )
            .bind(row.template_id).bind(row.sort_order).bind(row.effective_from).bind(row.effective_to)
            .fetch_optional(&self.db_pool)
            .await?;
            if overlap.is_some() {
                return Err(TaxError::OverlappingWindow(format!(
                    "template row sort_order {} overlaps an existing effective window",
                    row.sort_order
                )));
            }
            let id = Uuid::new_v4();
            let ct = row.charge_type.clone().unwrap_or_else(|| "on_net_total".to_string());
            sqlx::query(
                r#"INSERT INTO tax.tax_template_rows
                    (id, company_id, template_id, charge_type, rate, account_id, is_withholding, effective_from,
                     effective_to, sort_order, description)
                   VALUES ($1,$2,$3,$4::charge_type,$5,$6,$7,$8,$9,$10,$11)"#,
            )
            .bind(id).bind(company).bind(row.template_id).bind(&ct).bind(row.rate).bind(row.account_id)
            .bind(row.is_withholding).bind(row.effective_from).bind(row.effective_to)
            .bind(row.sort_order).bind(&row.description)
            .execute(&self.db_pool).await?;
            Ok(id)
        }).await
    }

    pub async fn create_withholding(&self, w: NewWithholding) -> Result<Uuid, TaxError> {
        let company = w.company_id;
        company_scope::with_company_scope(Some(company), async move {
            if !Self::valid_window(w.effective_from, w.effective_to) {
                return Err(TaxError::InvalidDateRange);
            }
            // Reject an overlapping window for the same code within this tenant (council 2026-07-03)
            // — so `resolve_withholding` always has exactly one applicable rate on any date. The
            // DB-level EXCLUDE (reshaped per-company by ADR-0010 B1) also enforces this.
            let overlap: Option<Uuid> = sqlx::query_scalar(
                r#"SELECT id FROM tax.withholding_categories
                   WHERE company_id=$1 AND code=$2 AND (metadata->>'deleted_at') IS NULL
                     AND daterange(effective_from, COALESCE(effective_to, 'infinity'::date), '[]')
                         && daterange($3, COALESCE($4, 'infinity'::date), '[]')
                   LIMIT 1"#,
            )
            .bind(company).bind(&w.code).bind(w.effective_from).bind(w.effective_to)
            .fetch_optional(&self.db_pool)
            .await?;
            if overlap.is_some() {
                return Err(TaxError::OverlappingWindow(format!(
                    "withholding code {} overlaps an existing effective window",
                    w.code
                )));
            }
            let id = Uuid::new_v4();
            sqlx::query(
                r#"INSERT INTO tax.withholding_categories
                    (id, company_id, code, name, rate, threshold_amount, account_id, effective_from, effective_to, status)
                   VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'active'::tax_status)"#,
            )
            .bind(id).bind(company).bind(&w.code).bind(&w.name).bind(&w.rate).bind(&w.threshold_amount)
            .bind(w.account_id).bind(w.effective_from).bind(w.effective_to)
            .execute(&self.db_pool).await?;
            Ok(id)
        }).await
    }
}

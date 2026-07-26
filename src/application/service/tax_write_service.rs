//! Validated write path for tax config — hand-authored (user-owned). Closes the CRUD-bypass:
//! templates/categories are config master data; here creates are validated (unique code, template
//! existence for a row, sane effective-date window). The `TaxEngine` reads this config.
//!
//! Tenant-scoped (ADR-0010 Decision B1): every entity carries a `company_id` and every create is
//! wrapped in `company_scope::with_company_scope(Some(company), …)`, with `company_id` bound into
//! both the INSERT and every existence SELECT — defense-in-depth on top of the RLS fence. Follows
//! the catalog_write_service / pos_write_service exemplar.
//!
//! All SQL lives in the repositories (tax_category / tax_template / tax_template_row /
//! withholding_category); this service only orchestrates. 4-layer rule.

use backbone_orm::company_scope;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    NewTaxCategoryRow, NewTaxTemplateRow, NewTaxTemplateRowRecord, NewWithholdingCategoryRow,
    TaxCategoryRepository, TaxTemplateRepository, TaxTemplateRowRepository,
    WithholdingCategoryRepository,
};

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

    pub async fn create_category(&self, c: NewCategory) -> Result<Uuid, TaxError> {
        let company = c.company_id;
        company_scope::with_company_scope(Some(company), async move {
            let id = Uuid::new_v4();
            let kind = c.tax_kind.clone().unwrap_or_else(|| "vat".to_string());
            let repos = TaxCategoryRepository::new(self.db_pool.clone());
            let r = repos.insert(&self.db_pool, &NewTaxCategoryRow {
                id, company_id: company, code: &c.code, name: &c.name, tax_kind: &kind,
            }).await;
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
                let cats = TaxCategoryRepository::new(self.db_pool.clone());
                let found = cats.find_by_id_in_company(&self.db_pool, cid, company).await?;
                if found.is_none() {
                    return Err(TaxError::CategoryNotFound(cid));
                }
            }
            let id = Uuid::new_v4();
            let tt = t.template_type.clone().unwrap_or_else(|| "sales".to_string());
            let repos = TaxTemplateRepository::new(self.db_pool.clone());
            let r = repos.insert(&self.db_pool, &NewTaxTemplateRow {
                id, company_id: company, code: &t.code, name: &t.name,
                template_type: &tt, tax_category_id: t.tax_category_id, is_inclusive: t.is_inclusive,
            }).await;
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
            let tpls = TaxTemplateRepository::new(self.db_pool.clone());
            let found = tpls.find_by_id_in_company(&self.db_pool, row.template_id, company).await?;
            if found.is_none() {
                return Err(TaxError::TemplateNotFound(row.template_id));
            }
            // Reject an overlapping sibling at the same sort_order (council 2026-07-03): two rows
            // effective on the same date would double-charge. `[from, to]` inclusive, open-ended =
            // infinity. Scoped to this template (which is itself company-scoped via its
            // template_id), so the check is per-tenant.
            let rows_repo = TaxTemplateRowRepository::new(self.db_pool.clone());
            let overlap = rows_repo.find_overlap(
                &self.db_pool, row.template_id, row.sort_order,
                row.effective_from, row.effective_to,
            ).await?;
            if overlap.is_some() {
                return Err(TaxError::OverlappingWindow(format!(
                    "template row sort_order {} overlaps an existing effective window",
                    row.sort_order
                )));
            }
            let id = Uuid::new_v4();
            let ct = row.charge_type.clone().unwrap_or_else(|| "on_net_total".to_string());
            rows_repo.insert(&self.db_pool, &NewTaxTemplateRowRecord {
                id, company_id: company, template_id: row.template_id,
                charge_type: &ct, rate: row.rate, account_id: row.account_id,
                is_withholding: row.is_withholding, effective_from: row.effective_from,
                effective_to: row.effective_to, sort_order: row.sort_order,
                description: row.description.as_deref(),
            }).await?;
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
            let whs = WithholdingCategoryRepository::new(self.db_pool.clone());
            let overlap = whs.find_overlap(
                &self.db_pool, company, &w.code, w.effective_from, w.effective_to,
            ).await?;
            if overlap.is_some() {
                return Err(TaxError::OverlappingWindow(format!(
                    "withholding code {} overlaps an existing effective window",
                    w.code
                )));
            }
            let id = Uuid::new_v4();
            whs.insert(&self.db_pool, &NewWithholdingCategoryRow {
                id, company_id: company, code: &w.code, name: &w.name, rate: w.rate,
                threshold_amount: w.threshold_amount, account_id: w.account_id,
                effective_from: w.effective_from, effective_to: w.effective_to,
            }).await?;
            Ok(id)
        }).await
    }
}

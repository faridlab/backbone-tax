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
    CompanyTaxSettingsRecord, CompanyTaxSettingsRepository, NewTaxCategoryRow,
    NewTaxRepartitionLineRecord, NewTaxTagRow, NewTaxTemplateRow, NewTaxTemplateRowRecord,
    NewWithholdingCategoryRow, RepartitionLineRecord, TaxCategoryRepository,
    TaxRepartitionLineRepository, TaxTagRepository, TaxTemplateRepository,
    TaxTemplateRowRepository, WithholdingCategoryRepository,
};

use super::tax_engine::TaxError;
use super::tax_rounding::round2;

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
    /// Cash-basis posture, materialized on the row at create. `None` ⇒ the
    /// company settings' `default_exigibility` (itself defaulting to `on_invoice`).
    pub tax_exigibility: Option<String>,
    /// Transition account for `on_payment` templates. `None` ⇒ the company
    /// settings' transition account.
    pub cash_basis_transition_account_id: Option<Uuid>,
}

/// PUT body for the company's tax posture. Full-replace semantics.
#[derive(Debug, Clone)]
pub struct NewCompanySettings {
    pub company_id: Uuid,
    /// `round_globally` | `round_per_line`.
    pub rounding_method: String,
    /// `on_invoice` | `on_payment`.
    pub default_exigibility: String,
    /// Required when `default_exigibility` is `on_payment` (and must name a
    /// reconcilable account).
    pub cash_basis_transition_account_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct NewRepartitionLine {
    pub company_id: Uuid,
    pub template_id: Uuid,
    /// `invoice` | `refund`.
    pub document_type: String,
    /// `base` | `tax`.
    pub repartition_type: String,
    pub factor_percent: Decimal,
    pub account_id: Option<Uuid>,
    pub tag_ids: Vec<Uuid>,
    pub sort_order: i32,
    pub description: Option<String>,
}

/// One tax split of a whole-family replacement (`ReplaceRepartitionFamily`).
#[derive(Debug, Clone)]
pub struct NewRepartitionSplit {
    pub factor_percent: Decimal,
    pub account_id: Option<Uuid>,
    pub tag_ids: Vec<Uuid>,
    pub sort_order: i32,
    pub description: Option<String>,
}

/// Replace ONE document-type family of a template's repartition wholesale:
/// the base line's tags plus the complete tax-split set. The retire (soft
/// delete) and the replacement inserts share a single transaction because the
/// deferred family-validation trigger would reject the intermediate
/// family-less state at a per-statement commit. This is the only sanctioned
/// way to reshape a family — `add_repartition_line` alone can never rebalance
/// a live family (its factors already sum to 100).
#[derive(Debug, Clone)]
pub struct ReplaceRepartitionFamily {
    pub company_id: Uuid,
    pub template_id: Uuid,
    /// `invoice` | `refund`.
    pub document_type: String,
    pub base_tag_ids: Vec<Uuid>,
    pub base_description: Option<String>,
    pub tax_splits: Vec<NewRepartitionSplit>,
}

#[derive(Debug, Clone)]
pub struct NewTag {
    pub company_id: Uuid,
    pub code: String,
    pub name: String,
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
        e.as_database_error()
            .map(|d| d.is_unique_violation())
            .unwrap_or(false)
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
            let r = repos
                .insert(
                    &self.db_pool,
                    &NewTaxCategoryRow {
                        id,
                        company_id: company,
                        code: &c.code,
                        name: &c.name,
                        tax_kind: &kind,
                    },
                )
                .await;
            match r {
                Ok(_) => Ok(id),
                Err(e) if Self::is_dup(&e) => Err(TaxError::DuplicateCode(c.code)),
                Err(e) => Err(e.into()),
            }
        })
        .await
    }

    /// A cash-basis posture must name a transition account that exists in
    /// `accounting.accounts` and is reconcilable — the payment-time flip pairs
    /// against that account. Fail-closed: when the accounting schema is absent
    /// the posture is unverifiable, so the validated path refuses it (raw SQL
    /// can still configure it; the DB trigger likewise cannot verify without
    /// the schema).
    async fn verify_caba_transition(&self, account: Uuid) -> Result<(), TaxError> {
        // Two statements on purpose: a prepared query that even mentions
        // accounting.accounts fails to PLAN when that schema is absent, so the
        // presence probe must run on its own before the reconcilability lookup.
        // Absent schema ⇒ unverifiable ⇒ refuse (fail-closed), matching the
        // write-path arm of the TG3 rule.
        let schema_present: Option<bool> = company_scope::fetch_optional_scalar_scoped(
            &self.db_pool,
            sqlx::query_scalar("SELECT to_regclass('accounting.accounts') IS NOT NULL"),
        )
        .await?;
        if !matches!(schema_present, Some(true)) {
            return Err(TaxError::CabaTransitionNotReconcilable(account));
        }
        let verdict: Option<bool> = company_scope::fetch_optional_scalar_scoped(
            &self.db_pool,
            sqlx::query_scalar(
                r#"SELECT EXISTS(SELECT 1 FROM accounting.accounts a
                                   WHERE a.id = $1 AND a.is_reconcilable)"#,
            )
            .bind(account),
        )
        .await?;
        match verdict {
            Some(true) => Ok(()),
            _ => Err(TaxError::CabaTransitionNotReconcilable(account)),
        }
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
            let tt = t.template_type.clone().unwrap_or_else(|| "sales".to_string());
            if !matches!(tt.as_str(), "sales" | "purchase") {
                return Err(TaxError::InvalidValue(format!("template_type: {tt}")));
            }
            // Friendly duplicate-name pre-check (the DB partial unique index is the
            // raw-SQL backstop).
            let tpls = TaxTemplateRepository::new(self.db_pool.clone());
            if tpls
                .find_by_name_in_company(&self.db_pool, company, &tt, &t.name)
                .await?
                .is_some()
            {
                return Err(TaxError::DuplicateName(t.name));
            }

            // Resolve the cash-basis posture: caller override > company settings
            // default > on_invoice. The resolved pair is MATERIALIZED on the row so
            // later company posture changes never rewrite existing templates.
            let settings = CompanyTaxSettingsRepository::new(self.db_pool.clone())
                .find_by_company(&self.db_pool, company)
                .await?;
            let exigibility = t
                .tax_exigibility
                .clone()
                .or_else(|| settings.as_ref().map(|s| s.default_exigibility.clone()))
                .unwrap_or_else(|| "on_invoice".to_string());
            if !matches!(exigibility.as_str(), "on_invoice" | "on_payment") {
                return Err(TaxError::InvalidValue(format!("tax_exigibility: {exigibility}")));
            }
            let mut transition = t
                .cash_basis_transition_account_id
                .or_else(|| settings.as_ref().and_then(|s| s.cash_basis_transition_account_id));
            if exigibility == "on_payment" {
                let account = transition.ok_or_else(|| {
                    TaxError::InvalidValue(
                        "cash_basis_transition_account_id is required when tax_exigibility is on_payment".into(),
                    )
                })?;
                self.verify_caba_transition(account).await?;
            } else {
                transition = None; // accrual templates never carry a transition account
            }

            // Template + seeded repartition families land atomically: the deferred
            // family-validation trigger would reject the intermediate states visible
            // at per-statement commits.
            let id = Uuid::new_v4();
            let mut tx = self.db_pool.begin().await?;
            company_scope::bind_current_company(&mut tx).await?;
            TaxTemplateRepository::insert_on(
                &mut *tx,
                &NewTaxTemplateRow {
                    id,
                    company_id: company,
                    code: &t.code,
                    name: &t.name,
                    template_type: &tt,
                    tax_category_id: t.tax_category_id,
                    is_inclusive: t.is_inclusive,
                    tax_exigibility: &exigibility,
                    cash_basis_transition_account_id: transition,
                },
            )
            .await?;
            // Seed both families (invoice + refund) with one base + one 100% tax
            // line each, so a new template is repartition-complete from birth and
            // documents of either kind have a routing. The tax split's account is
            // filled in later via `add_repartition_line` (replacing the seed).
            for family in ["invoice", "refund"] {
                TaxRepartitionLineRepository::insert_on(
                    &mut tx,
                    &NewTaxRepartitionLineRecord {
                        id: Uuid::new_v4(),
                        company_id: company,
                        template_id: id,
                        document_type: family,
                        repartition_type: "base",
                        factor_percent: Decimal::from(100),
                        account_id: None,
                        tag_ids: &[],
                        sort_order: 0,
                        description: None,
                    },
                )
                .await?;
                TaxRepartitionLineRepository::insert_on(
                    &mut tx,
                    &NewTaxRepartitionLineRecord {
                        id: Uuid::new_v4(),
                        company_id: company,
                        template_id: id,
                        document_type: family,
                        repartition_type: "tax",
                        factor_percent: Decimal::from(100),
                        account_id: None,
                        tag_ids: &[],
                        sort_order: 0,
                        description: Some("seeded 100% split (set the routing account)"),
                    },
                )
                .await?;
            }
            match tx.commit().await {
                Ok(_) => Ok(id),
                Err(e) if Self::is_dup(&e) => Err(TaxError::DuplicateCode(t.code)),
                Err(e) => Err(e.into()),
            }
        }).await
    }

    /// The company's tax posture, if configured. `None` ⇒ documented defaults
    /// (`round_globally` / `on_invoice`).
    pub async fn company_settings(
        &self,
        company_id: Uuid,
    ) -> Result<Option<CompanyTaxSettingsRecord>, TaxError> {
        company_scope::with_company_scope(Some(company_id), async move {
            CompanyTaxSettingsRepository::new(self.db_pool.clone())
                .find_by_company(&self.db_pool, company_id)
                .await
                .map_err(TaxError::from)
        })
        .await
    }

    /// Both repartition families of a template, ordered for display.
    pub async fn repartition_lines(
        &self,
        company_id: Uuid,
        template_id: Uuid,
    ) -> Result<Vec<RepartitionLineRecord>, TaxError> {
        company_scope::with_company_scope(Some(company_id), async move {
            TaxRepartitionLineRepository::new(self.db_pool.clone())
                .find_for_template(&self.db_pool, template_id)
                .await
                .map_err(TaxError::from)
        })
        .await
    }

    /// PUT semantics: insert-or-update the company's single live settings row.
    pub async fn upsert_company_settings(&self, s: NewCompanySettings) -> Result<Uuid, TaxError> {
        let company = s.company_id;
        company_scope::with_company_scope(Some(company), async move {
            if !matches!(s.rounding_method.as_str(), "round_globally" | "round_per_line") {
                return Err(TaxError::InvalidValue(format!(
                    "rounding_method: {}",
                    s.rounding_method
                )));
            }
            if !matches!(s.default_exigibility.as_str(), "on_invoice" | "on_payment") {
                return Err(TaxError::InvalidValue(format!(
                    "default_exigibility: {}",
                    s.default_exigibility
                )));
            }
            let transition = if s.default_exigibility == "on_payment" {
                let account = s.cash_basis_transition_account_id.ok_or_else(|| {
                    TaxError::InvalidValue(
                        "cash_basis_transition_account_id is required when default_exigibility is on_payment".into(),
                    )
                })?;
                self.verify_caba_transition(account).await?;
                Some(account)
            } else {
                None
            };
            CompanyTaxSettingsRepository::new(self.db_pool.clone())
                .upsert(
                    &self.db_pool,
                    company,
                    &s.rounding_method,
                    &s.default_exigibility,
                    transition,
                )
                .await
                .map_err(TaxError::from)
        })
        .await
    }

    /// Simulate the TG4 family validation over `existing` plus the candidate
    /// line, exactly as the DB's deferred constraint trigger will judge it at
    /// commit — friendly errors instead of an opaque commit-time raise.
    fn family_valid_after(existing: &[RepartitionLineRecord], cand: &NewRepartitionLine) -> bool {
        let mut invoice = (0usize, Vec::<Decimal>::new());
        let mut refund = (0usize, Vec::<Decimal>::new());
        let absorb = |fam: &mut (usize, Vec<Decimal>), l: &RepartitionLineRecord| {
            if l.repartition_type == "base" {
                fam.0 += 1;
            } else {
                fam.1.push(l.factor_percent);
            }
        };
        for l in existing {
            match l.document_type.as_str() {
                "invoice" => absorb(&mut invoice, l),
                _ => absorb(&mut refund, l),
            }
        }
        let target = if cand.document_type == "invoice" {
            &mut invoice
        } else {
            &mut refund
        };
        if cand.repartition_type == "base" {
            target.0 += 1;
        } else {
            target.1.push(cand.factor_percent);
        }
        let well_formed = |fam: &(usize, Vec<Decimal>)| {
            fam.0 == 1
                && !fam.1.is_empty()
                && round2(fam.1.iter().copied().sum()) == Decimal::from(100)
        };
        // Families present (having any live lines) must be maintained together.
        let invoice_present = invoice.0 > 0 || !invoice.1.is_empty();
        let refund_present = refund.0 > 0 || !refund.1.is_empty();
        if invoice_present != refund_present {
            return false;
        }
        if invoice_present && (!well_formed(&invoice) || !well_formed(&refund)) {
            return false;
        }
        true
    }

    /// Map a DB family/shape violation to the friendly error (the deferred
    /// constraint trigger is the raw-SQL backstop).
    fn is_family_violation(e: &sqlx::Error) -> bool {
        e.as_database_error()
            .map(|d| {
                d.is_check_violation()
                    || d.is_unique_violation()
                    || d.code().map(|c| c.as_ref() == "23000").unwrap_or(false)
            })
            .unwrap_or(false)
    }

    /// Add one repartition line to a template's family. Service-checked for
    /// friendly errors; the DB's deferred constraint trigger is the backstop.
    pub async fn add_repartition_line(&self, r: NewRepartitionLine) -> Result<Uuid, TaxError> {
        let company = r.company_id;
        company_scope::with_company_scope(Some(company), async move {
            if !matches!(r.document_type.as_str(), "invoice" | "refund") {
                return Err(TaxError::InvalidValue(format!(
                    "document_type: {}",
                    r.document_type
                )));
            }
            if !matches!(r.repartition_type.as_str(), "base" | "tax") {
                return Err(TaxError::InvalidValue(format!(
                    "repartition_type: {}",
                    r.repartition_type
                )));
            }
            if r.repartition_type == "base" && r.account_id.is_some() {
                return Err(TaxError::InvalidValue(
                    "base repartition lines carry tags only, never an account".into(),
                ));
            }
            if r.factor_percent == Decimal::ZERO {
                return Err(TaxError::InvalidValue(
                    "factor_percent must be nonzero".into(),
                ));
            }
            let tpls = TaxTemplateRepository::new(self.db_pool.clone());
            if tpls
                .find_by_id_in_company(&self.db_pool, r.template_id, company)
                .await?
                .is_none()
            {
                return Err(TaxError::TemplateNotFound(r.template_id));
            }
            let repo = TaxRepartitionLineRepository::new(self.db_pool.clone());
            let existing = repo.find_for_template(&self.db_pool, r.template_id).await?;
            if !Self::family_valid_after(&existing, &r) {
                return Err(TaxError::RepartitionInvalid(r.template_id));
            }
            let id = Uuid::new_v4();
            let res = repo
                .insert_scoped(
                    &self.db_pool,
                    &NewTaxRepartitionLineRecord {
                        id,
                        company_id: company,
                        template_id: r.template_id,
                        document_type: &r.document_type,
                        repartition_type: &r.repartition_type,
                        factor_percent: r.factor_percent,
                        account_id: r.account_id,
                        tag_ids: &r.tag_ids,
                        sort_order: r.sort_order,
                        description: r.description.as_deref(),
                    },
                )
                .await;
            match res {
                Ok(_) => Ok(id),
                Err(e) if Self::is_family_violation(&e) => {
                    Err(TaxError::RepartitionInvalid(r.template_id))
                }
                Err(e) => Err(e.into()),
            }
        })
        .await
    }

    /// Replace one repartition family atomically (see `ReplaceRepartitionFamily`).
    pub async fn replace_repartition_family(
        &self,
        f: ReplaceRepartitionFamily,
    ) -> Result<(), TaxError> {
        let company = f.company_id;
        company_scope::with_company_scope(Some(company), async move {
            if !matches!(f.document_type.as_str(), "invoice" | "refund") {
                return Err(TaxError::InvalidValue(format!(
                    "document_type: {}",
                    f.document_type
                )));
            }
            if f.tax_splits.is_empty() {
                return Err(TaxError::RepartitionInvalid(f.template_id));
            }
            let sum: Decimal = f.tax_splits.iter().map(|s| s.factor_percent).sum();
            if round2(sum) != Decimal::from(100) {
                return Err(TaxError::RepartitionInvalid(f.template_id));
            }
            if TaxTemplateRepository::new(self.db_pool.clone())
                .find_by_id_in_company(&self.db_pool, f.template_id, company)
                .await?
                .is_none()
            {
                return Err(TaxError::TemplateNotFound(f.template_id));
            }

            let mut tx = self.db_pool.begin().await?;
            company_scope::bind_current_company(&mut tx).await?;
            // The mirror family rule: a template is either fully pre-repartition
            // (zero live rows — legacy) or carries BOTH families. When starting
            // from the legacy shape, the requested family alone would leave the
            // template half-populated (refused by the DB guard), so the mirror
            // family is seeded with the same shape in the same transaction.
            let mirror = if f.document_type == "invoice" {
                "refund"
            } else {
                "invoice"
            };
            let legacy_shape = TaxRepartitionLineRepository::count_live_family_on(
                &mut tx,
                f.template_id,
                &f.document_type,
            )
            .await?
                == 0
                && TaxRepartitionLineRepository::count_live_family_on(
                    &mut tx,
                    f.template_id,
                    mirror,
                )
                .await?
                    == 0;
            let families: Vec<&str> = if legacy_shape {
                vec![f.document_type.as_str(), mirror]
            } else {
                vec![f.document_type.as_str()]
            };
            for family in families {
                TaxRepartitionLineRepository::soft_delete_family_on(&mut tx, f.template_id, family)
                    .await?;
                TaxRepartitionLineRepository::insert_on(
                    &mut tx,
                    &NewTaxRepartitionLineRecord {
                        id: Uuid::new_v4(),
                        company_id: company,
                        template_id: f.template_id,
                        document_type: family,
                        repartition_type: "base",
                        factor_percent: Decimal::from(100),
                        account_id: None,
                        tag_ids: &f.base_tag_ids,
                        sort_order: 0,
                        description: f.base_description.as_deref(),
                    },
                )
                .await?;
                for s in &f.tax_splits {
                    TaxRepartitionLineRepository::insert_on(
                        &mut tx,
                        &NewTaxRepartitionLineRecord {
                            id: Uuid::new_v4(),
                            company_id: company,
                            template_id: f.template_id,
                            document_type: family,
                            repartition_type: "tax",
                            factor_percent: s.factor_percent,
                            account_id: s.account_id,
                            tag_ids: &s.tag_ids,
                            sort_order: s.sort_order,
                            description: s.description.as_deref(),
                        },
                    )
                    .await?;
                }
            }
            match tx.commit().await {
                Ok(_) => Ok(()),
                Err(e) if Self::is_family_violation(&e) => {
                    Err(TaxError::RepartitionInvalid(f.template_id))
                }
                Err(e) => Err(e.into()),
            }
        })
        .await
    }

    /// Create a reporting tag (referenced by repartition lines' `tag_ids`).
    pub async fn create_tag(&self, g: NewTag) -> Result<Uuid, TaxError> {
        let company = g.company_id;
        company_scope::with_company_scope(Some(company), async move {
            let repo = TaxTagRepository::new(self.db_pool.clone());
            if repo
                .find_by_code_in_company(&self.db_pool, company, &g.code)
                .await?
                .is_some()
            {
                return Err(TaxError::DuplicateCode(g.code));
            }
            let id = Uuid::new_v4();
            repo.insert(
                &self.db_pool,
                &NewTaxTagRow {
                    id,
                    company_id: company,
                    code: &g.code,
                    name: &g.name,
                },
            )
            .await?;
            Ok(id)
        })
        .await
    }

    pub async fn add_row(&self, row: NewTemplateRow) -> Result<Uuid, TaxError> {
        let company = row.company_id;
        company_scope::with_company_scope(Some(company), async move {
            if !Self::valid_window(row.effective_from, row.effective_to) {
                return Err(TaxError::InvalidDateRange);
            }
            let tpls = TaxTemplateRepository::new(self.db_pool.clone());
            let found = tpls
                .find_by_id_in_company(&self.db_pool, row.template_id, company)
                .await?;
            if found.is_none() {
                return Err(TaxError::TemplateNotFound(row.template_id));
            }
            // Reject an overlapping sibling at the same sort_order (council 2026-07-03): two rows
            // effective on the same date would double-charge. `[from, to]` inclusive, open-ended =
            // infinity. Scoped to this template (which is itself company-scoped via its
            // template_id), so the check is per-tenant.
            let rows_repo = TaxTemplateRowRepository::new(self.db_pool.clone());
            let overlap = rows_repo
                .find_overlap(
                    &self.db_pool,
                    row.template_id,
                    row.sort_order,
                    row.effective_from,
                    row.effective_to,
                )
                .await?;
            if overlap.is_some() {
                return Err(TaxError::OverlappingWindow(format!(
                    "template row sort_order {} overlaps an existing effective window",
                    row.sort_order
                )));
            }
            let id = Uuid::new_v4();
            let ct = row
                .charge_type
                .clone()
                .unwrap_or_else(|| "on_net_total".to_string());
            rows_repo
                .insert(
                    &self.db_pool,
                    &NewTaxTemplateRowRecord {
                        id,
                        company_id: company,
                        template_id: row.template_id,
                        charge_type: &ct,
                        rate: row.rate,
                        account_id: row.account_id,
                        is_withholding: row.is_withholding,
                        effective_from: row.effective_from,
                        effective_to: row.effective_to,
                        sort_order: row.sort_order,
                        description: row.description.as_deref(),
                    },
                )
                .await?;
            Ok(id)
        })
        .await
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
            let overlap = whs
                .find_overlap(
                    &self.db_pool,
                    company,
                    &w.code,
                    w.effective_from,
                    w.effective_to,
                )
                .await?;
            if overlap.is_some() {
                return Err(TaxError::OverlappingWindow(format!(
                    "withholding code {} overlaps an existing effective window",
                    w.code
                )));
            }
            let id = Uuid::new_v4();
            whs.insert(
                &self.db_pool,
                &NewWithholdingCategoryRow {
                    id,
                    company_id: company,
                    code: &w.code,
                    name: &w.name,
                    rate: w.rate,
                    threshold_amount: w.threshold_amount,
                    account_id: w.account_id,
                    effective_from: w.effective_from,
                    effective_to: w.effective_to,
                },
            )
            .await?;
            Ok(id)
        })
        .await
    }
}

//! Guarded route composition — the RECOMMENDED way to mount the tax module.
//!
//! Hand-authored (user-owned; see `metaphor.codegen.yaml`). Tax config (categories/templates/rows/
//! withholding) is read + **validated create**; the engine is exposed as **compute** endpoints
//! (`POST /tax/calculate`, `POST /tax/withholding`) that return tax LINES — tax never posts to the
//! GL; the caller attaches the lines to an AccountingPost. Generic mutation is not mounted.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    routing::post,
    Json, Router,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::service::efaktur_service::{EFakturService, TaxComplianceError};
use crate::application::service::tax_engine::{
    DocumentTaxLine, DocumentTaxRequest, DocumentTaxRequestLine, DocumentType, TaxEngine, TaxError,
    TaxLine,
};
use crate::application::service::tax_rounding::RoundingMethod;
use crate::application::service::tax_write_service::{
    NewCategory, NewCompanySettings, NewRepartitionLine, NewRepartitionSplit, NewTag, NewTemplate,
    NewTemplateRow, NewWithholding, ReplaceRepartitionFamily, TaxWriteService,
};
use crate::infrastructure::persistence::CompanyTaxSettingsRecord;
use crate::TaxModule;

use super::{
    create_tax_category_read_routes, create_tax_template_read_routes,
    create_tax_template_row_read_routes, create_withholding_category_read_routes,
};

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}
#[derive(Debug, Serialize)]
struct IdResponse {
    id: Uuid,
}
fn err_response(e: TaxError) -> axum::response::Response {
    let status = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(ErrorBody {
            error: e.code(),
            message: e.to_string(),
        }),
    )
        .into_response()
}

/// The compliance engine's refusals map onto the same error-envelope shape (`code()` /
/// `http_status()`), so the e-Faktur surface answers with codes callers can branch on —
/// `period_not_open`, `period_not_finalized`, `efaktur_not_found`, … — not opaque 500s.
fn compliance_err_response(e: TaxComplianceError) -> axum::response::Response {
    let status = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(ErrorBody {
            error: e.code(),
            message: e.to_string(),
        }),
    )
        .into_response()
}

// ── Tenant consistency ────────────────────────────────────────────────────────
//
// Every body or query below names the caller's `companyId`, and the write service binds that value
// into its statements — it would override whatever company the caller's token established. When a
// host has mounted an ambient company scope (backbone-auth's `company_auth` wraps every request in
// it), the named company must agree with it, or an authenticated tenant could shape ANY company's
// tax configuration simply by naming it in the body. With no ambient scope (unit tests, trusted
// internal hosts) the check is skipped — the module keeps its standalone shape.
fn tenant_guard(requested: Uuid) -> Option<axum::response::Response> {
    match backbone_orm::current_company() {
        Some(authenticated) if authenticated != requested => {
            Some(err_response(TaxError::CompanyMismatch))
        }
        _ => None,
    }
}

// ── config writes ──────────────────────────────────────────────────────────────
// Each create body carries the caller's `companyId` (ADR-0010 B1): the write service binds it
// into the INSERT and wraps the call in `with_company_scope`. The compute endpoints below read
// the company from the ambient request scope (set by the deployment's scope middleware).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateCategoryBody {
    company_id: Uuid,
    code: String,
    name: String,
    #[serde(default)]
    tax_kind: Option<String>,
}
async fn create_category(
    State(svc): State<Arc<TaxWriteService>>,
    Json(b): Json<CreateCategoryBody>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(b.company_id) {
        return r;
    }
    match svc
        .create_category(NewCategory {
            company_id: b.company_id,
            code: b.code,
            name: b.name,
            tax_kind: b.tax_kind,
        })
        .await
    {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err_response(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTemplateBody {
    company_id: Uuid,
    code: String,
    name: String,
    #[serde(default)]
    template_type: Option<String>,
    #[serde(default)]
    tax_category_id: Option<Uuid>,
    #[serde(default)]
    is_inclusive: bool,
    /// `on_invoice` | `on_payment` — absent ⇒ the company settings default.
    #[serde(default)]
    tax_exigibility: Option<String>,
    #[serde(default)]
    cash_basis_transition_account_id: Option<Uuid>,
}
async fn create_template(
    State(svc): State<Arc<TaxWriteService>>,
    Json(b): Json<CreateTemplateBody>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(b.company_id) {
        return r;
    }
    match svc
        .create_template(NewTemplate {
            company_id: b.company_id,
            code: b.code,
            name: b.name,
            template_type: b.template_type,
            tax_category_id: b.tax_category_id,
            is_inclusive: b.is_inclusive,
            tax_exigibility: b.tax_exigibility,
            cash_basis_transition_account_id: b.cash_basis_transition_account_id,
        })
        .await
    {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err_response(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompanySettingsBody {
    company_id: Uuid,
    rounding_method: String,
    default_exigibility: String,
    #[serde(default)]
    cash_basis_transition_account_id: Option<Uuid>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompanySettingsOut {
    company_id: Uuid,
    rounding_method: String,
    default_exigibility: String,
    cash_basis_transition_account_id: Option<Uuid>,
}
impl From<CompanyTaxSettingsRecord> for CompanySettingsOut {
    fn from(s: CompanyTaxSettingsRecord) -> Self {
        Self {
            company_id: s.company_id,
            rounding_method: s.rounding_method,
            default_exigibility: s.default_exigibility,
            cash_basis_transition_account_id: s.cash_basis_transition_account_id,
        }
    }
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompanyIdQuery {
    company_id: Uuid,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepartitionQuery {
    company_id: Uuid,
    template_id: Uuid,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepartitionLineOut {
    id: Uuid,
    document_type: String,
    repartition_type: String,
    factor_percent: Decimal,
    account_id: Option<Uuid>,
    sort_order: i32,
}
async fn list_repartition_lines(
    State(svc): State<Arc<TaxWriteService>>,
    axum::extract::Query(q): axum::extract::Query<RepartitionQuery>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(q.company_id) {
        return r;
    }
    match svc.repartition_lines(q.company_id, q.template_id).await {
        Ok(lines) => {
            let out: Vec<RepartitionLineOut> = lines
                .into_iter()
                .map(|l| RepartitionLineOut {
                    id: l.id,
                    document_type: l.document_type,
                    repartition_type: l.repartition_type,
                    factor_percent: l.factor_percent,
                    account_id: l.account_id,
                    sort_order: l.sort_order,
                })
                .collect();
            (StatusCode::OK, Json(out)).into_response()
        }
        Err(e) => err_response(e),
    }
}
async fn get_company_settings(
    State(svc): State<Arc<TaxWriteService>>,
    axum::extract::Query(q): axum::extract::Query<CompanyIdQuery>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(q.company_id) {
        return r;
    }
    match svc.company_settings(q.company_id).await {
        Ok(s) => (StatusCode::OK, Json(s.map(CompanySettingsOut::from))).into_response(),
        Err(e) => err_response(e),
    }
}
async fn put_company_settings(
    State(svc): State<Arc<TaxWriteService>>,
    Json(b): Json<CompanySettingsBody>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(b.company_id) {
        return r;
    }
    match svc
        .upsert_company_settings(NewCompanySettings {
            company_id: b.company_id,
            rounding_method: b.rounding_method,
            default_exigibility: b.default_exigibility,
            cash_basis_transition_account_id: b.cash_basis_transition_account_id,
        })
        .await
    {
        Ok(id) => (StatusCode::OK, Json(IdResponse { id })).into_response(),
        Err(e) => err_response(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddRepartitionLineBody {
    company_id: Uuid,
    template_id: Uuid,
    /// `invoice` | `refund`.
    document_type: String,
    /// `base` | `tax`.
    repartition_type: String,
    factor_percent: Decimal,
    #[serde(default)]
    account_id: Option<Uuid>,
    #[serde(default)]
    tag_ids: Vec<Uuid>,
    #[serde(default)]
    sort_order: i32,
    #[serde(default)]
    description: Option<String>,
}
async fn add_repartition_line(
    State(svc): State<Arc<TaxWriteService>>,
    Json(b): Json<AddRepartitionLineBody>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(b.company_id) {
        return r;
    }
    match svc
        .add_repartition_line(NewRepartitionLine {
            company_id: b.company_id,
            template_id: b.template_id,
            document_type: b.document_type,
            repartition_type: b.repartition_type,
            factor_percent: b.factor_percent,
            account_id: b.account_id,
            tag_ids: b.tag_ids,
            sort_order: b.sort_order,
            description: b.description,
        })
        .await
    {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err_response(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplaceFamilyBody {
    company_id: Uuid,
    template_id: Uuid,
    /// `invoice` | `refund`.
    document_type: String,
    #[serde(default)]
    base_tag_ids: Vec<Uuid>,
    #[serde(default)]
    base_description: Option<String>,
    tax_splits: Vec<ReplaceSplitBody>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplaceSplitBody {
    factor_percent: Decimal,
    #[serde(default)]
    account_id: Option<Uuid>,
    #[serde(default)]
    tag_ids: Vec<Uuid>,
    #[serde(default)]
    sort_order: i32,
    #[serde(default)]
    description: Option<String>,
}
/// PUT semantics: replace ONE document-type family wholesale (the only sanctioned
/// reshape — additive posts can never rebalance a live family whose factors
/// already sum to 100).
async fn replace_repartition_family(
    State(svc): State<Arc<TaxWriteService>>,
    Json(b): Json<ReplaceFamilyBody>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(b.company_id) {
        return r;
    }
    match svc
        .replace_repartition_family(ReplaceRepartitionFamily {
            company_id: b.company_id,
            template_id: b.template_id,
            document_type: b.document_type,
            base_tag_ids: b.base_tag_ids,
            base_description: b.base_description,
            tax_splits: b
                .tax_splits
                .into_iter()
                .map(|s| NewRepartitionSplit {
                    factor_percent: s.factor_percent,
                    account_id: s.account_id,
                    tag_ids: s.tag_ids,
                    sort_order: s.sort_order,
                    description: s.description,
                })
                .collect(),
        })
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err_response(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTagBody {
    company_id: Uuid,
    code: String,
    name: String,
}
async fn create_tag(
    State(svc): State<Arc<TaxWriteService>>,
    Json(b): Json<CreateTagBody>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(b.company_id) {
        return r;
    }
    match svc
        .create_tag(NewTag {
            company_id: b.company_id,
            code: b.code,
            name: b.name,
        })
        .await
    {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err_response(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddRowBody {
    company_id: Uuid,
    template_id: Uuid,
    #[serde(default)]
    charge_type: Option<String>,
    rate: Decimal,
    #[serde(default)]
    account_id: Option<Uuid>,
    #[serde(default)]
    is_withholding: bool,
    effective_from: NaiveDate,
    #[serde(default)]
    effective_to: Option<NaiveDate>,
    #[serde(default)]
    sort_order: i32,
    #[serde(default)]
    description: Option<String>,
}
async fn add_row(
    State(svc): State<Arc<TaxWriteService>>,
    Json(b): Json<AddRowBody>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(b.company_id) {
        return r;
    }
    match svc
        .add_row(NewTemplateRow {
            company_id: b.company_id,
            template_id: b.template_id,
            charge_type: b.charge_type,
            rate: b.rate,
            account_id: b.account_id,
            is_withholding: b.is_withholding,
            effective_from: b.effective_from,
            effective_to: b.effective_to,
            sort_order: b.sort_order,
            description: b.description,
        })
        .await
    {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err_response(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateWithholdingBody {
    company_id: Uuid,
    code: String,
    name: String,
    rate: Decimal,
    #[serde(default)]
    threshold_amount: Decimal,
    #[serde(default)]
    account_id: Option<Uuid>,
    effective_from: NaiveDate,
    #[serde(default)]
    effective_to: Option<NaiveDate>,
}
async fn create_withholding(
    State(svc): State<Arc<TaxWriteService>>,
    Json(b): Json<CreateWithholdingBody>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(b.company_id) {
        return r;
    }
    match svc
        .create_withholding(NewWithholding {
            company_id: b.company_id,
            code: b.code,
            name: b.name,
            rate: b.rate,
            threshold_amount: b.threshold_amount,
            account_id: b.account_id,
            effective_from: b.effective_from,
            effective_to: b.effective_to,
        })
        .await
    {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err_response(e),
    }
}

// ── compute (the seam: returns tax lines, never posts) ──────────────────────────
#[derive(Debug, Serialize)]
struct TaxLineOut {
    account_id: Option<Uuid>,
    rate: Decimal,
    tax_amount: Decimal,
    is_withholding: bool,
    description: Option<String>,
}
impl From<TaxLine> for TaxLineOut {
    fn from(l: TaxLine) -> Self {
        Self {
            account_id: l.account_id,
            rate: l.rate,
            tax_amount: l.tax_amount,
            is_withholding: l.is_withholding,
            description: l.description,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalculateBody {
    template_id: Uuid,
    base_amount: Decimal,
    on_date: NaiveDate,
}
async fn calculate(
    State(engine): State<Arc<TaxEngine>>,
    Json(b): Json<CalculateBody>,
) -> axum::response::Response {
    match engine
        .calculate(b.template_id, b.base_amount, b.on_date)
        .await
    {
        Ok(lines) => {
            let out: Vec<TaxLineOut> = lines.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(out)).into_response()
        }
        Err(e) => err_response(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentCalculateLineBody {
    template_id: Uuid,
    quantity: Decimal,
    unit_price: Decimal,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentCalculateBody {
    company_id: Uuid,
    /// `invoice` | `refund`.
    document_type: String,
    on_date: NaiveDate,
    lines: Vec<DocumentCalculateLineBody>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DocumentTaxLineOut {
    source_index: usize,
    template_id: Uuid,
    account_id: Option<Uuid>,
    real_account_id: Option<Uuid>,
    rate: Decimal,
    tax_amount: Decimal,
    is_withholding: bool,
    description: Option<String>,
    tag_ids: Vec<Uuid>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DocumentTaxResultOut {
    net_amounts: Vec<Decimal>,
    lines: Vec<DocumentTaxLineOut>,
    excluded_total: Decimal,
    tax_total: Decimal,
    included_total: Decimal,
    method: &'static str,
    base_tags: Vec<Uuid>,
}
async fn calculate_document(
    State(engine): State<Arc<TaxEngine>>,
    Json(b): Json<DocumentCalculateBody>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(b.company_id) {
        return r;
    }
    let doc_type = match DocumentType::from_db(&b.document_type) {
        Some(t) => t,
        None => {
            return err_response(TaxError::InvalidValue(format!(
                "documentType must be 'invoice' or 'refund', got '{}'",
                b.document_type
            )))
        }
    };
    let req = DocumentTaxRequest {
        company_id: b.company_id,
        document_type: doc_type,
        on_date: b.on_date,
        lines: b
            .lines
            .into_iter()
            .map(|l| DocumentTaxRequestLine {
                template_id: l.template_id,
                quantity: l.quantity,
                unit_price: l.unit_price,
            })
            .collect(),
    };
    match engine.calculate_document(&req).await {
        Ok(r) => {
            let out = DocumentTaxResultOut {
                net_amounts: r.net_amounts,
                lines: r
                    .lines
                    .into_iter()
                    .map(|l| DocumentTaxLineOut {
                        source_index: l.source_index,
                        template_id: l.template_id,
                        account_id: l.account_id,
                        real_account_id: l.real_account_id,
                        rate: l.rate,
                        tax_amount: l.tax_amount,
                        is_withholding: l.is_withholding,
                        description: l.description,
                        tag_ids: l.tag_ids,
                    })
                    .collect(),
                excluded_total: r.excluded_total,
                tax_total: r.tax_total,
                included_total: r.included_total,
                method: RoundingMethod::as_db(r.method),
                base_tags: r.base_tags,
            };
            (StatusCode::OK, Json(out)).into_response()
        }
        Err(e) => err_response(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WithholdingBody {
    category_id: Uuid,
    base_amount: Decimal,
    on_date: NaiveDate,
}
async fn resolve_withholding(
    State(engine): State<Arc<TaxEngine>>,
    Json(b): Json<WithholdingBody>,
) -> axum::response::Response {
    match engine
        .resolve_withholding(b.category_id, b.base_amount, b.on_date)
        .await
    {
        Ok(line) => {
            let out: Option<TaxLineOut> = line.map(Into::into);
            (StatusCode::OK, Json(out)).into_response()
        }
        Err(e) => err_response(e),
    }
}

// ── e-Faktur documents + masa-pajak filing lifecycle ──────────────────────────
//
// Kebab-case bases under /e-faktur and /filing-periods — deliberately NOT the generated
// snake_case CRUD bases (`/e_faktur_documents`, `/tax_filing_periods`), which stay unmounted:
// generic mutation over the numbering records would break the gapless invariant. The verbs here
// are lifecycle transitions only; hosts gate them behind a role (the surface never binds a role
// itself — that is a composition decision).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EFakturDocumentQuery {
    company_id: Uuid,
    /// Masa pajak start (YYYY-MM-DD, the first day of the month).
    period: NaiveDate,
    /// Optional status filter: `assigned` | `confirmed` | `voided`.
    #[serde(default)]
    status: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EFakturDocumentOut {
    id: Uuid,
    tax_transaction_id: Uuid,
    number: String,
    transaction_code: String,
    taxpayer_segment: String,
    period: NaiveDate,
    sequence: i32,
    assignment_date: NaiveDate,
    status: String,
}
impl From<crate::infrastructure::persistence::EFakturDocumentRow> for EFakturDocumentOut {
    fn from(d: crate::infrastructure::persistence::EFakturDocumentRow) -> Self {
        Self {
            id: d.id,
            tax_transaction_id: d.tax_transaction_id,
            number: d.number,
            transaction_code: d.transaction_code,
            taxpayer_segment: d.taxpayer_segment,
            period: d.period,
            sequence: d.sequence,
            assignment_date: d.assignment_date,
            status: d.status,
        }
    }
}
async fn list_efaktur_documents(
    State(svc): State<Arc<EFakturService>>,
    axum::extract::Query(q): axum::extract::Query<EFakturDocumentQuery>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(q.company_id) {
        return r;
    }
    match svc
        .list_period_documents(q.company_id, q.period, q.status.as_deref())
        .await
    {
        Ok(docs) => {
            let out: Vec<EFakturDocumentOut> = docs.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(out)).into_response()
        }
        Err(e) => compliance_err_response(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompanyBody {
    company_id: Uuid,
}
async fn confirm_efaktur(
    State(svc): State<Arc<EFakturService>>,
    Path(id): Path<Uuid>,
    Json(b): Json<CompanyBody>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(b.company_id) {
        return r;
    }
    match svc.confirm_efaktur(b.company_id, id).await {
        Ok(doc) => (StatusCode::OK, Json(EFakturDocumentOut::from(doc))).into_response(),
        Err(e) => compliance_err_response(e),
    }
}
async fn void_efaktur(
    State(svc): State<Arc<EFakturService>>,
    Path(id): Path<Uuid>,
    Json(b): Json<CompanyBody>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(b.company_id) {
        return r;
    }
    match svc.void_efaktur(b.company_id, id).await {
        Ok(doc) => (StatusCode::OK, Json(EFakturDocumentOut::from(doc))).into_response(),
        Err(e) => compliance_err_response(e),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FilingPeriodOut {
    id: Uuid,
    period: NaiveDate,
    status: String,
    next_sequence: i32,
    output_total: Decimal,
    input_total: Decimal,
    withholding_total: Decimal,
}
impl From<crate::infrastructure::persistence::FilingPeriodRow> for FilingPeriodOut {
    fn from(p: crate::infrastructure::persistence::FilingPeriodRow) -> Self {
        Self {
            id: p.id,
            period: p.period,
            status: p.status,
            next_sequence: p.next_sequence,
            output_total: p.output_total,
            input_total: p.input_total,
            withholding_total: p.withholding_total,
        }
    }
}
async fn list_filing_periods(
    State(svc): State<Arc<EFakturService>>,
    axum::extract::Query(q): axum::extract::Query<CompanyIdQuery>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(q.company_id) {
        return r;
    }
    match svc.list_filing_periods(q.company_id).await {
        Ok(periods) => {
            let out: Vec<FilingPeriodOut> = periods.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(out)).into_response()
        }
        Err(e) => compliance_err_response(e),
    }
}

fn parse_period(p: &str) -> Option<NaiveDate> {
    // Accept only a real masa-pajak start: YYYY-MM-01. Anything else is a client error.
    if !p.ends_with("-01") {
        return None;
    }
    p.parse::<NaiveDate>().ok()
}
async fn finalize_filing_period(
    State(svc): State<Arc<EFakturService>>,
    Path(period): Path<String>,
    Json(b): Json<CompanyBody>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(b.company_id) {
        return r;
    }
    let Some(period) = parse_period(&period) else {
        return err_response(TaxError::InvalidValue(
            "period must be the masa pajak start date (YYYY-MM-01)".into(),
        ));
    };
    match svc.finalize_period(b.company_id, period).await {
        Ok(row) => (StatusCode::OK, Json(FilingPeriodOut::from(row))).into_response(),
        Err(e) => compliance_err_response(e),
    }
}
async fn file_filing_period(
    State(svc): State<Arc<EFakturService>>,
    Path(period): Path<String>,
    Json(b): Json<CompanyBody>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(b.company_id) {
        return r;
    }
    let Some(period) = parse_period(&period) else {
        return err_response(TaxError::InvalidValue(
            "period must be the masa pajak start date (YYYY-MM-01)".into(),
        ));
    };
    match svc.file_period(b.company_id, period).await {
        Ok(row) => (StatusCode::OK, Json(FilingPeriodOut::from(row))).into_response(),
        Err(e) => compliance_err_response(e),
    }
}

/// One export row: the document joined to its transaction's invoice projection and totals. The
/// host's CSV verb joins buyer identity + per-line detail from billing on top of this.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EFakturExportRowOut {
    document: EFakturDocumentOut,
    invoice_ref: Uuid,
    posting_date: NaiveDate,
    taxable_base: Decimal,
    output_total: Decimal,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EFakturExportQuery {
    company_id: Uuid,
    period: NaiveDate,
}
async fn list_efaktur_export_rows(
    State(svc): State<Arc<EFakturService>>,
    axum::extract::Query(q): axum::extract::Query<EFakturExportQuery>,
) -> axum::response::Response {
    if let Some(r) = tenant_guard(q.company_id) {
        return r;
    }
    match svc.export_rows(q.company_id, q.period).await {
        Ok(rows) => {
            let out: Vec<EFakturExportRowOut> = rows
                .into_iter()
                .map(|r| EFakturExportRowOut {
                    document: EFakturDocumentOut::from(r.document),
                    invoice_ref: r.invoice_ref,
                    posting_date: r.posting_date,
                    taxable_base: r.taxable_base,
                    output_total: r.output_total,
                })
                .collect();
            (StatusCode::OK, Json(out)).into_response()
        }
        Err(e) => compliance_err_response(e),
    }
}

fn create_tax_write_routes(svc: Arc<TaxWriteService>) -> Router {
    Router::new()
        .route("/tax-categories", post(create_category))
        .route("/tax-templates", post(create_template))
        .route("/tax-template-rows", post(add_row))
        .route("/withholding-categories", post(create_withholding))
        .route(
            "/company-tax-settings",
            get(get_company_settings).put(put_company_settings),
        )
        .route(
            "/tax-repartition-lines",
            get(list_repartition_lines)
                .post(add_repartition_line)
                .put(replace_repartition_family),
        )
        .route("/tax-tags", post(create_tag))
        .with_state(svc)
}

fn create_tax_compute_routes(engine: Arc<TaxEngine>) -> Router {
    Router::new()
        .route("/tax/calculate", post(calculate))
        .route("/tax/calculate-document", post(calculate_document))
        .route("/tax/withholding", post(resolve_withholding))
        .with_state(engine)
}

fn create_efaktur_routes(svc: Arc<EFakturService>) -> Router {
    Router::new()
        .route("/e-faktur/documents", get(list_efaktur_documents))
        .route("/e-faktur/documents/:id/confirm", post(confirm_efaktur))
        .route("/e-faktur/documents/:id/void", post(void_efaktur))
        .route("/e-faktur/export-rows", get(list_efaktur_export_rows))
        .route("/filing-periods", get(list_filing_periods))
        .route(
            "/filing-periods/:period/finalize",
            post(finalize_filing_period),
        )
        .route("/filing-periods/:period/file", post(file_filing_period))
        .with_state(svc)
}

/// Mount the tax module: read config + validated create + the compute engine + the e-Faktur
/// document / masa-pajak filing lifecycle.
/// **Prefer this over `TaxModule::all_crud_routes()` for any real deployment.**
pub fn create_guarded_tax_routes(m: &TaxModule) -> Router {
    Router::new()
        .merge(create_tax_category_read_routes(
            m.tax_category_service.clone(),
        ))
        .merge(create_tax_template_read_routes(
            m.tax_template_service.clone(),
        ))
        .merge(create_tax_template_row_read_routes(
            m.tax_template_row_service.clone(),
        ))
        .merge(create_withholding_category_read_routes(
            m.withholding_category_service.clone(),
        ))
        .merge(create_tax_write_routes(m.tax_write_service.clone()))
        .merge(create_tax_compute_routes(m.tax_engine.clone()))
        .merge(create_efaktur_routes(m.efaktur_service.clone()))
}

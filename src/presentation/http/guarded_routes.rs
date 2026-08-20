//! Guarded route composition — the RECOMMENDED way to mount the tax module.
//!
//! Hand-authored (user-owned; see `metaphor.codegen.yaml`). Tax config (categories/templates/rows/
//! withholding) is read + **validated create**; the engine is exposed as **compute** endpoints
//! (`POST /tax/calculate`, `POST /tax/withholding`) that return tax LINES — tax never posts to the
//! GL; the caller attaches the lines to an AccountingPost. Generic mutation is not mounted.

use std::sync::Arc;

use axum::{
    extract::State, http::StatusCode, response::IntoResponse, routing::get, routing::post, Json,
    Router,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    match svc.company_settings(q.company_id).await {
        Ok(s) => (StatusCode::OK, Json(s.map(CompanySettingsOut::from))).into_response(),
        Err(e) => err_response(e),
    }
}
async fn put_company_settings(
    State(svc): State<Arc<TaxWriteService>>,
    Json(b): Json<CompanySettingsBody>,
) -> axum::response::Response {
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

/// Mount the tax module: read config + validated create + the compute engine.
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
}

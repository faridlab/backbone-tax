//! Guarded route composition — the RECOMMENDED way to mount the tax module.
//!
//! Hand-authored (user-owned; see `metaphor.codegen.yaml`). Tax config (categories/templates/rows/
//! withholding) is read + **validated create**; the engine is exposed as **compute** endpoints
//! (`POST /tax/calculate`, `POST /tax/withholding`) that return tax LINES — tax never posts to the
//! GL; the caller attaches the lines to an AccountingPost. Generic mutation is not mounted.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::service::tax_engine::{TaxEngine, TaxError, TaxLine};
use crate::application::service::tax_write_service::{
    NewCategory, NewTemplate, NewTemplateRow, NewWithholding, TaxWriteService,
};
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
    (status, Json(ErrorBody { error: e.code(), message: e.to_string() })).into_response()
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
    match engine.calculate(b.template_id, b.base_amount, b.on_date).await {
        Ok(lines) => {
            let out: Vec<TaxLineOut> = lines.into_iter().map(Into::into).collect();
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
    match engine.resolve_withholding(b.category_id, b.base_amount, b.on_date).await {
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
        .with_state(svc)
}

fn create_tax_compute_routes(engine: Arc<TaxEngine>) -> Router {
    Router::new()
        .route("/tax/calculate", post(calculate))
        .route("/tax/withholding", post(resolve_withholding))
        .with_state(engine)
}

/// Mount the tax module: read config + validated create + the compute engine.
/// **Prefer this over `TaxModule::all_crud_routes()` for any real deployment.**
pub fn create_guarded_tax_routes(m: &TaxModule) -> Router {
    Router::new()
        .merge(create_tax_category_read_routes(m.tax_category_service.clone()))
        .merge(create_tax_template_read_routes(m.tax_template_service.clone()))
        .merge(create_tax_template_row_read_routes(m.tax_template_row_service.clone()))
        .merge(create_withholding_category_read_routes(m.withholding_category_service.clone()))
        .merge(create_tax_write_routes(m.tax_write_service.clone()))
        .merge(create_tax_compute_routes(m.tax_engine.clone()))
}

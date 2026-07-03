//! Route-level probes: config writes are validated, the compute endpoints return tax lines, and
//! generic mutation is not exposed on the guarded surface. Requires DATABASE_URL (:5433).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

use backbone_tax::{create_guarded_tax_routes, TaxModule};

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_tax".to_string());
    PgPool::connect(&url).await.unwrap()
}
async fn module(pool: &PgPool) -> TaxModule {
    TaxModule::builder().with_database(pool.clone()).build().unwrap()
}
async fn req(app: axum::Router, method: &str, uri: &str, body: Option<String>) -> (StatusCode, String) {
    let b = body.map(Body::from).unwrap_or(Body::empty());
    let resp = app
        .oneshot(Request::builder().method(method).uri(uri).header("content-type","application/json").body(b).unwrap())
        .await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}
fn uq(p: &str) -> String { format!("{p}-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]) }

// IGC-1: generic bulk create on a config entity is not exposed on the guarded surface.
#[tokio::test]
async fn guarded_routes_lock_generic_template_bulk() {
    let pool = pool().await;
    let (status, _) = req(create_guarded_tax_routes(&module(&pool).await), "POST", "/tax-templates/bulk", Some("[]".into())).await;
    assert!(
        status == StatusCode::METHOD_NOT_ALLOWED || status == StatusCode::NOT_FOUND,
        "generic bulk template create must not be exposed; got {status}"
    );
}

// IGC-2: a template row for a non-existent template is rejected.
#[tokio::test]
async fn guarded_row_rejects_missing_template() {
    let pool = pool().await;
    let body = format!(r#"{{"templateId":"{}","rate":"11","effectiveFrom":"2022-04-01"}}"#, uuid::Uuid::new_v4());
    let (status, _) = req(create_guarded_tax_routes(&module(&pool).await), "POST", "/tax-template-rows", Some(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// IGC-3: an invalid effective-date window is rejected.
#[tokio::test]
async fn guarded_row_rejects_bad_date_window() {
    let pool = pool().await;
    let app = create_guarded_tax_routes(&module(&pool).await);
    let (_, body) = req(app, "POST", "/tax-templates",
        Some(format!(r#"{{"code":"{}","name":"T","templateType":"sales"}}"#, uq("T")))).await;
    let tid = body.split("\"id\":\"").nth(1).unwrap().split('"').next().unwrap().to_string();
    // effective_to before effective_from
    let row = format!(r#"{{"templateId":"{tid}","rate":"11","effectiveFrom":"2025-01-01","effectiveTo":"2024-01-01"}}"#);
    let (status, _) = req(create_guarded_tax_routes(&module(&pool).await), "POST", "/tax-template-rows", Some(row)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// IGC-4: the compute endpoint returns tax lines end to end (PPN 11% of 1,000,000 → 110,000).
#[tokio::test]
async fn compute_endpoint_returns_tax_lines() {
    let pool = pool().await;
    // seed a template + row via the guarded write surface
    let (_, tbody) = req(create_guarded_tax_routes(&module(&pool).await), "POST", "/tax-templates",
        Some(format!(r#"{{"code":"{}","name":"PPN","templateType":"sales"}}"#, uq("C")))).await;
    let tid = tbody.split("\"id\":\"").nth(1).unwrap().split('"').next().unwrap().to_string();
    req(create_guarded_tax_routes(&module(&pool).await), "POST", "/tax-template-rows",
        Some(format!(r#"{{"templateId":"{tid}","rate":"11","effectiveFrom":"2022-04-01"}}"#))).await;

    let calc = format!(r#"{{"templateId":"{tid}","baseAmount":"1000000","onDate":"2026-07-03"}}"#);
    let (status, body) = req(create_guarded_tax_routes(&module(&pool).await), "POST", "/tax/calculate", Some(calc)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#""tax_amount":"110000""#) && body.contains(r#""rate":"11""#),
        "expected a PPN 110,000 line; got {body}"
    );
}

//! Route-level probes: config writes are validated, the compute endpoints return tax lines, and
//! generic mutation is not exposed on the guarded surface. Requires DATABASE_URL (:5433).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

use backbone_orm::company_scope;
use backbone_tax::{create_guarded_tax_routes, TaxModule};

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:5433/backbone_tax".to_string()
    });
    PgPool::connect(&url).await.unwrap()
}
async fn module(pool: &PgPool) -> TaxModule {
    TaxModule::builder()
        .with_database(pool.clone())
        .build()
        .unwrap()
}
async fn req(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<String>,
) -> (StatusCode, String) {
    let b = body.map(Body::from).unwrap_or(Body::empty());
    let resp = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(b)
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}
fn uq(p: &str) -> String {
    format!("{p}-{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
}

// IGC-1: generic bulk create on a config entity is not exposed on the guarded surface.
#[tokio::test]
async fn guarded_routes_lock_generic_template_bulk() {
    let pool = pool().await;
    let (status, _) = req(
        create_guarded_tax_routes(&module(&pool).await),
        "POST",
        "/tax-templates/bulk",
        Some("[]".into()),
    )
    .await;
    assert!(
        status == StatusCode::METHOD_NOT_ALLOWED || status == StatusCode::NOT_FOUND,
        "generic bulk template create must not be exposed; got {status}"
    );
}

// IGC-2: a template row for a non-existent template is rejected.
#[tokio::test]
async fn guarded_row_rejects_missing_template() {
    let pool = pool().await;
    let body = format!(
        r#"{{"templateId":"{}","rate":"11","effectiveFrom":"2022-04-01"}}"#,
        uuid::Uuid::new_v4()
    );
    let (status, _) = req(
        create_guarded_tax_routes(&module(&pool).await),
        "POST",
        "/tax-template-rows",
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// IGC-3: an invalid effective-date window is rejected.
#[tokio::test]
async fn guarded_row_rejects_bad_date_window() {
    let pool = pool().await;
    let company = uuid::Uuid::new_v4();
    let app = create_guarded_tax_routes(&module(&pool).await);
    let (_, body) = req(
        app,
        "POST",
        "/tax-templates",
        Some(format!(
            r#"{{"companyId":"{company}","code":"{}","name":"T","templateType":"sales"}}"#,
            uq("T")
        )),
    )
    .await;
    let tid = body
        .split("\"id\":\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    // effective_to before effective_from
    let row = format!(
        r#"{{"companyId":"{company}","templateId":"{tid}","rate":"11","effectiveFrom":"2025-01-01","effectiveTo":"2024-01-01"}}"#
    );
    let (status, _) = req(
        create_guarded_tax_routes(&module(&pool).await),
        "POST",
        "/tax-template-rows",
        Some(row),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// IGC-4: the compute endpoint returns tax lines end to end (PPN 11% of 1,000,000 → 110,000).
#[tokio::test]
async fn compute_endpoint_returns_tax_lines() {
    let pool = pool().await;
    let company = uuid::Uuid::new_v4();
    // seed a template + row via the guarded write surface
    let (_, tbody) = req(
        create_guarded_tax_routes(&module(&pool).await),
        "POST",
        "/tax-templates",
        Some(format!(
            r#"{{"companyId":"{company}","code":"{}","name":"PPN","templateType":"sales"}}"#,
            uq("C")
        )),
    )
    .await;
    let tid = tbody
        .split("\"id\":\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    req(create_guarded_tax_routes(&module(&pool).await), "POST", "/tax-template-rows",
        Some(format!(r#"{{"companyId":"{company}","templateId":"{tid}","rate":"11","effectiveFrom":"2022-04-01"}}"#))).await;

    let calc = format!(r#"{{"templateId":"{tid}","baseAmount":"1000000","onDate":"2026-07-03"}}"#);
    // The compute endpoint reads company from the AMBIENT task-local scope (set in deployment by the
    // scope middleware), not the body — wrap the call in with_company_scope so the engine sees the
    // same tenant the rows were created under (else it fails loud as NoCompanyScope → 401).
    let (status, body) = company_scope::with_company_scope(
        Some(company),
        req(
            create_guarded_tax_routes(&module(&pool).await),
            "POST",
            "/tax/calculate",
            Some(calc),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#""tax_amount":"110000"#) && body.contains(r#""rate":"11."#),
        "expected a PPN 110,000 line (11% of 1,000,000); got {body}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// DB-guard probes (TG1/TG3/TG4/TG6) and the tenant fence on the new tables.
// The HTTP legs pin the friendly service arm; the raw-SQL legs pin that the
// invariants hold even for writers that bypass the service entirely.
// ─────────────────────────────────────────────────────────────────────────────

/// Create a live template via the guarded surface; returns (company, template id).
/// The service auto-seeds both repartition families (base + 100% tax each).
async fn seed_template(pool: &PgPool) -> (uuid::Uuid, uuid::Uuid) {
    let company = uuid::Uuid::new_v4();
    let (_, body) = req(
        create_guarded_tax_routes(&module(pool).await),
        "POST",
        "/tax-templates",
        Some(format!(
            r#"{{"companyId":"{company}","code":"{}","name":"T {}","templateType":"sales"}}"#,
            uq("T"),
            &company.to_string()[..8]
        )),
    )
    .await;
    let tid = body
        .split("\"id\":\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    (company, uuid::Uuid::parse_str(&tid).unwrap())
}

/// Soft-delete the LIVE lines of one family (optionally only one
/// repartition_type — `None` retires the whole family). Must run inside a
/// transaction that also lands the replacement, or commits the "no live
/// repartition rows" legacy shape: the deferred family trigger only ever sees
/// transaction-final states.
async fn soft_delete_family(
    tx: &mut sqlx::PgConnection,
    template_id: uuid::Uuid,
    document_type: &str,
    only_type: Option<&str>,
) {
    sqlx::query(
        r#"UPDATE tax.tax_repartition_lines
              SET metadata = jsonb_set(COALESCE(metadata, '{}'::jsonb), '{deleted_at}', to_jsonb(NOW()))
            WHERE template_id = $1 AND document_type::text = $2
              AND ($3::text IS NULL OR repartition_type::text = $3)
              AND (metadata->>'deleted_at') IS NULL"#,
    )
    .bind(template_id)
    .bind(document_type)
    .bind(only_type)
    .execute(&mut *tx)
    .await
    .unwrap();
}

// IGC-5 (TG1): two live templates of the same type and name in one company are
// refused by the DB itself — the service pre-check is a friendly echo, not the guard.
#[tokio::test]
async fn igc5_tg1_duplicate_name_refused_by_db() {
    let pool = pool().await;
    let company = uuid::Uuid::new_v4();
    let name = format!("dup-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    sqlx::query(
        r#"INSERT INTO tax.tax_templates (id, company_id, code, name) VALUES ($1, $2, $3, $4)"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(company)
    .bind(uq("A"))
    .bind(&name)
    .execute(&pool)
    .await
    .unwrap();
    // same company, same type, same live name → the partial unique index refuses
    let err = sqlx::query(
        r#"INSERT INTO tax.tax_templates (id, company_id, code, name) VALUES ($1, $2, $3, $4)"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(company)
    .bind(uq("C"))
    .bind(&name)
    .execute(&pool)
    .await
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("duplicate key") || msg.contains("idx_tax_templates_company_type_name"),
        "TG1 must refuse a second live template with the same name; got {msg}"
    );
}

// IGC-6 (TG4): a family whose tax factors stop summing to 100 is refused —
// service arm via HTTP (422) and DB arm via a raw committing transaction.
#[tokio::test]
async fn igc6_tg4_unbalanced_family_refused() {
    let pool = pool().await;
    let (company, tid) = seed_template(&pool).await;

    // service arm: adding +50% on top of the seeded 100% cannot rebalance
    let (status, body) = req(create_guarded_tax_routes(&module(&pool).await), "POST", "/tax-repartition-lines",
        Some(format!(
            r#"{{"companyId":"{company}","templateId":"{tid}","documentType":"invoice","repartitionType":"tax","factorPercent":"50"}}"#))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {body}");

    // DB arm: raw insert + commit → deferred family trigger raises
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        r#"INSERT INTO tax.tax_repartition_lines (id, company_id, template_id, document_type, repartition_type, factor_percent)
           VALUES ($1, $2, $3, 'invoice', 'tax', 50)"#,
    )
    .bind(uuid::Uuid::new_v4()).bind(company).bind(tid)
    .execute(&mut *tx).await.unwrap();
    let err = tx.commit().await.unwrap_err();
    assert!(
        err.to_string().contains("repartition family"),
        "TG4 must refuse an unbalanced family; got {err}"
    );
}

// IGC-7 (TG4): a family with a base line but no tax line is malformed — even
// when both families are degraded symmetrically (the mirror holds; the shape does not).
#[tokio::test]
async fn igc7_tg4_missing_base_or_tax() {
    let pool = pool().await;
    let (_company, tid) = seed_template(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    for family in ["invoice", "refund"] {
        soft_delete_family(&mut tx, tid, family, Some("tax")).await;
    }
    let err = tx.commit().await.unwrap_err();
    assert!(
        err.to_string().contains("repartition family"),
        "TG4 must refuse a family without tax lines; got {err}"
    );
}

// IGC-8 (TG4): invoice and refund families are maintained together — retiring
// one whole family while the other stays live is refused.
#[tokio::test]
async fn igc8_tg4_mirror_required() {
    let pool = pool().await;
    let (_company, tid) = seed_template(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    soft_delete_family(&mut tx, tid, "refund", None).await;
    let err = tx.commit().await.unwrap_err();
    assert!(
        err.to_string().contains("maintained together"),
        "TG4 must keep both families present; got {err}"
    );
}

// IGC-9 (TG3): a cash-basis template is refused when the transition account
// cannot be VERIFIED — in a tax-only database (no accounting schema) the write
// path fails closed rather than deferring onto an unknown account.
#[tokio::test]
async fn igc9_tg3_non_reconcilable_transition_refused() {
    let pool = pool().await;
    let company = uuid::Uuid::new_v4();
    let (status, body) = req(create_guarded_tax_routes(&module(&pool).await), "POST", "/tax-templates",
        Some(format!(
            r#"{{"companyId":"{company}","code":"{}","name":"CABA","templateType":"sales","taxExigibility":"on_payment","cashBasisTransitionAccountId":"{}"}}"#,
            uq("CABA"), uuid::Uuid::new_v4()))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {body}");
    assert!(
        body.contains("caba_transition_not_reconcilable"),
        "got {body}"
    );
}

// IGC-10 (TG6): company_id is immutable on tax rows — a raw UPDATE moving a
// template or repartition line between companies is refused by the trigger.
#[tokio::test]
async fn igc10_tg6_company_immutable() {
    let pool = pool().await;
    let (_company, tid) = seed_template(&pool).await;
    let other = uuid::Uuid::new_v4();
    let err = sqlx::query("UPDATE tax.tax_templates SET company_id = $1 WHERE id = $2")
        .bind(other)
        .bind(tid)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("company_id is immutable"),
        "got {err}"
    );

    let err =
        sqlx::query("UPDATE tax.tax_repartition_lines SET company_id = $1 WHERE template_id = $2")
            .bind(other)
            .bind(tid)
            .execute(&pool)
            .await
            .unwrap_err();
    assert!(
        err.to_string().contains("company_id is immutable"),
        "got {err}"
    );
}

// IGC-11: company settings defaulting to on_payment must name a transition
// account — service arm (422) and DB CHECK arm (raw INSERT refused).
#[tokio::test]
async fn igc11_settings_caba_requires_transition() {
    let pool = pool().await;
    let company = uuid::Uuid::new_v4();
    let (status, body) = req(create_guarded_tax_routes(&module(&pool).await), "PUT", "/company-tax-settings",
        Some(format!(
            r#"{{"companyId":"{company}","roundingMethod":"round_globally","defaultExigibility":"on_payment"}}"#))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {body}");

    let err = sqlx::query(
        r#"INSERT INTO tax.company_tax_settings (id, company_id, default_exigibility)
           VALUES ($1, $2, 'on_payment')"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(company)
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("ck_company_tax_settings_caba_requires_transition"),
        "the DB CHECK must refuse on_payment without a transition account; got {err}"
    );
}

// IGC-12: the tenant fence on the new tables (ADR-0014 strict). A restricted
// role (NOSUPERUSER NOBYPASSRLS) writes only inside `app.company_id`: matching
// rows land, cross-company rows are refused at the statement, and an unset
// scope sees nothing. The repartition success leg seeds both families in one
// transaction — the deferred family trigger makes piecemeal inserts illegal,
// which is itself part of the contract being probed.
#[tokio::test]
async fn igc12_rls_new_tax_tables() {
    const ROLE: &str = "bbtax_rls_probe";
    const PWD: &str = "probe";
    static ROLE_DDL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = ROLE_DDL_LOCK.lock().await;

    let admin = pool().await;
    // Raw template insert (no service auto-seed): the restricted role below
    // seeds the families itself — through the fence, in one transaction.
    let company = uuid::Uuid::new_v4();
    let tid = uuid::Uuid::new_v4();
    sqlx::query(r#"INSERT INTO tax.tax_templates (id, company_id, code, name) VALUES ($1, $2, $3, 'RLS probe')"#)
        .bind(tid).bind(company).bind(uq("RLS"))
        .execute(&admin).await.unwrap();
    let other = uuid::Uuid::new_v4();
    let _ = sqlx::query(&format!("DROP OWNED BY {ROLE}"))
        .execute(&admin)
        .await;
    let _ = sqlx::query(&format!("DROP ROLE IF EXISTS {ROLE}"))
        .execute(&admin)
        .await;
    for stmt in [
        format!("CREATE ROLE {ROLE} LOGIN PASSWORD '{PWD}' NOSUPERUSER NOBYPASSRLS"),
        format!("GRANT USAGE ON SCHEMA tax TO {ROLE}"),
        // SELECT too: the deferred family-validation trigger counts live lines
        // as the invoking user, inside the same fence.
        format!("GRANT SELECT, INSERT ON tax.tax_repartition_lines TO {ROLE}"),
        format!("GRANT INSERT ON tax.tax_tags TO {ROLE}"),
    ] {
        sqlx::query(&stmt).execute(&admin).await.unwrap();
    }
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:5433/backbone_tax".to_string()
    });
    let host = url
        .split("@")
        .nth(1)
        .unwrap_or("localhost:5433/backbone_tax")
        .to_string();
    let restricted = sqlx::PgPool::connect(&format!("postgresql://{ROLE}:{PWD}@{host}"))
        .await
        .unwrap();

    // matching scope: a full, valid invoice+refund family lands
    let mut tx = restricted.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.company_id', $1, true)")
        .bind(company.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    for family in ["invoice", "refund"] {
        for (rtype, factor) in [("base", 100i32), ("tax", 100)] {
            sqlx::query(
                r#"INSERT INTO tax.tax_repartition_lines
                       (id, company_id, template_id, document_type, repartition_type, factor_percent)
                   VALUES ($1, $2, $3, $4::repartition_document_type, $5::repartition_type, $6)"#,
            )
            .bind(uuid::Uuid::new_v4()).bind(company).bind(tid)
            .bind(family).bind(rtype).bind(factor)
            .execute(&mut *tx).await.unwrap();
        }
    }
    tx.commit()
        .await
        .expect("same-company family insert must pass the fence");

    // cross-company: refused at the statement by WITH CHECK
    let err = sqlx::query(
        r#"INSERT INTO tax.tax_tags (id, company_id, code, name) VALUES ($1, $2, $3, 'x')"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(other)
    .bind(uq("X"))
    .execute(&restricted)
    .await
    .unwrap_err();
    assert!(err.to_string().contains("row-level security"), "got {err}");

    // unset scope: the fence sees nothing, every write is refused (fail-closed)
    let err = sqlx::query(
        r#"INSERT INTO tax.tax_tags (id, company_id, code, name) VALUES ($1, $2, $3, 'y')"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(company)
    .bind(uq("Y"))
    .execute(&restricted)
    .await
    .unwrap_err();
    assert!(err.to_string().contains("row-level security"), "got {err}");

    let _ = sqlx::query(&format!("DROP OWNED BY {ROLE}"))
        .execute(&admin)
        .await;
    let _ = sqlx::query(&format!("DROP ROLE IF EXISTS {ROLE}"))
        .execute(&admin)
        .await;
}

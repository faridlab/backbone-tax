//! Route-level probes: config writes are validated, the compute endpoints return tax lines, and
//! generic mutation is not exposed on the guarded surface. Requires DATABASE_URL (:5433).
//!
//! Tenancy: the module ships NONE (ADR-0029) — no company column, no fence declaration, no
//! tenant predicate in any statement. Two things replace the old in-module fence:
//!
//! 1. **Caller identity** rides the request as the [`OrgContext`] extension the composing
//!    service's org auth middleware inserts. The handlers require its PRESENCE (401 without
//!    one) and derive nothing tenant-shaped from it. The `companyId` fields on the request
//!    bodies are the documented LEGACY TWIN input: under a bound ambient scope the ambient
//!    scope wins, so the named value can never widen what a handler touches.
//! 2. **Row isolation** is the composing service's tenancy decorator. This suite connects as
//!    the DB owner (a superuser, whom RLS can never bind), so raw assertion SQL runs plain —
//!    and the POSTURE itself is pinned from below by IGC-12 under `SET ROLE` to a plain
//!    non-superuser: the tenant axis is gone, the RLS enable+force flags stay armed for the
//!    decorator, and until the decorator installs policies the probe role is default-denied
//!    (zero rows, writes refused) no matter what legacy variable is set.
//!
//! The compute endpoint's engine reads require the AMBIENT org scope (they fail loud as
//! `no_org_scope` otherwise), so the calculate probe runs inside [`scoped`] — the
//! single-company scope emulation (`OrgScope::for_company_unit`) exactly mirroring what a
//! composing service binds per request.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Request as HttpRequest, StatusCode};
use axum::middleware::{self, Next};
use sqlx::{Acquire, PgPool};
use tower::ServiceExt;
use uuid::Uuid;

use backbone_auth::org::OrgContext;
use backbone_orm::org_scope;
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
/// The caller identity a request carries in production (inserted by the composing service's
/// org auth layer). The module's handlers only require its PRESENCE — the `OrgContext`
/// extractor rejects a request without one 401 — and derive nothing tenant-shaped from it.
fn caller() -> OrgContext {
    OrgContext {
        acting_unit_id: Uuid::new_v4(),
        entitled_units: vec![],
        legacy_company_id: None,
        user_id: Uuid::new_v4().to_string(),
    }
}
/// Wrap the router with the extension the host auth stack provides in production.
fn with_caller(router: axum::Router, org: OrgContext) -> axum::Router {
    router.layer(middleware::from_fn(
        move |mut req: Request, next: Next| {
            let org = org.clone();
            async move {
                req.extensions_mut().insert(org);
                next.run(req).await
            }
        },
    ))
}
/// The guarded composition + caller extension — the mounting a composing service uses.
fn guarded(m: &TaxModule) -> axum::Router {
    with_caller(create_guarded_tax_routes(m), caller())
}
/// Run `f` with an ambient org scope bound — the single-company emulation of what a composing
/// service resolves and binds per request. The engine's reads pick the scope off here.
async fn scoped<F, R>(pool: &PgPool, company: Uuid, f: F) -> R
where
    F: std::future::Future<Output = R>,
{
    org_scope::with_org_request_scope(
        pool,
        org_scope::OrgScope::for_company_unit(company),
        f,
    )
    .await
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
            HttpRequest::builder()
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
    format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8])
}

// IGC-1: generic bulk create on a config entity is not exposed on the guarded surface.
#[tokio::test]
async fn guarded_routes_lock_generic_template_bulk() {
    let pool = pool().await;
    let (status, _) = req(
        guarded(&module(&pool).await),
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
        guarded(&module(&pool).await),
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
    let app = guarded(&module(&pool).await);
    let (_, body) = req(
        app,
        "POST",
        "/tax-templates",
        Some(format!(
            r#"{{"companyId":"{company}","code":"{}","name":"{name}","templateType":"sales"}}"#,
            uq("T"),
            name = uq("N")
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
        guarded(&module(&pool).await),
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
    // seed a template + row via the guarded write surface (the companyId on the bodies is the
    // documented LEGACY TWIN input; the writes run under the caller's ambient scope)
    let (_, tbody) = req(
        guarded(&module(&pool).await),
        "POST",
        "/tax-templates",
        Some(format!(
            r#"{{"companyId":"{company}","code":"{}","name":"{name}","templateType":"sales"}}"#,
            uq("C"),
            name = uq("PPN")
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
    req(guarded(&module(&pool).await), "POST", "/tax-template-rows",
        Some(format!(r#"{{"companyId":"{company}","templateId":"{tid}","rate":"11","effectiveFrom":"2022-04-01"}}"#))).await;

    let calc = format!(r#"{{"templateId":"{tid}","baseAmount":"1000000","onDate":"2026-07-03"}}"#);
    // The compute endpoint's engine reads the AMBIENT org scope (set in deployment by the
    // composing service's scope middleware), not the body — wrap the call in `scoped` so the
    // engine sees the same unit the rows were created under (else it fails loud as
    // no_org_scope → 500).
    let (status, body) = scoped(
        &pool,
        company,
        req(
            guarded(&module(&pool).await),
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
// DB-guard probes (TG1/TG3/TG4) and the strip posture. The HTTP legs pin the
// friendly service arms; the raw-SQL legs pin that the invariants hold even for
// writers that bypass the service entirely. The tenancy-era DB guards (the
// per-unit name unique and the company-immutable trigger) are decorator
// posture now: the module-level arms that remain are the DOMAIN invariants.
// ─────────────────────────────────────────────────────────────────────────────

/// Create a live template via the guarded surface; returns (company, template id).
/// The service auto-seeds both repartition families (base + 100% tax each).
async fn seed_template(pool: &PgPool) -> (uuid::Uuid, uuid::Uuid) {
    let company = uuid::Uuid::new_v4();
    let (_, body) = req(
        guarded(&module(pool).await),
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

// IGC-5 (TG1): two live templates of the same type and name are refused by the
// service's friendly pre-check. The per-unit DB unique that used to arbitrate
// this is decorator posture now (the strip drops it — a tenant-free variant
// would falsely collide two units' rows), so the guard that remains in-module
// is the pre-check arm.
#[tokio::test]
async fn igc5_tg1_duplicate_name_refused_by_service() {
    let pool = pool().await;
    let company = uuid::Uuid::new_v4();
    let name = format!("dup-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let app = guarded(&module(&pool).await);
    let (status, _) = req(
        app,
        "POST",
        "/tax-templates",
        Some(format!(
            r#"{{"companyId":"{company}","code":"{}","name":"{name}","templateType":"sales"}}"#,
            uq("A")
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "the first create must land");

    // same type, same live name → the friendly pre-check refuses
    let (status, body) = req(
        guarded(&module(&pool).await),
        "POST",
        "/tax-templates",
        Some(format!(
            r#"{{"companyId":"{company}","code":"{}","name":"{name}","templateType":"sales"}}"#,
            uq("C")
        )),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "TG1 must refuse a second live template with the same name; got {body}"
    );
    assert!(
        body.contains("duplicate_name"),
        "expected the duplicate_name code; got {body}"
    );
}

// IGC-6 (TG4): a family whose tax factors stop summing to 100 is refused —
// service arm via HTTP (422) and DB arm via a raw committing transaction.
#[tokio::test]
async fn igc6_tg4_unbalanced_family_refused() {
    let pool = pool().await;
    let (company, tid) = seed_template(&pool).await;

    // service arm: adding +50% on top of the seeded 100% cannot rebalance
    let (status, body) = req(guarded(&module(&pool).await), "POST", "/tax-repartition-lines",
        Some(format!(
            r#"{{"companyId":"{company}","templateId":"{tid}","documentType":"invoice","repartitionType":"tax","factorPercent":"50"}}"#))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {body}");

    // DB arm: raw insert + commit → deferred family trigger raises
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        r#"INSERT INTO tax.tax_repartition_lines (id, template_id, document_type, repartition_type, factor_percent)
           VALUES ($1, $2, 'invoice', 'tax', 50)"#,
    )
    .bind(uuid::Uuid::new_v4()).bind(tid)
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
    let (status, body) = req(guarded(&module(&pool).await), "POST", "/tax-templates",
        Some(format!(
            r#"{{"companyId":"{company}","code":"{}","name":"CABA","templateType":"sales","taxExigibility":"on_payment","cashBasisTransitionAccountId":"{}"}}"#,
            uq("CABA"), uuid::Uuid::new_v4()))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {body}");
    assert!(
        body.contains("caba_transition_not_reconcilable"),
        "got {body}"
    );
}

// IGC-10 (TG6) is retired with the tenancy strip: it pinned the company_id-is-
// immutable guard trigger, and both the column and the trigger are tenancy
// artifacts the module no longer ships (the strip migration drops them). The
// posture that replaces it is pinned by IGC-12 below.

// IGC-11: company settings defaulting to on_payment must name a transition
// account — service arm (422) and DB CHECK arm (raw INSERT refused).
#[tokio::test]
async fn igc11_settings_caba_requires_transition() {
    let pool = pool().await;
    let company = uuid::Uuid::new_v4();
    let (status, body) = req(guarded(&module(&pool).await), "PUT", "/company-tax-settings",
        Some(format!(
            r#"{{"companyId":"{company}","roundingMethod":"round_globally","defaultExigibility":"on_payment"}}"#))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {body}");

    let err = sqlx::query(
        r#"INSERT INTO tax.company_tax_settings (id, default_exigibility)
           VALUES ($1, 'on_payment')"#,
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("ck_company_tax_settings_caba_requires_transition"),
        "the DB CHECK must refuse on_payment without a transition account; got {err}"
    );
}

// ─── IGC-12: the strip posture — tenant axis gone, fence flags stay armed ─────
//
// The module ships no tenant axis (ADR-0029): no company_id column, no legacy company-isolation
// policies, no company-leading indexes. Row-level security stays ENABLED + FORCED — the
// composing service's tenancy decorator owns the policies that make it bite. Probed from
// below (SET ROLE to a plain non-superuser, whom RLS does bind): with no policy admitting
// it, the role sees ZERO rows and cannot WRITE, no matter what legacy variable is set —
// default deny. (The pre-strip probe asserted the module's own per-company fence; that fence
// is composition posture now, and what the module itself must guarantee is the fail-closed
// shape below.)
#[tokio::test]
async fn igc12_rls_new_tax_tables() {
    let pool = pool().await;

    // Seed one live tag through the owner pool so the default-deny read below is
    // meaningful (a row exists; the probe role just cannot see it).
    sqlx::query(r#"INSERT INTO tax.tax_tags (id, code, name) VALUES ($1, $2, 'posture probe')"#)
        .bind(uuid::Uuid::new_v4())
        .bind(uq("POSTURE"))
        .execute(&pool)
        .await
        .unwrap();

    // ── schema posture: the tenant axis is gone, the fence flags stay armed ──
    for table in [
        "tax_categories",
        "tax_templates",
        "tax_template_rows",
        "withholding_categories",
        "company_tax_settings",
        "tax_tags",
        "tax_repartition_lines",
        "tax_transactions",
        "efaktur_documents",
        "tax_filing_periods",
    ] {
        let company_col: bool = sqlx::query_scalar(
            r#"SELECT EXISTS (
                   SELECT 1 FROM information_schema.columns
                   WHERE table_schema = 'tax' AND table_name = $1
                     AND column_name = 'company_id'
               )"#,
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!company_col, "tax.{table} must not carry a company_id column");

        let legacy_policies: i64 = sqlx::query_scalar(
            r#"SELECT count(*) FROM pg_policies
               WHERE schemaname = 'tax' AND tablename = $1
                 AND policyname LIKE '%company_isolation'"#,
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            legacy_policies, 0,
            "tax.{table} must not carry a legacy company-isolation policy"
        );

        let company_indexes: i64 = sqlx::query_scalar(
            r#"SELECT count(*) FROM pg_indexes
               WHERE schemaname = 'tax' AND tablename = $1
                 AND indexname LIKE '%company_id%'"#,
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(company_indexes, 0, "tax.{table} must not carry company-leading indexes");

        let (rls_enabled, rls_forced): (bool, bool) = sqlx::query_as(
            r#"SELECT relrowsecurity, relforcerowsecurity
               FROM pg_class
               WHERE oid = to_regclass($1)"#,
        )
        .bind(format!("tax.{table}"))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(rls_enabled, "tax.{table} must keep row-level security ENABLED (the decorator owns the policies)");
        assert!(rls_forced, "tax.{table} must keep row-level security FORCED (the decorator owns the policies)");
    }

    // ── default-deny, probed from below ─────────────────────────────────────

    // The probe role: non-superuser, minimal grants, idempotent (NOLOGIN — privileges from a
    // prior run make DROP ROLE refuse, so the family pattern creates-if-absent instead).
    // The advisory lock serializes the create/grant/set-role window so a fresh database
    // cannot race two CREATE ROLEs.
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(814402)")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        r#"DO $$ BEGIN
               IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'bbtax_probe_rls') THEN
                   CREATE ROLE bbtax_probe_rls NOLOGIN;
               END IF;
           END $$"#,
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query("GRANT USAGE ON SCHEMA tax TO bbtax_probe_rls")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("GRANT SELECT, INSERT ON ALL TABLES IN SCHEMA tax TO bbtax_probe_rls")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("SET ROLE bbtax_probe_rls")
        .execute(&mut *conn)
        .await
        .unwrap();

    // With no policy admitting it, the role sees nothing — even though the owner-seeded row
    // exists. Setting the legacy company variable resurrects nothing: no policy reads it
    // anymore (the decorator's org-scoped policies will, once composed). One explicit
    // transaction; a SET/RESET pairing is session-level, but keeping the read transactional
    // matches the family probe pattern.
    let mut tx = conn.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.company_id', $1, true)")
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM tax.tax_tags")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(n, 0, "a role no policy admits sees zero rows, legacy variable or not");
    tx.rollback().await.unwrap();

    // The role holds the GRANTs but no policy admits its write — default deny refuses the
    // INSERT even though the table carries no tenant column at all.
    let refused = sqlx::query(
        r#"INSERT INTO tax.tax_tags (id, code, name) VALUES ($1, $2, 'denied write')"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(uq("DENIED"))
    .execute(&mut *conn)
    .await;
    let err = match refused {
        Err(e) => e,
        Ok(_) => panic!("a role no policy admits must not write (default deny)"),
    };
    assert!(
        err.as_database_error().is_some(),
        "policy denial, not a transport error: {err}"
    );

    sqlx::query("RESET ROLE").execute(&mut *conn).await.unwrap();
    sqlx::query("SELECT pg_advisory_unlock(814402)")
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);

    // The owner pool still sees its row — the denial above is the missing policy, not an
    // empty database. And the owner role writes fine: no tenant axis on the module's own
    // write path.
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM tax.tax_tags")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(n > 0, "the owner role must still see its seeded row");
    let write = sqlx::query(r#"INSERT INTO tax.tax_tags (id, code, name) VALUES ($1, $2, 'owner write')"#)
        .bind(uuid::Uuid::new_v4())
        .bind(uq("OWNERW"))
        .execute(&pool)
        .await;
    assert!(write.is_ok(), "the owner role writes without any tenant axis: {write:?}");
}

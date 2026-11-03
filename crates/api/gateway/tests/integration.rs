//! Integration Control Plane API: connector registry, connections, secret
//! redaction, tenant isolation and RBAC. Requires DATABASE_URL; skips when unset.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use talentos_authz::jwt;
use talentos_db::TenantPool;
use talentos_gateway::{router, state::AppState};
use talentos_kernel::{TenantContext, TenantId, UserId};
use tower::ServiceExt;
use uuid::Uuid;

const JWT_SECRET: &[u8] = b"test-secret-for-integrations-32-bytes-ok";

fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

fn app_state(pool: TenantPool) -> AppState {
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let pool = Arc::new(pool);
    AppState {
        pool: pool.clone(),
        auth_pool: pool.pool().clone(),
        jwt_secret: Arc::from(JWT_SECRET),
        redis: fred::prelude::Client::new(
            fred::prelude::Config::from_url(&redis_url).expect("REDIS_URL"),
            None,
            None,
            None,
        ),
        access_ttl_secs: 900,
        refresh_ttl_days: 7,
        allowed_origins: Vec::new(),
        graphql_schema: Arc::new(talentos_graphql::build_schema()),
        memory: Arc::new(talentos_memory::MemoryStore::new(pool.clone(), None)),
        pii: None,
        base_url: None,
        http: reqwest::Client::new(),
        oidc_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        secrets: None,
        started_at: std::time::Instant::now(),
    }
}

async fn seed_tenant(raw: &PgPool) -> Uuid {
    let tenant_id = Uuid::new_v4();
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'Integration Test', $2)")
        .bind(tenant_id)
        .bind(format!("integration-test-{}", &tenant_id.to_string()[..8]))
        .execute(&mut *tx)
        .await
        .expect("tenant");
    tx.commit().await.expect("commit");
    tenant_id
}

fn bearer(tenant_id: Uuid, roles: &[&str]) -> String {
    let ctx = TenantContext {
        tenant_id: TenantId(tenant_id),
        entity_id: None,
        user_id: UserId(Uuid::new_v4()),
        roles: roles.iter().map(|r| r.to_string()).collect(),
    };
    format!(
        "Bearer {}",
        jwt::issue_access_token(&ctx, JWT_SECRET, 900).expect("token")
    )
}

async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    auth: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, auth);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let req = builder
        .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("oneshot");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

fn connector_body(version: &str) -> Value {
    json!({
        "connector_id": "talentos.rest",
        "name": "REST / OpenAPI",
        "version": version,
        "protocol": "rest",
        "manifest": {
            "id": "talentos.rest",
            "version": version,
            "protocol": "rest",
            "operations": [{"name": "probe", "kind": "probe"}]
        },
        "risk": "low"
    })
}

#[tokio::test]
async fn connector_lifecycle_listing_and_pagination() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_id = seed_tenant(&raw).await;
    let auth = bearer(tenant_id, &["hr_admin"]);
    let app = router::build(app_state(pool));

    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/integrations/connectors",
        &auth,
        Some(connector_body("1.0.0")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["connector"]["status"], "draft");

    let (status, body) = call(
        &app,
        Method::GET,
        "/api/v1/integrations/connectors?limit=1",
        &auth,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["connector_id"], "talentos.rest");
    assert_eq!(body["next_offset"], Value::Null);

    let (status, body) = call(
        &app,
        Method::GET,
        "/api/v1/integrations/connectors/talentos.rest?version=1.0.0",
        &auth,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["connector"]["version"], "1.0.0");

    let (status, body) = call(
        &app,
        Method::PATCH,
        "/api/v1/integrations/connectors/talentos.rest?version=1.0.0",
        &auth,
        Some(json!({ "name": "REST connector", "risk": "medium" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["connector"]["name"], "REST connector");

    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/integrations/connectors/talentos.rest/publish",
        &auth,
        Some(json!({ "version": "1.0.0" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["connector"]["status"], "published");

    let (status, body) = call(
        &app,
        Method::PATCH,
        "/api/v1/integrations/connectors/talentos.rest?version=1.0.0",
        &auth,
        Some(json!({ "name": "Nope" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "connector_published_immutable");

    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/integrations/connectors/talentos.rest/publish",
        &auth,
        Some(json!({ "version": "1.0.0" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "already_published");

    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/integrations/connectors/talentos.rest/disable",
        &auth,
        Some(json!({ "version": "1.0.0" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["connector"]["status"], "disabled");

    let (status, body) = call(
        &app,
        Method::GET,
        "/api/v1/integrations/connectors",
        &auth,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 0);
    assert_eq!(body["items"].as_array().map(Vec::len), Some(0));

    let (status, body) = call(
        &app,
        Method::GET,
        "/api/v1/integrations/connectors?include_disabled=true",
        &auth,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);

    let (status, _) = call(
        &app,
        Method::GET,
        "/api/v1/integrations/connectors/talentos.rest?version=9.9.9",
        &auth,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn connections_crud_and_secret_ref_never_echoed() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_id = seed_tenant(&raw).await;
    let auth = bearer(tenant_id, &["hr_admin"]);
    let app = router::build(app_state(pool));

    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/integrations/connectors",
        &auth,
        Some(connector_body("1.0.0")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let secret = "secret/data/talentos/tenants/x/connectors/rest";
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/integrations/connections",
        &auth,
        Some(json!({
            "connector_id": "talentos.rest",
            "connector_version": "1.0.0",
            "name": "HRIS prod",
            "endpoint": "https://api.example.com",
            "secret_ref": secret
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["connection"]["has_secret"], true);
    assert!(
        !body.to_string().contains(secret),
        "secret_ref must never be echoed: {body}"
    );
    let connection_id = body["connection"]["id"].as_str().expect("id").to_owned();

    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/integrations/connections",
        &auth,
        Some(json!({
            "connector_id": "talentos.rest",
            "connector_version": "1.0.0",
            "name": "HRIS staging",
            "endpoint": "https://staging.example.com"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["connection"]["has_secret"], false);

    let (status, body) = call(
        &app,
        Method::GET,
        "/api/v1/integrations/connections",
        &auth,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2);
    assert!(!body.to_string().contains(secret));
    assert!(!body.to_string().contains("secret_ref"));

    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("/api/v1/integrations/connections/{connection_id}"),
        &auth,
        Some(json!({ "endpoint": "https://api2.example.com", "state": "disabled" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["connection"]["endpoint"], "https://api2.example.com");
    assert_eq!(body["connection"]["state"], "disabled");

    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/api/v1/integrations/connections/{connection_id}/test"),
        &auth,
        Some(json!({ "ok": true, "latency_ms": 12 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["connection"]["last_test"]["ok"], true);
    assert!(body["connection"]["last_verified_at"].is_string());

    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/api/v1/integrations/connections/{connection_id}"),
        &auth,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/integrations/connections/{connection_id}"),
        &auth,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/api/v1/integrations/connections/{connection_id}"),
        &auth,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tenant_isolation_and_rbac() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_a = seed_tenant(&raw).await;
    let tenant_b = seed_tenant(&raw).await;
    let app = router::build(app_state(pool));

    let hr_a = bearer(tenant_a, &["hr_admin"]);
    let hr_b = bearer(tenant_b, &["hr_admin"]);
    let employee = bearer(tenant_a, &["employee"]);
    let payroll = bearer(tenant_a, &["payroll_manager"]);

    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/integrations/connectors",
        &hr_a,
        Some(connector_body("1.0.0")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // Tenant B cannot see or fetch tenant A's connector.
    let (status, body) = call(
        &app,
        Method::GET,
        "/api/v1/integrations/connectors?include_disabled=true",
        &hr_b,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 0);
    let (status, _) = call(
        &app,
        Method::GET,
        "/api/v1/integrations/connectors/talentos.rest?version=1.0.0",
        &hr_b,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Employee cannot read or write.
    let (status, _) = call(
        &app,
        Method::GET,
        "/api/v1/integrations/connectors",
        &employee,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        Method::POST,
        "/api/v1/integrations/connectors",
        &employee,
        Some(connector_body("2.0.0")),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Payroll manager may read but not write.
    let (status, _) = call(
        &app,
        Method::GET,
        "/api/v1/integrations/connectors",
        &payroll,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &app,
        Method::POST,
        "/api/v1/integrations/connectors",
        &payroll,
        Some(connector_body("2.0.0")),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Tenant B cannot fetch tenant A's connection either.
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/integrations/connections",
        &hr_a,
        Some(json!({
            "connector_id": "talentos.rest",
            "connector_version": "1.0.0",
            "name": "A only",
            "endpoint": "https://a.example.com"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let connection_id = body["connection"]["id"].as_str().expect("id");
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/api/v1/integrations/connections/{connection_id}"),
        &hr_b,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

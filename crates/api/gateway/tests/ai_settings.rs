//! AI settings API: key never leaks, RBAC, and fail-closed without OpenBao.
//! Requires DATABASE_URL; skips when unset.

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
use talentos_gateway::{router, secrets::SecretsClient, state::AppState};
use talentos_kernel::{TenantContext, TenantId, UserId};
use tower::ServiceExt;
use uuid::Uuid;

const JWT_SECRET: &[u8] = b"test-secret-for-ai-settings-32bytes-ok!!";
const PROVIDER_KEY: &str = "sk-super-secret-provider-key";

fn database_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

fn app_state(pool: TenantPool, secrets: Option<Arc<SecretsClient>>) -> AppState {
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
        secrets,
        started_at: std::time::Instant::now(),
    }
}

async fn seed(raw: &PgPool) -> Uuid {
    let tenant_id = Uuid::new_v4();
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'AI Test', $2)")
        .bind(tenant_id)
        .bind(format!("ai-test-{}", &tenant_id.to_string()[..8]))
        .execute(&mut *tx)
        .await
        .expect("tenant");
    sqlx::query(
        "INSERT INTO ai_settings (tenant_id, model, api_key_set, secret_ref)
         VALUES ($1, 'seeded-model', true, $2)",
    )
    .bind(tenant_id)
    .bind(format!("secret/data/talentos/tenants/{tenant_id}/ai"))
    .execute(&mut *tx)
    .await
    .expect("ai settings");
    tx.commit().await.expect("commit");
    tenant_id
}

async fn stored_model(raw: &PgPool, tenant_id: Uuid) -> String {
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query("SELECT model FROM ai_settings WHERE tenant_id = $1")
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await
        .expect("row");
    let _ = tx.rollback().await;
    use sqlx::Row;
    row.get("model")
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

/// Minimal KV v2 stand-in: accepts any write and returns 204.
async fn fake_openbao() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let app = Router::new().fallback(|| async { StatusCode::NO_CONTENT });
        axum::serve(listener, app).await.expect("serve");
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn ai_settings_never_return_the_key() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_id = seed(&raw).await;

    let openbao = fake_openbao().await;
    let state = app_state(
        pool,
        Some(Arc::new(SecretsClient::new(openbao, "test-token"))),
    );
    let app = router::build(state);
    let admin = bearer(tenant_id, &["hr_admin"]);
    let employee = bearer(tenant_id, &["employee"]);

    // GET: non-secret fields only.
    let (status, body) = call(&app, Method::GET, "/api/v1/ai/settings", &admin, None).await;
    assert_eq!(status, StatusCode::OK, "GET failed: {body}");
    assert_eq!(body["api_key_set"], true);
    assert_eq!(body["secrets_backend"], "openbao");
    assert!(body.get("secret_ref").is_none());
    assert!(body.get("api_key").is_none());
    assert!(!body.to_string().contains("secret/data/talentos"));

    // PUT with a key echoes no key and flips api_key_set.
    let (status, body) = call(
        &app,
        Method::PUT,
        "/api/v1/ai/settings",
        &admin,
        Some(json!({
            "provider": "huakati",
            "base_url": null,
            "model": "test-model",
            "mcp_enabled": false,
            "skills": [],
            "api_key": PROVIDER_KEY,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "PUT failed: {body}");
    assert_eq!(body["api_key_set"], true);
    assert_eq!(body["model"], "test-model");
    assert!(!body.to_string().contains(PROVIDER_KEY));
    assert!(body.get("api_key").is_none());

    // A following GET must still omit it.
    let (_, body) = call(&app, Method::GET, "/api/v1/ai/settings", &admin, None).await;
    assert!(!body.to_string().contains(PROVIDER_KEY));

    // Employee is denied both read and write.
    for method in [Method::GET, Method::PUT] {
        let payload = (method == Method::PUT).then(|| {
            json!({
                "provider": "huakati",
                "base_url": null,
                "model": "nope",
                "mcp_enabled": true,
                "skills": [],
            })
        });
        let (status, _) = call(&app, method, "/api/v1/ai/settings", &employee, payload).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .execute(&raw)
        .await;
}

#[tokio::test]
async fn ai_settings_fail_closed_without_openbao() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_id = seed(&raw).await;
    let app = router::build(app_state(pool, None));
    let admin = bearer(tenant_id, &["hr_admin"]);

    // A key with no backend is a 503 and writes nothing.
    let (status, body) = call(
        &app,
        Method::PUT,
        "/api/v1/ai/settings",
        &admin,
        Some(json!({
            "provider": "huakati",
            "base_url": null,
            "model": "should-not-persist",
            "mcp_enabled": true,
            "skills": [],
            "api_key": PROVIDER_KEY,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"], "ai_unavailable");
    assert_eq!(stored_model(&raw, tenant_id).await, "seeded-model");

    // Connection test without a backend is also 503.
    let (status, body) = call(&app, Method::POST, "/api/v1/ai/settings/test", &admin, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"], "ai_unavailable");

    let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .execute(&raw)
        .await;
}

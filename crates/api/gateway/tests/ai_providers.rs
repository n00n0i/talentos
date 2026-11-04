//! Multi-provider AI API: presets, provider CRUD, model fetch/cache, model roles
//! and RBAC. Requires DATABASE_URL; skips when unset.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    response::IntoResponse,
    Json, Router,
};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use talentos_authz::jwt;
use talentos_db::TenantPool;
use talentos_gateway::{router, secrets::SecretsClient, state::AppState};
use talentos_kernel::{TenantContext, TenantId, UserId};
use tower::ServiceExt;
use uuid::Uuid;

const JWT_SECRET: &[u8] = b"test-secret-for-ai-providers-32-bytes-ok";
const API_KEY: &str = "sk-super-secret-provider-key";

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

async fn seed_tenant(raw: &PgPool) -> Uuid {
    let tenant_id = Uuid::new_v4();
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'Provider Test', $2)")
        .bind(tenant_id)
        .bind(format!("provider-test-{}", &tenant_id.to_string()[..8]))
        .execute(&mut *tx)
        .await
        .expect("tenant");
    tx.commit().await.expect("commit");
    tenant_id
}

/// Insert a provider directly (no key) and return its id.
async fn seed_provider(raw: &PgPool, tenant_id: Uuid, base_url: &str) -> Uuid {
    let provider_id = Uuid::new_v4();
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "INSERT INTO ai_providers (id, tenant_id, name, kind, base_url)
         VALUES ($1, $2, 'Local', 'ollama', $3)",
    )
    .bind(provider_id)
    .bind(tenant_id)
    .bind(base_url)
    .execute(&mut *tx)
    .await
    .expect("provider");
    tx.commit().await.expect("commit");
    provider_id
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

/// Minimal KV v2 stand-in: accepts key writes and deletes.
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

/// OpenAI-compatible `/models` upstream. Records whether an Authorization
/// header reached it.
async fn fake_models_upstream() -> (String, Arc<std::sync::Mutex<Option<String>>>) {
    let seen_auth = Arc::new(std::sync::Mutex::new(None));
    let recorder = seen_auth.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let app = Router::new().fallback(move |headers: axum::http::HeaderMap| {
            *recorder.lock().unwrap() = headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            async move {
                Json(json!({
                    "object": "list",
                    "data": [
                        {"id": "llama3", "created": 1700000000, "owned_by": "library"},
                        {"id": "bge-m3", "created": 0, "owned_by": "ollama"},
                    ]
                }))
                .into_response()
            }
        });
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://{addr}/v1"), seen_auth)
}

/// A port nothing listens on.
async fn closed_base_url() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(listener);
    format!("http://{addr}/v1")
}

async fn cached_models(raw: &PgPool, tenant_id: Uuid, provider_id: Uuid) -> Value {
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query("SELECT models_cache FROM ai_providers WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id)
        .bind(provider_id)
        .fetch_one(&mut *tx)
        .await
        .expect("row");
    let cache = row.get("models_cache");
    let _ = tx.rollback().await;
    cache
}

#[tokio::test]
async fn presets_include_ollama_without_key_requirement() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_id = seed_tenant(&raw).await;
    let app = router::build(app_state(pool, None));

    let (status, body) = call(
        &app,
        Method::GET,
        "/api/v1/ai/provider-presets",
        &bearer(tenant_id, &["hr_admin"]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ollama = body["presets"]
        .as_array()
        .expect("presets")
        .iter()
        .find(|preset| preset["kind"] == "ollama")
        .expect("ollama preset");
    assert_eq!(ollama["key_required"], false);

    let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .execute(&raw)
        .await;
}

#[tokio::test]
async fn provider_crud_never_returns_the_key() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_id = seed_tenant(&raw).await;
    let openbao = fake_openbao().await;
    let app = router::build(app_state(
        pool,
        Some(Arc::new(SecretsClient::new(openbao, "test-token"))),
    ));
    let admin = bearer(tenant_id, &["hr_admin"]);

    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/ai/providers",
        &admin,
        Some(json!({
            "name": "OpenAI",
            "kind": "openai",
            "base_url": "https://api.openai.com/v1/",
            "api_key": API_KEY,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["has_key"], true);
    assert_eq!(body["base_url"], "https://api.openai.com/v1");
    assert!(!body.to_string().contains(API_KEY));
    let provider_id = body["id"].as_str().expect("id").to_string();

    let (status, body) = call(&app, Method::GET, "/api/v1/ai/providers", &admin, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["providers"].as_array().unwrap().len(), 1);
    assert_eq!(body["providers"][0]["has_key"], true);
    assert!(!body.to_string().contains(API_KEY));
    assert!(body.get("api_key").is_none());

    let (status, body) = call(
        &app,
        Method::PUT,
        &format!("/api/v1/ai/providers/{provider_id}"),
        &admin,
        Some(json!({"name": "OpenAI prod", "enabled": false})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "OpenAI prod");
    assert_eq!(body["enabled"], false);
    assert!(!body.to_string().contains(API_KEY));

    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/v1/ai/providers/{provider_id}"),
        &admin,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, body) = call(&app, Method::GET, "/api/v1/ai/providers", &admin, None).await;
    assert_eq!(body["providers"].as_array().unwrap().len(), 0);

    let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .execute(&raw)
        .await;
}

#[tokio::test]
async fn fetch_models_normalises_and_caches_without_key_header() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_id = seed_tenant(&raw).await;
    let (upstream, seen_auth) = fake_models_upstream().await;
    let provider_id = seed_provider(&raw, tenant_id, &upstream).await;
    let app = router::build(app_state(pool, None));

    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/api/v1/ai/providers/{provider_id}/models"),
        &bearer(tenant_id, &["hr_admin"]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["models"][0]["id"], "llama3");
    assert_eq!(body["models"][0]["owned_by"], "library");
    assert_eq!(body["models"][1]["id"], "bge-m3");
    assert_eq!(body["models"][1]["created"], 0);
    assert_eq!(
        seen_auth.lock().unwrap().as_deref(),
        None,
        "a provider without api_key_ref must not send Authorization"
    );

    let cache = cached_models(&raw, tenant_id, provider_id).await;
    assert_eq!(cache.as_array().unwrap().len(), 2);
    assert_eq!(cache[0]["id"], "llama3");

    let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .execute(&raw)
        .await;
}

#[tokio::test]
async fn fetch_models_unreachable_provider_is_502() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_id = seed_tenant(&raw).await;
    let base_url = closed_base_url().await;
    let provider_id = seed_provider(&raw, tenant_id, &base_url).await;
    let app = router::build(app_state(pool, None));

    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/api/v1/ai/providers/{provider_id}/models"),
        &bearer(tenant_id, &["hr_admin"]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"], "provider_unreachable");
    assert!(!body.to_string().contains(API_KEY));

    let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .execute(&raw)
        .await;
}

#[tokio::test]
async fn model_role_upsert_and_get() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_id = seed_tenant(&raw).await;
    let provider_id = seed_provider(&raw, tenant_id, "http://127.0.0.1:9/v1").await;
    let app = router::build(app_state(pool, None));
    let admin = bearer(tenant_id, &["hr_admin"]);

    let (status, body) = call(
        &app,
        Method::PUT,
        "/api/v1/ai/model-roles/chat",
        &admin,
        Some(json!({"provider_id": provider_id, "model": "llama3"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["model"], "llama3");

    // Upsert replaces the same role.
    let (status, body) = call(
        &app,
        Method::PUT,
        "/api/v1/ai/model-roles/chat",
        &admin,
        Some(json!({
            "provider_id": provider_id,
            "model": "llama3.1",
            "params": {"temperature": 0.1},
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = call(&app, Method::GET, "/api/v1/ai/model-roles", &admin, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["chat"]["model"], "llama3.1");
    assert_eq!(body["chat"]["params"]["temperature"], 0.1);
    assert!(body["embedding"].is_null());
    assert!(body["ocr"].is_null());
    assert!(body["vision"].is_null());
    assert!(body["graph_extract"].is_null());

    let (status, body) = call(
        &app,
        Method::PUT,
        "/api/v1/ai/model-roles/nope",
        &admin,
        Some(json!({"provider_id": provider_id, "model": "x"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, _) = call(
        &app,
        Method::PUT,
        "/api/v1/ai/model-roles/chat",
        &admin,
        Some(json!({"provider_id": Uuid::new_v4(), "model": "x"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .execute(&raw)
        .await;
}

#[tokio::test]
async fn employee_is_forbidden_from_provider_settings() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_id = seed_tenant(&raw).await;
    let provider_id = seed_provider(&raw, tenant_id, "http://127.0.0.1:9/v1").await;
    let app = router::build(app_state(pool, None));
    let employee = bearer(tenant_id, &["employee"]);

    for (method, uri, body) in [
        (Method::GET, "/api/v1/ai/providers".to_string(), None),
        (
            Method::POST,
            "/api/v1/ai/providers".to_string(),
            Some(json!({"name": "x", "kind": "openai", "base_url": "https://x/v1"})),
        ),
        (Method::GET, "/api/v1/ai/model-roles".to_string(), None),
        (
            Method::PUT,
            "/api/v1/ai/model-roles/chat".to_string(),
            Some(json!({"provider_id": provider_id, "model": "x"})),
        ),
        (Method::GET, "/api/v1/ai/provider-presets".to_string(), None),
    ] {
        let (status, _) = call(&app, method, &uri, &employee, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
    }

    let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .execute(&raw)
        .await;
}

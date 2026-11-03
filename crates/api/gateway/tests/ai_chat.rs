//! Chat endpoint: RBAC, fail-closed without a secret, resolves the `chat` role,
//! and a grounded reply against a fake OpenAI-compatible provider.
//! Requires DATABASE_URL; skips when unset.

use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    response::IntoResponse,
    Json, Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use talentos_authz::jwt;
use talentos_db::TenantPool;
use talentos_gateway::{router, secrets::SecretsClient, state::AppState};
use talentos_kernel::{TenantContext, TenantId, UserId};
use tower::ServiceExt;
use uuid::Uuid;

const JWT_SECRET: &[u8] = b"test-secret-for-ai-chat-endpoint-32-bytes";
const CHAT_PATH: &str = "/api/v1/ai/chat";

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

/// Tenant + one provider with a key + a `chat` role pointing at `model`.
async fn seed(raw: &PgPool, base_url: &str, model: &str) -> Uuid {
    let tenant_id = Uuid::new_v4();
    let provider_id = Uuid::new_v4();
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'Chat Test', $2)")
        .bind(tenant_id)
        .bind(format!("chat-test-{}", &tenant_id.to_string()[..8]))
        .execute(&mut *tx)
        .await
        .expect("tenant");
    sqlx::query(
        "INSERT INTO ai_providers (id, tenant_id, name, kind, base_url, api_key_ref)
         VALUES ($1, $2, 'Test', 'custom', $3, $4)",
    )
    .bind(provider_id)
    .bind(tenant_id)
    .bind(base_url)
    .bind(format!(
        "secret/data/talentos/tenants/{tenant_id}/providers/{provider_id}"
    ))
    .execute(&mut *tx)
    .await
    .expect("provider");
    sqlx::query(
        "INSERT INTO ai_model_roles (tenant_id, role, provider_id, model, params)
         VALUES ($1, 'chat', $2, $3, '{}')",
    )
    .bind(tenant_id)
    .bind(provider_id)
    .bind(model)
    .execute(&mut *tx)
    .await
    .expect("chat role");
    tx.commit().await.expect("commit");
    tenant_id
}

async fn conversation_count(raw: &PgPool, tenant_id: Uuid) -> i64 {
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ai_conversations WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_one(&mut *tx)
            .await
            .expect("count");
    let _ = tx.rollback().await;
    count
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

async fn post_chat(app: &Router, auth: &str, message: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(CHAT_PATH)
        .header(header::AUTHORIZATION, auth)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "message": message }).to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("oneshot");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// Stand-in OpenBao KV v2: every read returns the same provider key.
async fn fake_openbao() -> String {
    let body = json!({ "data": { "data": { "api_key": "sk-test" } } });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let app = Router::new().fallback(move || {
            let body = body.clone();
            async move { Json(body) }
        });
        axum::serve(listener, app).await.expect("serve");
    });
    format!("http://{addr}")
}

/// Stand-in OpenAI-compatible provider; records the model the caller asked for.
async fn fake_provider(model: &str) -> (String, Arc<Mutex<Option<String>>>) {
    let seen = Arc::new(Mutex::new(None));
    let recorder = seen.clone();
    let model = model.to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let app = Router::new().fallback(move |Json(body): Json<Value>| {
            *recorder.lock().unwrap() = body
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string);
            let model = model.clone();
            async move {
                Json(json!({
                    "id": "chatcmpl-test",
                    "object": "chat.completion",
                    "created": 0,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": "สวัสดีค่ะ"},
                        "finish_reason": "stop"
                    }],
                    "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
                }))
                .into_response()
            }
        });
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://{addr}/v1"), seen)
}

#[tokio::test]
async fn chat_requires_role_and_permission() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_id = seed(&raw, "http://127.0.0.1:9/v1", "test-model").await;
    let app = router::build(app_state(pool, None));

    // Role exists but no secret backend: 503 ai_unavailable.
    let (status, body) = post_chat(&app, &bearer(tenant_id, &["hr_admin"]), "สวัสดี").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"], "ai_unavailable");

    // Employee lacks ai.chat.
    let (status, _) = post_chat(&app, &bearer(tenant_id, &["employee"]), "สวัสดี").await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .execute(&raw)
        .await;
}

#[tokio::test]
async fn chat_uses_the_chat_role_model_and_persists() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();

    let (provider, seen) = fake_provider("role-chat-model").await;
    let tenant_id = seed(&raw, &provider, "role-chat-model").await;
    let openbao = fake_openbao().await;
    let app = router::build(app_state(
        pool,
        Some(Arc::new(SecretsClient::new(openbao, "test-token"))),
    ));

    let (status, body) = post_chat(&app, &bearer(tenant_id, &["hr_admin"]), "ฝ่ายขายมีใครบ้าง").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["reply"], "สวัสดีค่ะ");
    assert_eq!(body["usage"]["input_tokens"], 10);
    assert_eq!(body["usage"]["output_tokens"], 5);
    assert!(body["citations"].is_array());
    assert_eq!(
        seen.lock().unwrap().as_deref(),
        Some("role-chat-model"),
        "chat must send the model from the chat role"
    );
    assert_eq!(conversation_count(&raw, tenant_id).await, 1);

    let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .execute(&raw)
        .await;
}

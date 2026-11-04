//! KG endpoints: RBAC, rebuild idempotency, counts and neighbours.
//! Requires DATABASE_URL; skips when unset.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use talentos_authz::jwt;
use talentos_db::TenantPool;
use talentos_gateway::{router, state::AppState};
use talentos_kernel::{TenantContext, TenantId, UserId};
use tower::ServiceExt;
use uuid::Uuid;

const JWT_SECRET: &[u8] = b"test-secret-for-kg-endpoints-32-bytes-ok";

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

/// Tenant + one entity + two employees in the same department.
async fn seed(raw: &PgPool) -> Uuid {
    let tenant_id = Uuid::new_v4();
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'KG API Co', $2)")
        .bind(tenant_id)
        .bind(format!("kg-api-{}", &tenant_id.to_string()[..8]))
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let entity_id = Uuid::new_v4();
    sqlx::query("INSERT INTO entities (id, tenant_id, name) VALUES ($1, $2, 'KG Entity')")
        .bind(entity_id)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .expect("entity");
    for (code, first_name) in [("EMP001", "สมชาย"), ("EMP002", "สมหญิง")] {
        sqlx::query(
            "INSERT INTO employees
               (id, tenant_id, entity_id, employee_code, first_name_th, last_name_th,
                hire_date, position_title, department, base_salary, tax_filing_status,
                pf_member, status)
             VALUES ($1,$2,$3,$4,$5,'ทดสอบ','2024-01-01','Engineer','Engineering',30000,1,false,'active')",
        )
        .bind(Uuid::new_v4())
        .bind(tenant_id)
        .bind(entity_id)
        .bind(code)
        .bind(first_name)
        .execute(&mut *tx)
        .await
        .expect("employee");
    }
    tx.commit().await.expect("commit");
    tenant_id
}

async fn org_node_id(raw: &PgPool, tenant_id: Uuid) -> Uuid {
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row =
        sqlx::query("SELECT id FROM kg_nodes WHERE tenant_id = $1 AND class = 'Organization'")
            .bind(tenant_id)
            .fetch_one(&mut *tx)
            .await
            .expect("org node");
    let _ = tx.rollback().await;
    row.get("id")
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

#[tokio::test]
async fn kg_rebuild_and_read_endpoints() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_id = seed(&raw).await;
    let app = router::build(app_state(pool));
    let admin = bearer(tenant_id, &["hr_admin"]);
    let manager = bearer(tenant_id, &["manager"]);
    let employee = bearer(tenant_id, &["employee"]);

    // First rebuild: hierarchy from the two employees.
    let (status, first) = call(&app, Method::POST, "/api/v1/ai/kg/rebuild", &admin, None).await;
    assert_eq!(status, StatusCode::OK, "rebuild failed: {first}");
    assert_eq!(first["nodes"]["Organization"], 1);
    assert_eq!(first["nodes"]["LegalEntity"], 1);
    assert_eq!(first["nodes"]["Employee"], 2);
    assert_eq!(first["nodes"]["Department"], 1);
    assert_eq!(first["nodes"]["Position"], 1);
    assert_eq!(first["nodes"]["Policy"], 0);
    assert_eq!(first["edges"], 7);

    // Running it again changes nothing.
    let (status, second) = call(&app, Method::POST, "/api/v1/ai/kg/rebuild", &admin, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["nodes"], second["nodes"]);
    assert_eq!(first["edges"], second["edges"]);

    // Counts + last rebuild timestamp, readable by ai.chat roles.
    let (status, body) = call(&app, Method::GET, "/api/v1/ai/kg", &manager, None).await;
    assert_eq!(status, StatusCode::OK, "counts failed: {body}");
    assert_eq!(body["nodes"]["Employee"], 2);
    assert_eq!(body["edges"], 7);
    assert!(
        body["last_rebuilt_at"].is_string(),
        "max(updated_at) should be present"
    );

    // Neighbours of the organization: its legal entity at depth 1.
    let org = org_node_id(&raw, tenant_id).await;
    let (status, body) = call(
        &app,
        Method::GET,
        &format!("/api/v1/ai/kg/neighbors?node_id={org}&depth=1"),
        &manager,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "neighbors failed: {body}");
    let nodes = body["nodes"].as_array().expect("nodes array");
    assert_eq!(nodes.len(), 2, "organization + legal entity");
    assert!(nodes.iter().any(|n| n["class"] == json!("LegalEntity")));

    // Employees hold neither permission.
    for (method, uri) in [
        (Method::POST, "/api/v1/ai/kg/rebuild"),
        (Method::GET, "/api/v1/ai/kg"),
        (
            Method::GET,
            "/api/v1/ai/kg/neighbors?node_id=00000000-0000-0000-0000-000000000000&depth=1",
        ),
    ] {
        let (status, _) = call(&app, method, uri, &employee, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
    }

    let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .execute(&raw)
        .await;
}

#[tokio::test]
async fn kg_node_listing_search_and_class_filter() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let raw = pool.pool().clone();
    let tenant_id = seed(&raw).await;

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    for (class, key, label_th) in [
        ("Employee", "KG-LIST-1", "สมชาย ใจดี"),
        ("Employee", "KG-LIST-2", "สมหญิง ใจดี"),
        ("Department", "KG-LIST-3", "ฝ่ายขาย"),
    ] {
        sqlx::query(
            "INSERT INTO kg_nodes (tenant_id, class, key, label_th) VALUES ($1, $2, $3, $4)",
        )
        .bind(tenant_id)
        .bind(class)
        .bind(key)
        .bind(label_th)
        .execute(&mut *tx)
        .await
        .expect("node");
    }
    tx.commit().await.expect("commit");

    let app = router::build(app_state(pool));
    let manager = bearer(tenant_id, &["manager"]);
    let employee = bearer(tenant_id, &["employee"]);

    // Label substring: two "ใจดี" employees.
    let (status, body) = call(
        &app,
        Method::GET,
        "/api/v1/ai/kg/nodes?q=%E0%B9%83%E0%B8%88%E0%B8%94%E0%B8%B5",
        &manager,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "node list failed: {body}");
    let nodes = body["nodes"].as_array().expect("nodes array");
    assert_eq!(nodes.len(), 2);
    assert!(nodes
        .iter()
        .all(|n| n["class"] == json!("Employee") && n["label_th"].is_string()));

    // Class filter narrows to the department, and `q` composes with it.
    let (status, body) = call(
        &app,
        Method::GET,
        "/api/v1/ai/kg/nodes?q=%E0%B8%9D%E0%B9%88%E0%B8%B2%E0%B8%A2&class=Department",
        &manager,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "class filter failed: {body}");
    assert_eq!(body["nodes"].as_array().expect("nodes").len(), 1);
    assert_eq!(body["nodes"][0]["key"], json!("KG-LIST-3"));

    // Employees lack ai.chat.
    let (status, _) = call(&app, Method::GET, "/api/v1/ai/kg/nodes", &employee, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .execute(&raw)
        .await;
}

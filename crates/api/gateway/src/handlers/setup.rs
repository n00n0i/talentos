//! First-run setup. Public, but only while the instance has no tenants:
//! it creates the first tenant, entity, and HR admin through the UI instead of
//! the provisioning CLI. Everything after this is authenticated.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::state::AppState;
use talentos_authz::password;

#[derive(Debug, Deserialize)]
pub struct SetupBody {
    pub org_name: String,
    pub slug: String,
    pub entity_name: String,
    pub admin_email: String,
    pub password: String,
    pub geofence_lat: Option<f64>,
    pub geofence_lng: Option<f64>,
    pub geofence_radius_m: Option<i32>,
    /// Test/non-production bootstrap secret; allows additional tenants.
    pub setup_token: Option<String>,
}

#[derive(Debug, Serialize)]
struct SetupResponse {
    tenant_slug: String,
    email: String,
}

pub async fn setup(State(state): State<AppState>, Json(body): Json<SetupBody>) -> Response {
    let org = body.org_name.trim();
    let slug = body.slug.trim().to_lowercase();
    let entity = body.entity_name.trim();
    let email = body.admin_email.trim().to_lowercase();

    if org.is_empty() || entity.is_empty() {
        return bad("organization and entity names are required");
    }
    if !(2..=63).contains(&slug.len())
        || !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return bad("slug must match [a-z0-9-]{2,63}");
    }
    if !email.contains('@') || email.len() > 255 {
        return bad("admin email is invalid");
    }
    if body.password.chars().count() < 12 {
        return bad("password must be at least 12 characters");
    }

    let occupied: bool = match sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tenants)")
        .fetch_one(&state.auth_pool)
        .await
    {
        Ok(v) => v,
        Err(e) => return internal(&e.to_string()),
    };
    if occupied {
        let expected = std::env::var("SETUP_TOKEN").unwrap_or_default();
        let production = std::env::var("APP_ENV").unwrap_or_default() == "production";
        let supplied = body.setup_token.as_deref().unwrap_or("");
        if production || expected.is_empty() || supplied != expected {
            return conflict("this installation is already initialized");
        }
    }

    let hash = match tokio::task::spawn_blocking({
        let p = body.password.clone();
        move || password::hash_password(&p)
    })
    .await
    {
        Ok(Ok(h)) => h,
        _ => return internal("password hashing failed"),
    };

    let tenant_id = Uuid::new_v4();
    let entity_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();

    let mut tx = match state.auth_pool.begin().await {
        Ok(t) => t,
        Err(e) => return internal(&e.to_string()),
    };
    if sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return internal("could not scope setup transaction");
    }

    if let Err(e) = sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1,$2,$3)")
        .bind(tenant_id)
        .bind(org)
        .bind(&slug)
        .execute(&mut *tx)
        .await
    {
        return conflict(&format!("slug is unavailable: {e}"));
    }

    if let Err(e) = sqlx::query(
        "INSERT INTO entities
           (id, tenant_id, name, tax_id, address, geofence_lat, geofence_lng, geofence_radius_m)
         VALUES ($1,$2,$3,NULL,'{}'::jsonb,$4,$5,$6)",
    )
    .bind(entity_id)
    .bind(tenant_id)
    .bind(entity)
    .bind(body.geofence_lat.unwrap_or(13.7563))
    .bind(body.geofence_lng.unwrap_or(100.5018))
    .bind(body.geofence_radius_m.unwrap_or(200))
    .execute(&mut *tx)
    .await
    {
        return internal(&e.to_string());
    }

    if let Err(e) = sqlx::query(
        "INSERT INTO users (id, tenant_id, email, password_hash, full_name_th, is_active)
         VALUES ($1,$2,$3,$4,'ผู้ดูแลระบบ',TRUE)",
    )
    .bind(user_id)
    .bind(tenant_id)
    .bind(&email)
    .bind(&hash)
    .execute(&mut *tx)
    .await
    {
        return internal(&e.to_string());
    }

    let role_id: i32 = match sqlx::query_scalar(
        "INSERT INTO roles (tenant_id, name) VALUES ($1,'hr_admin')
         ON CONFLICT (tenant_id, name) DO UPDATE SET name = EXCLUDED.name
         RETURNING id",
    )
    .bind(tenant_id)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(id) => id,
        Err(e) => return internal(&e.to_string()),
    };

    if let Err(e) =
        sqlx::query("INSERT INTO user_roles (user_id, role_id, entity_id) VALUES ($1,$2,$3)")
            .bind(user_id)
            .bind(role_id)
            .bind(entity_id)
            .execute(&mut *tx)
            .await
    {
        return internal(&e.to_string());
    }

    let _ = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1,'system','setup','tenant.setup','tenant',$2,$3)"#,
    )
    .bind(tenant_id)
    .bind(tenant_id.to_string())
    .bind(json!({ "slug": slug, "email": email }))
    .execute(&mut *tx)
    .await;

    if let Err(e) = tx.commit().await {
        return internal(&e.to_string());
    }

    (
        StatusCode::CREATED,
        Json(SetupResponse {
            tenant_slug: slug,
            email,
        }),
    )
        .into_response()
}

fn bad(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: "invalid_request".into(),
            message: message.into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn conflict(message: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(ErrorResponse {
            error: "conflict".into(),
            message: message.into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn internal(message: &str) -> Response {
    tracing::error!(error = %message, "setup failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "internal_error".into(),
            message: "setup failed".into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

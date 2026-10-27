//! User accounts and invitations.
//!
//! No email service is required on-prem: an invitation returns a one-time link
//! that HR copies to the new user. Tokens are stored hashed, expire in 7 days,
//! and are single-use.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use talentos_authz::{password, rbac::ROLES};

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

const INVITE_TTL_DAYS: i64 = 7;
const PASSWORD_MIN: usize = 12;
const PASSWORD_MAX: usize = 128;

#[derive(Debug, Deserialize)]
pub struct InviteBody {
    pub email: String,
    pub full_name_th: Option<String>,
    pub role: String,
    pub entity_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct AcceptInviteBody {
    pub token: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordBody {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
pub struct ResetPasswordBody {
    pub token: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct AssignRoleBody {
    pub role: String,
    pub entity_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct UserDto {
    pub id: Uuid,
    pub email: String,
    pub full_name_th: Option<String>,
    pub is_active: bool,
    pub roles: Vec<String>,
    pub last_login_at: Option<String>,
    pub totp_enabled: bool,
}

/// GET /api/v1/users
pub async fn list(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "user_management", "invite") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let rows = sqlx::query(
        r#"SELECT u.id, u.email, u.full_name_th, u.is_active, u.last_login_at, u.totp_enabled,
                  COALESCE(array_remove(array_agg(DISTINCT r.name), NULL), '{}') AS roles
             FROM users u
             LEFT JOIN user_roles ur ON ur.user_id = u.id
             LEFT JOIN roles r ON r.id = ur.role_id
            WHERE u.tenant_id = $1
            GROUP BY u.id
            ORDER BY u.created_at"#,
    )
    .bind(ctx.tenant_id.0)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    match rows {
        Ok(rows) => {
            use sqlx::Row;
            let users: Vec<UserDto> = rows
                .iter()
                .map(|row| UserDto {
                    id: row.get("id"),
                    email: row.get("email"),
                    full_name_th: row.get("full_name_th"),
                    is_active: row.get("is_active"),
                    roles: row.get::<Vec<String>, _>("roles"),
                    last_login_at: row
                        .get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_login_at")
                        .map(|t| t.to_rfc3339()),
                    totp_enabled: row.get("totp_enabled"),
                })
                .collect();
            (StatusCode::OK, Json(json!({ "users": users }))).into_response()
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

/// POST /api/v1/users/invite — returns the one-time token, no email sent.
pub async fn invite(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<InviteBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "user_management", "invite") {
        return forbidden;
    }
    let email = body.email.trim().to_lowercase();
    if !email.contains('@') || email.len() > 254 {
        return bad_request("a valid email is required");
    }
    if !ROLES.contains(&body.role.as_str()) {
        return bad_request("role must be one of hr_admin, payroll_manager, manager, employee");
    }

    let mut token_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut token_bytes);
    let token = hex::encode(token_bytes);
    let token_hash = Sha256::digest(token.as_bytes()).to_vec();

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };

    let exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE tenant_id = $1 AND lower(email) = $2")
            .bind(ctx.tenant_id.0)
            .bind(&email)
            .fetch_optional(&mut *tx)
            .await
            .unwrap_or(None);
    if exists.is_some() {
        let _ = tx.rollback().await;
        return conflict("a user with this email already exists");
    }

    // A new invitation supersedes any still-open one for the same address.
    let _ = sqlx::query(
        "UPDATE user_invites SET accepted_at = now()
          WHERE tenant_id = $1 AND lower(email) = $2 AND accepted_at IS NULL",
    )
    .bind(ctx.tenant_id.0)
    .bind(&email)
    .execute(&mut *tx)
    .await;

    let inserted = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
        r#"INSERT INTO user_invites
             (tenant_id, email, full_name_th, role, entity_id, token_hash, expires_at, created_by)
           VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(days => $7), $8)
           RETURNING expires_at"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(&email)
    .bind(body.full_name_th.as_deref())
    .bind(&body.role)
    .bind(body.entity_id)
    .bind(&token_hash)
    .bind(INVITE_TTL_DAYS as i32)
    .bind(ctx.user_id.0)
    .fetch_one(&mut *tx)
    .await;

    let expires_at = match inserted {
        Ok(t) => t,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };

    let _ = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'user.invite', 'user', $3, $4)"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0.to_string())
    .bind(&email)
    .bind(json!({ "role": body.role, "email": email }))
    .execute(&mut *tx)
    .await;

    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    (
        StatusCode::CREATED,
        Json(json!({
            "email": email,
            "role": body.role,
            "expires_at": expires_at.to_rfc3339(),
            "invite_token": token,
            "invite_path": format!("/invite/{token}"),
        })),
    )
        .into_response()
}

/// POST /api/v1/auth/accept-invite — public: the token is the credential.
pub async fn accept_invite(
    State(state): State<AppState>,
    Json(body): Json<AcceptInviteBody>,
) -> Response {
    let token = body.token.trim();
    if token.len() < 32 {
        return bad_request("invite token is invalid");
    }
    if let Some(message) = password_problem(&body.password) {
        return bad_request(&message);
    }
    let token_hash = Sha256::digest(token.as_bytes()).to_vec();

    // The tenant is not known before the token is resolved, so this lookup uses
    // the BYPASSRLS auth pool, exactly like login.
    let invite = sqlx::query(
        r#"SELECT id, tenant_id, email, full_name_th, role, entity_id, expires_at, accepted_at
             FROM user_invites WHERE token_hash = $1"#,
    )
    .bind(&token_hash)
    .fetch_optional(&state.auth_pool)
    .await;

    use sqlx::Row;
    let invite = match invite {
        Ok(Some(row)) => row,
        Ok(None) => return bad_request("invite token is invalid or already used"),
        Err(e) => return internal_err(&e.to_string()),
    };
    let accepted_at: Option<chrono::DateTime<chrono::Utc>> = invite.get("accepted_at");
    let expires_at: chrono::DateTime<chrono::Utc> = invite.get("expires_at");
    if accepted_at.is_some() || expires_at <= chrono::Utc::now() {
        return bad_request("invite token is invalid or expired");
    }

    let tenant_id = talentos_kernel::TenantId(invite.get("tenant_id"));
    let email: String = invite.get("email");
    let full_name_th: Option<String> = invite.get("full_name_th");
    let role: String = invite.get("role");
    let entity_id: Option<Uuid> = invite.get("entity_id");
    let invite_id: Uuid = invite.get("id");

    let hash =
        match tokio::task::spawn_blocking(move || password::hash_password(&body.password)).await {
            Ok(Ok(hash)) => hash,
            _ => return internal_err("password hashing failed"),
        };

    let mut tx = match state.pool.begin_tenant(tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };

    let claimed = sqlx::query(
        "UPDATE user_invites SET accepted_at = now()
          WHERE id = $1 AND accepted_at IS NULL AND expires_at > now()",
    )
    .bind(invite_id)
    .execute(&mut *tx)
    .await;
    match claimed {
        Ok(done) if done.rows_affected() == 1 => {}
        Ok(_) => {
            let _ = tx.rollback().await;
            return conflict("invite token is invalid or already used");
        }
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    }

    let user_id = match sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO users (tenant_id, email, password_hash, full_name_th, is_active)
           VALUES ($1, $2, $3, $4, TRUE) RETURNING id"#,
    )
    .bind(tenant_id.0)
    .bind(&email)
    .bind(&hash)
    .bind(full_name_th.as_deref())
    .fetch_one(&mut *tx)
    .await
    {
        Ok(id) => id,
        Err(e) => {
            let _ = tx.rollback().await;
            if is_unique_violation(&e) {
                return conflict("a user with this email already exists");
            }
            return internal_err(&e.to_string());
        }
    };

    let role_id = match sqlx::query_scalar::<_, i32>(
        r#"INSERT INTO roles (tenant_id, name) VALUES ($1, $2)
           ON CONFLICT (tenant_id, name) DO UPDATE SET name = EXCLUDED.name
           RETURNING id"#,
    )
    .bind(tenant_id.0)
    .bind(&role)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(id) => id,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };

    if let Err(e) = sqlx::query(
        r#"INSERT INTO user_roles (user_id, role_id, entity_id) VALUES ($1, $2, $3)
           ON CONFLICT (user_id, role_id) DO UPDATE SET entity_id = EXCLUDED.entity_id"#,
    )
    .bind(user_id)
    .bind(role_id)
    .bind(entity_id)
    .execute(&mut *tx)
    .await
    {
        let _ = tx.rollback().await;
        return internal_err(&e.to_string());
    }

    let _ = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'user.accept_invite', 'user', $2, $3)"#,
    )
    .bind(tenant_id.0)
    .bind(user_id.to_string())
    .bind(json!({ "role": role, "email": email }))
    .execute(&mut *tx)
    .await;

    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    (
        StatusCode::OK,
        Json(json!({ "ok": true, "email": email, "role": role })),
    )
        .into_response()
}

/// POST /api/v1/auth/change-password — own password only.
pub async fn change_password(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<ChangePasswordBody>,
) -> Response {
    if let Some(message) = password_problem(&body.new_password) {
        return bad_request(&message);
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let current: Option<String> = sqlx::query_scalar(
        "SELECT password_hash FROM users WHERE id = $1 AND tenant_id = $2 AND is_active",
    )
    .bind(ctx.user_id.0)
    .bind(ctx.tenant_id.0)
    .fetch_optional(&mut *tx)
    .await
    .ok()
    .flatten();
    let _ = tx.rollback().await;

    let Some(current) = current else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "account not found",
        );
    };

    let old = body.current_password.clone();
    let ok = tokio::task::spawn_blocking(move || password::verify_password(&old, &current))
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or(false);
    if !ok {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "current password is incorrect",
        );
    }

    let new_password = body.new_password.clone();
    let hash =
        match tokio::task::spawn_blocking(move || password::hash_password(&new_password)).await {
            Ok(Ok(hash)) => hash,
            _ => return internal_err("password hashing failed"),
        };

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    if let Err(e) = sqlx::query(
        "UPDATE users SET password_hash = $1, updated_at = now() WHERE id = $2 AND tenant_id = $3",
    )
    .bind(&hash)
    .bind(ctx.user_id.0)
    .bind(ctx.tenant_id.0)
    .execute(&mut *tx)
    .await
    {
        let _ = tx.rollback().await;
        return internal_err(&e.to_string());
    }
    let _ = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'auth.change_password', 'user', $2, '{}'::jsonb)"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0.to_string())
    .execute(&mut *tx)
    .await;
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    // Every existing refresh token belongs to the old password.
    let _ = sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(ctx.user_id.0)
    .execute(&state.auth_pool)
    .await;

    StatusCode::NO_CONTENT.into_response()
}

/// POST /api/v1/users/:id/reset-password — HR-issued, single-use link (1 hour).
pub async fn issue_password_reset(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(user_id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "user_management", "assign_role") {
        return forbidden;
    }

    let mut token_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut token_bytes);
    let token = hex::encode(token_bytes);
    let token_hash = Sha256::digest(token.as_bytes()).to_vec();

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let email: Option<String> = sqlx::query_scalar(
        "SELECT email FROM users WHERE id = $1 AND tenant_id = $2 AND is_active",
    )
    .bind(user_id)
    .bind(ctx.tenant_id.0)
    .fetch_optional(&mut *tx)
    .await
    .unwrap_or(None);
    let Some(email) = email else {
        let _ = tx.rollback().await;
        return error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "user not found or inactive",
        );
    };

    let expires_at = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
        r#"INSERT INTO password_resets (tenant_id, user_id, token_hash, expires_at, created_by)
           VALUES ($1, $2, $3, now() + interval '1 hour', $4)
           RETURNING expires_at"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(user_id)
    .bind(&token_hash)
    .bind(ctx.user_id.0)
    .fetch_one(&mut *tx)
    .await;

    let expires_at = match expires_at {
        Ok(t) => t,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };

    let _ = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'user.password_reset_issued', 'user', $3, '{}'::jsonb)"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0.to_string())
    .bind(user_id.to_string())
    .execute(&mut *tx)
    .await;

    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    (
        StatusCode::CREATED,
        Json(json!({
            "email": email,
            "expires_at": expires_at.to_rfc3339(),
            "reset_token": token,
            "reset_path": format!("/reset/{token}"),
        })),
    )
        .into_response()
}

/// POST /api/v1/auth/reset-password — public: the token is the credential.
pub async fn accept_password_reset(
    State(state): State<AppState>,
    Json(body): Json<ResetPasswordBody>,
) -> Response {
    let token = body.token.trim();
    if token.len() < 32 {
        return bad_request("reset token is invalid");
    }
    if let Some(message) = password_problem(&body.password) {
        return bad_request(&message);
    }
    let token_hash = Sha256::digest(token.as_bytes()).to_vec();

    let row = sqlx::query(
        "SELECT id, tenant_id, user_id, expires_at, used_at
           FROM password_resets WHERE token_hash = $1",
    )
    .bind(&token_hash)
    .fetch_optional(&state.auth_pool)
    .await;

    use sqlx::Row;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => return bad_request("reset token is invalid or already used"),
        Err(e) => return internal_err(&e.to_string()),
    };
    let used_at: Option<chrono::DateTime<chrono::Utc>> = row.get("used_at");
    let expires_at: chrono::DateTime<chrono::Utc> = row.get("expires_at");
    if used_at.is_some() || expires_at <= chrono::Utc::now() {
        return bad_request("reset token is invalid or expired");
    }
    let reset_id: Uuid = row.get("id");
    let tenant_id = talentos_kernel::TenantId(row.get("tenant_id"));
    let user_id: Uuid = row.get("user_id");

    let new_password = body.password.clone();
    let hash =
        match tokio::task::spawn_blocking(move || password::hash_password(&new_password)).await {
            Ok(Ok(hash)) => hash,
            _ => return internal_err("password hashing failed"),
        };

    let mut tx = match state.pool.begin_tenant(tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let claimed = sqlx::query(
        "UPDATE password_resets SET used_at = now()
          WHERE id = $1 AND used_at IS NULL AND expires_at > now()",
    )
    .bind(reset_id)
    .execute(&mut *tx)
    .await;
    match claimed {
        Ok(done) if done.rows_affected() == 1 => {}
        Ok(_) => {
            let _ = tx.rollback().await;
            return conflict("reset token is invalid or already used");
        }
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    }

    if let Err(e) = sqlx::query(
        "UPDATE users SET password_hash = $1, updated_at = now() WHERE id = $2 AND tenant_id = $3",
    )
    .bind(&hash)
    .bind(user_id)
    .bind(tenant_id.0)
    .execute(&mut *tx)
    .await
    {
        let _ = tx.rollback().await;
        return internal_err(&e.to_string());
    }
    let _ = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'user.password_reset', 'user', $2, '{}'::jsonb)"#,
    )
    .bind(tenant_id.0)
    .bind(user_id.to_string())
    .execute(&mut *tx)
    .await;
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    let _ = sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .execute(&state.auth_pool)
    .await;

    (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
}

/// POST /api/v1/users/:id/roles — add/replace one role for an existing user.
pub async fn assign_role(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(user_id): Path<Uuid>,
    Json(body): Json<AssignRoleBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "user_management", "assign_role") {
        return forbidden;
    }
    if !ROLES.contains(&body.role.as_str()) {
        return bad_request("role must be one of hr_admin, payroll_manager, manager, employee");
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let belongs: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE id = $1 AND tenant_id = $2")
            .bind(user_id)
            .bind(ctx.tenant_id.0)
            .fetch_optional(&mut *tx)
            .await
            .unwrap_or(None);
    if belongs.is_none() {
        let _ = tx.rollback().await;
        return error_response(StatusCode::NOT_FOUND, "not_found", "user not found");
    }

    let role_id = sqlx::query_scalar::<_, i32>(
        r#"INSERT INTO roles (tenant_id, name) VALUES ($1, $2)
           ON CONFLICT (tenant_id, name) DO UPDATE SET name = EXCLUDED.name
           RETURNING id"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(&body.role)
    .fetch_one(&mut *tx)
    .await;

    let role_id = match role_id {
        Ok(id) => id,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };

    if let Err(e) = sqlx::query(
        r#"INSERT INTO user_roles (user_id, role_id, entity_id) VALUES ($1, $2, $3)
           ON CONFLICT (user_id, role_id) DO UPDATE SET entity_id = EXCLUDED.entity_id"#,
    )
    .bind(user_id)
    .bind(role_id)
    .bind(body.entity_id)
    .execute(&mut *tx)
    .await
    {
        let _ = tx.rollback().await;
        return internal_err(&e.to_string());
    }

    let _ = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'user.assign_role', 'user', $3, $4)"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0.to_string())
    .bind(user_id.to_string())
    .bind(json!({ "role": body.role }))
    .execute(&mut *tx)
    .await;

    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }
    (
        StatusCode::OK,
        Json(json!({ "user_id": user_id, "role": body.role })),
    )
        .into_response()
}

/// DELETE /api/v1/users/:id — deactivate (never hard-delete an actor).
pub async fn deactivate(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(user_id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "user_management", "assign_role") {
        return forbidden;
    }
    if user_id == ctx.user_id.0 {
        return bad_request("you cannot deactivate your own account");
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let done = sqlx::query(
        "UPDATE users SET is_active = FALSE, updated_at = now() WHERE id = $1 AND tenant_id = $2",
    )
    .bind(user_id)
    .bind(ctx.tenant_id.0)
    .execute(&mut *tx)
    .await;

    match done {
        Ok(done) if done.rows_affected() == 0 => {
            let _ = tx.rollback().await;
            return error_response(StatusCode::NOT_FOUND, "not_found", "user not found");
        }
        Ok(_) => {}
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    }

    let _ = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'user.deactivate', 'user', $3, '{}'::jsonb)"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0.to_string())
    .bind(user_id.to_string())
    .execute(&mut *tx)
    .await;
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    let _ = sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .execute(&state.auth_pool)
    .await;

    StatusCode::NO_CONTENT.into_response()
}

/// POST /api/v1/users/:id/2fa/reset — HR admin clears the target user's 2FA.
pub async fn reset_2fa(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(user_id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "user_management", "reset_2fa") {
        return forbidden;
    }
    // Recovery is for a locked-out colleague, not a way around your own code.
    if user_id == ctx.user_id.0 {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "use /auth/2fa/disable with your current code to turn off your own 2FA",
        );
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };

    let done = sqlx::query(
        "UPDATE users SET totp_secret = NULL, totp_enabled = FALSE, totp_enabled_at = NULL \
         WHERE id = $1 AND tenant_id = $2",
    )
    .bind(user_id)
    .bind(ctx.tenant_id.0)
    .execute(&mut *tx)
    .await;

    match done {
        Ok(done) if done.rows_affected() == 0 => {
            let _ = tx.rollback().await;
            return error_response(StatusCode::NOT_FOUND, "not_found", "user not found");
        }
        Ok(_) => {}
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    }

    if let Err(e) = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'user.2fa_reset', 'user', $3, '{}'::jsonb)"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0.to_string())
    .bind(user_id.to_string())
    .execute(&mut *tx)
    .await
    {
        let _ = tx.rollback().await;
        return internal_err(&e.to_string());
    }

    // Same transaction: reset is only complete if the old sessions die with it.
    if let Err(e) = sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = now() \
         WHERE user_id = $1 AND tenant_id = $2 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .bind(ctx.tenant_id.0)
    .execute(&mut *tx)
    .await
    {
        let _ = tx.rollback().await;
        return internal_err(&e.to_string());
    }

    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    (StatusCode::OK, Json(json!({ "totp_enabled": false }))).into_response()
}

fn password_problem(password: &str) -> Option<String> {
    let len = password.chars().count();
    if len < PASSWORD_MIN {
        return Some(format!(
            "password must be at least {PASSWORD_MIN} characters"
        ));
    }
    if len > PASSWORD_MAX {
        return Some(format!(
            "password must be at most {PASSWORD_MAX} characters"
        ));
    }
    None
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code == "23505")
}

fn bad_request(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_request", message)
}

fn conflict(message: &str) -> Response {
    error_response(StatusCode::CONFLICT, "conflict", message)
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "user endpoint failed");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "An unexpected error occurred",
    )
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: code.into(),
            message: message.into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

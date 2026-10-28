use std::net::SocketAddr;
use std::time::Duration;

use axum::extract::connect_info::ConnectInfo;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Error as SqlxError;
use uuid::Uuid;

use talentos_authz::{jwt, password, rbac, recovery_codes, totp, AuthError};
use talentos_kernel::{EntityId, TenantContext, TenantId, UserId};

use crate::dto::requests::{LoginRequest, RecoveryTotpBody, RefreshRequest};
use crate::dto::responses::{ErrorResponse, FieldError, MeResponse, TokenResponse};
use crate::middleware::auth::Authed;
use crate::ratelimit;
use crate::state::AppState;

// M4: spec §6.5 says 10 req/minute per IP, not 15 min.
const IP_WINDOW: Duration = Duration::from_secs(60);
const EMAIL_WINDOW: Duration = Duration::from_secs(15 * 60);
const IP_MAX: u32 = 10;
const EMAIL_MAX: u32 = 5;

/// POST /api/v1/auth/login
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<LoginRequest>,
) -> Response {
    let errs = body.validate();
    if !errs.is_empty() {
        // M3: error code must be "invalid_request" per spec §4.1/§4.5, not "validation_error".
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid_request".into(),
                message: "Request validation failed".into(),
                details: errs
                    .iter()
                    .map(|(f, m)| FieldError {
                        field: (*f).into(),
                        message: (*m).into(),
                    })
                    .collect(),
                request_id: uuid::Uuid::new_v4().to_string(),
                retry_after_seconds: None,
            }),
        )
            .into_response();
    }

    let email = body.email.to_lowercase();
    let ip = addr.ip();
    let ip_key = format!("rl:login:ip:{ip}");
    let email_key = format!("rl:login:email:{}:{}", body.tenant_slug, email);

    for (key, max, window) in [
        (ip_key.as_str(), IP_MAX, IP_WINDOW),
        (email_key.as_str(), EMAIL_MAX, EMAIL_WINDOW),
    ] {
        if let Err(retry_after) = ratelimit::check(&state.redis, key, max, window).await {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, retry_after.to_string())],
                Json(ErrorResponse {
                    error: "rate_limited".into(),
                    message: "Too many attempts. Try again later.".into(),
                    details: vec![],
                    request_id: uuid::Uuid::new_v4().to_string(),
                    retry_after_seconds: Some(retry_after),
                }),
            )
                .into_response();
        }
    }

    match authenticate(
        &state,
        &body.tenant_slug,
        &email,
        &body.password,
        body.totp_code.as_deref(),
    )
    .await
    {
        Ok(ctx) => {
            ratelimit::reset(&state.redis, &[ip_key.clone(), email_key.clone()]).await;
            // M1: update last_login_at and insert audit_log row on success.
            record_login_success(&state, &ctx, &addr).await;
            match issue_tokens(&state, &ctx).await {
                Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
                Err(e) => auth_err_to_response(e),
            }
        }
        Err(e) => auth_err_to_response(e),
    }
}

async fn authenticate(
    state: &AppState,
    tenant_slug: &str,
    email: &str,
    plain: &str,
    totp_code: Option<&str>,
) -> Result<TenantContext, AuthError> {
    // B7: propagate sqlx errors instead of swallowing them into InvalidCredentials.
    let row = sqlx::query!(
        r#"SELECT u.id AS user_id, u.tenant_id, u.password_hash, u.is_active,
                  u.totp_secret, u.totp_enabled,
                  (SELECT ur2.entity_id FROM user_roles ur2
                    WHERE ur2.user_id = u.id AND ur2.entity_id IS NOT NULL
                    ORDER BY ur2.assigned_at LIMIT 1) AS entity_id,
                  array_remove(array_agg(DISTINCT r.name), NULL)::text[] AS roles
           FROM users u
           JOIN tenants t ON t.id = u.tenant_id AND t.slug = $1
           LEFT JOIN user_roles ur ON ur.user_id = u.id
           LEFT JOIN roles r ON r.id = ur.role_id
           WHERE lower(u.email) = $2
           -- Group by the primary key only; roles must not be split into one row
           -- per entity scope, or a multi-role user logs in with only one role.
           GROUP BY u.id"#,
        tenant_slug,
        email
    )
    .fetch_optional(&state.auth_pool)
    .await
    .map_err(internal_db_err)?;

    let Some(row) = row else {
        // Timing equalization: burn equivalent CPU to a real verify (spec §6.1 / TC-12).
        let _ = password::verify_password(plain, &password::dummy_hash());
        return Err(AuthError::InvalidCredentials);
    };

    if !row.is_active {
        let _ = password::verify_password(plain, &row.password_hash);
        return Err(AuthError::AccountDisabled);
    }

    if !password::verify_password(plain, &row.password_hash)? {
        return Err(AuthError::InvalidCredentials);
    }

    // M2: reject zero-role users (spec §6.3). A user must have at least one role.
    let roles = row.roles.unwrap_or_default();
    if roles.is_empty() {
        tracing::warn!(user_id = %row.user_id, "login rejected: user has no roles assigned");
        return Err(AuthError::InsufficientPermission {
            reason: "no roles".into(),
        });
    }

    // 2FA gate after password success.
    if row.totp_enabled {
        let cipher = pii_cipher(state)?;
        let secret = match row.totp_secret {
            Some(enc) => cipher.decrypt(&enc).map_err(|e| {
                tracing::error!(error = %e, user_id = %row.user_id, "failed to decrypt totp_secret");
                AuthError::Internal("totp decrypt failed".into())
            })?,
            None => {
                tracing::error!(user_id = %row.user_id, "totp enabled but secret missing");
                return Err(AuthError::Internal("totp secret missing".into()));
            }
        };

        match totp_code {
            Some(code) => {
                if totp::verify(&secret, code, now_unix()) {
                    // TOTP accepted.
                } else if verify_recovery_code_tx(state, row.tenant_id, row.user_id, code).await? {
                    // Recovery code accepted and marked used.
                } else {
                    return Err(AuthError::InvalidTotp);
                }
            }
            None => return Err(AuthError::TotpRequired),
        }
    }

    let ctx = TenantContext {
        tenant_id: TenantId(row.tenant_id),
        entity_id: row.entity_id.map(EntityId),
        user_id: UserId(row.user_id),
        roles,
    };
    Ok(ctx)
}

/// M1: side effects on successful login — update last_login_at, write audit_log.
async fn record_login_success(state: &AppState, ctx: &TenantContext, addr: &SocketAddr) {
    // Best-effort: don't fail the login if the bookkeeping write fails.
    let _ = sqlx::query!(
        "UPDATE users SET last_login_at = now() WHERE id = $1",
        ctx.user_id.0
    )
    .execute(&state.auth_pool)
    .await
    .map_err(|e| tracing::warn!(error = %e, "failed to update last_login_at"));

    let _ = sqlx::query!(
        r#"INSERT INTO audit_log (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state, ip_address)
           VALUES ($1, 'user', $2, 'auth.login', 'session', $2, '{}'::jsonb, $3::inet)"#,
        ctx.tenant_id.0,
        ctx.user_id.0.to_string(),
        sqlx::types::ipnetwork::IpNetwork::from(addr.ip())
    )
    .execute(&state.auth_pool)
    .await
    .map_err(|e| tracing::warn!(error = %e, "failed to write audit_log for login"));
}

async fn verify_recovery_code_tx(
    state: &AppState,
    tenant_id: Uuid,
    user_id: Uuid,
    code: &str,
) -> Result<bool, AuthError> {
    let normalized = recovery_codes::normalize(code);
    if normalized.len() != 8 {
        return Ok(false);
    }
    let hash = recovery_codes::hash_code(&normalized);
    let updated = sqlx::query!(
        "UPDATE totp_recovery_codes SET used_at = now() \
         WHERE user_id = $1 AND tenant_id = $2 AND code_hash = $3 AND used_at IS NULL \
         RETURNING id",
        user_id,
        tenant_id,
        hash
    )
    .fetch_optional(&state.auth_pool)
    .await
    .map_err(internal_db_err)?;
    Ok(updated.is_some())
}

async fn issue_tokens(state: &AppState, ctx: &TenantContext) -> Result<TokenResponse, AuthError> {
    let access = jwt::issue_access_token(ctx, &state.jwt_secret, state.access_ttl_secs)?;
    let raw_refresh = jwt::generate_refresh_token();
    store_refresh_token(state, ctx, &raw_refresh).await?;
    Ok(TokenResponse {
        access_token: access,
        refresh_token: raw_refresh,
        token_type: "Bearer",
        expires_in: state.access_ttl_secs,
    })
}

fn hash_refresh(raw: &str) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(raw.as_bytes());
    h.finalize().to_vec()
}

async fn store_refresh_token(
    state: &AppState,
    ctx: &TenantContext,
    raw: &str,
) -> Result<(), AuthError> {
    let hashed = hash_refresh(raw);
    sqlx::query!(
        r#"INSERT INTO refresh_tokens (token_hash, user_id, tenant_id, entity_id, expires_at)
           VALUES ($1, $2, $3, $4, now() + make_interval(days => $5))"#,
        hashed,
        ctx.user_id.0,
        ctx.tenant_id.0,
        ctx.entity_id.map(|e| e.0),
        state.refresh_ttl_days as i32
    )
    .execute(&state.auth_pool)
    .await
    .map_err(internal_db_err)?;
    Ok(())
}

/// POST /api/v1/auth/refresh — rotate: invalidate old, issue new pair.
/// M6: rate-limited like login (spec §5 step 3 applies to all /auth/*).
pub async fn refresh(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<RefreshRequest>,
) -> Response {
    // M6: apply IP-based rate limiting to refresh endpoint too.
    let ip = addr.ip();
    let rl_key = format!("rl:refresh:ip:{ip}");
    if let Err(retry_after) =
        ratelimit::check(&state.redis, &rl_key, 30, Duration::from_secs(60)).await
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(axum::http::header::RETRY_AFTER, retry_after.to_string())],
            Json(ErrorResponse {
                error: "rate_limited".into(),
                message: "Too many refresh attempts.".into(),
                details: vec![],
                request_id: uuid::Uuid::new_v4().to_string(),
                retry_after_seconds: Some(retry_after),
            }),
        )
            .into_response();
    }

    let hashed = hash_refresh(&body.refresh_token);

    // B7: propagate DB errors, don't swallow into 401.
    let lookup = sqlx::query!(
        r#"SELECT user_id, tenant_id, entity_id, revoked_at, parent_hash,
                  expires_at > now() AS is_valid
           FROM refresh_tokens WHERE token_hash = $1"#,
        hashed
    )
    .fetch_optional(&state.auth_pool)
    .await
    .map_err(internal_db_err);

    let tok = match lookup {
        Ok(Some(t)) => t,
        Ok(None) => return auth_err_to_response(AuthError::TokenExpired),
        Err(e) => return auth_err_to_response(e),
    };

    // Reuse detection: already-revoked token presented again → theft scenario, revoke family.
    if tok.revoked_at.is_some() {
        tracing::warn!("refresh token reuse detected — revoking family");
        revoke_family(&state, tok.parent_hash).await;
        return auth_err_to_response(AuthError::TokenExpired);
    }

    if !tok.is_valid.unwrap_or(false) {
        return auth_err_to_response(AuthError::TokenExpired);
    }

    // Load roles at refresh time (may differ from login-time snapshot).
    let roles_result = sqlx::query!(
        r#"SELECT array_remove(array_agg(DISTINCT r.name), NULL)::text[] AS roles
           FROM user_roles ur JOIN roles r ON r.id = ur.role_id
           WHERE ur.user_id = $1"#,
        tok.user_id
    )
    .fetch_one(&state.auth_pool)
    .await;
    let roles = match roles_result {
        Ok(r) => r.roles.unwrap_or_default(),
        Err(e) => return internal_err_to_response(e),
    };

    let ctx = TenantContext {
        tenant_id: TenantId(tok.tenant_id),
        entity_id: tok.entity_id.map(EntityId),
        user_id: UserId(tok.user_id),
        roles,
    };

    // Generate new tokens BEFORE opening tx so JWT issuance failure doesn't hold DB lock.
    let access = match jwt::issue_access_token(&ctx, &state.jwt_secret, state.access_ttl_secs) {
        Ok(t) => t,
        Err(e) => return auth_err_to_response(e),
    };
    let raw_new = jwt::generate_refresh_token();
    let new_hashed = hash_refresh(&raw_new);

    // Atomic rotation: revoke old + insert successor in ONE CTE.
    let tx_result = sqlx::query!(
        r#"WITH rotated AS (
               UPDATE refresh_tokens SET revoked_at = now()
               WHERE token_hash = $1 AND revoked_at IS NULL
               RETURNING 1
           )
           INSERT INTO refresh_tokens (token_hash, user_id, tenant_id, entity_id, expires_at, parent_hash)
           SELECT $2, $3, $4, $5, now() + make_interval(days => $6), $1
           FROM rotated
           RETURNING id"#,
        hashed,
        new_hashed,
        ctx.user_id.0,
        ctx.tenant_id.0,
        ctx.entity_id.map(|e| e.0),
        state.refresh_ttl_days as i32,
    )
    .fetch_optional(&state.auth_pool)
    .await;

    match tx_result {
        Ok(Some(_)) => {
            ratelimit::reset(&state.redis, std::slice::from_ref(&rl_key)).await;
            (
                StatusCode::OK,
                Json(TokenResponse {
                    access_token: access,
                    refresh_token: raw_new,
                    token_type: "Bearer",
                    expires_in: state.access_ttl_secs,
                }),
            )
                .into_response()
        }
        Ok(_) => {
            // Concurrent consumption won the race — reject replay.
            auth_err_to_response(AuthError::TokenExpired)
        }
        Err(e) => internal_err_to_response(e),
    }
}

/// Walk parent_hash chain upward and revoke every ancestor token.
async fn revoke_family(state: &AppState, start_parent: Option<Vec<u8>>) {
    let mut current = match start_parent {
        Some(h) => h,
        None => return,
    };
    loop {
        let row = sqlx::query!(
            r#"UPDATE refresh_tokens SET revoked_at = now()
               WHERE token_hash = $1 AND revoked_at IS NULL
               RETURNING parent_hash"#,
            current
        )
        .fetch_optional(&state.auth_pool)
        .await;

        match row {
            Ok(Some(r)) => match r.parent_hash {
                Some(next) => current = next,
                None => break,
            },
            Ok(None) => break,
            Err(e) => {
                // B7: log loudly but continue walking — partial revocation is better than none.
                tracing::error!(error = %e, "revoke_family: DB error mid-walk, stopping");
                break;
            }
        }
    }
    tracing::info!("refresh token family revocation completed");
}

/// GET /api/v1/auth/me — profile + effective permissions from verified JWT context.
pub async fn me(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    // Tenant-scoped: RLS is FORCE-enabled, so this must run inside the GUC tx.
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err_to_response(e),
    };
    let row = sqlx::query!(
        r#"SELECT email, full_name_th, full_name_en, last_login_at, totp_enabled,
                  (SELECT name FROM tenants WHERE id = $2) AS tenant_name
           FROM users WHERE id = $1 AND tenant_id = $2 AND is_active = TRUE"#,
        ctx.user_id.0,
        ctx.tenant_id.0
    )
    .fetch_optional(&mut *tx)
    .await;

    let u = match row {
        Ok(Some(u)) => u,
        Ok(None) => return auth_err_to_response(AuthError::AccountDisabled),
        Err(e) => return internal_err_to_response(e),
    };

    let perms = rbac::expand_permissions(&ctx.roles);
    (
        StatusCode::OK,
        Json(MeResponse {
            user_id: ctx.user_id.to_string(),
            email: u.email,
            full_name_th: u.full_name_th,
            full_name_en: u.full_name_en,
            tenant_id: ctx.tenant_id.to_string(),
            tenant_name: u.tenant_name,
            entity_id: ctx.entity_id.map(|e| e.to_string()),
            roles: ctx.roles.clone(),
            permissions: perms,
            last_login_at: u.last_login_at.map(|t| t.to_rfc3339()),
            totp_enabled: u.totp_enabled,
        }),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
pub struct TotpCodeBody {
    pub code: String,
}

/// POST /api/v1/auth/2fa/setup
pub async fn setup_2fa(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    let Some(cipher) = state.pii.as_deref() else {
        return service_unavailable(
            "PII_ENCRYPTION_KEY is not configured; 2FA setup is unavailable",
        );
    };

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err_to_response(e),
    };

    let row = match sqlx::query!(
        "SELECT email, totp_enabled FROM users WHERE id = $1 AND tenant_id = $2 AND is_active FOR UPDATE",
        ctx.user_id.0,
        ctx.tenant_id.0
    )
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err_to_response(e);
        }
    };

    let Some(row) = row else {
        let _ = tx.rollback().await;
        return auth_err_to_response(AuthError::AccountDisabled);
    };

    if row.totp_enabled {
        let _ = tx.rollback().await;
        return conflict("2FA is already enabled");
    }

    let secret_plain = totp::generate_secret();
    let secret_encrypted = match cipher.encrypt(&secret_plain) {
        Ok(enc) => enc,
        Err(e) => {
            tracing::error!(error = %e, "failed to encrypt totp_secret");
            let _ = tx.rollback().await;
            return auth_err_to_response(AuthError::Internal("encryption failed".into()));
        }
    };

    let updated = sqlx::query!(
        "UPDATE users SET totp_secret = $1, totp_enabled = FALSE, totp_enabled_at = NULL \
         WHERE id = $2 AND tenant_id = $3",
        secret_encrypted,
        ctx.user_id.0,
        ctx.tenant_id.0
    )
    .execute(&mut *tx)
    .await;

    if let Err(e) = updated {
        let _ = tx.rollback().await;
        return internal_err_to_response(e);
    }

    if let Err(e) = audit_2fa_tx(
        &mut tx,
        ctx.tenant_id,
        ctx.user_id,
        "auth.2fa_setup",
        ctx.user_id,
    )
    .await
    {
        let _ = tx.rollback().await;
        return internal_err_to_response(e);
    }

    if let Err(e) = tx.commit().await {
        return internal_err_to_response(e);
    }

    (
        StatusCode::OK,
        Json(json!({
            "secret": secret_plain,
            "otpauth_url": totp::otpauth_url(&row.email, &secret_plain),
        })),
    )
        .into_response()
}

/// POST /api/v1/auth/2fa/enable
pub async fn enable_2fa(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<TotpCodeBody>,
) -> Response {
    if body.code.len() != 6 || !body.code.chars().all(|c| c.is_ascii_digit()) {
        return bad_request("code must be 6 digits");
    }
    if let Err(retry_after) = ratelimit::check(
        &state.redis,
        &format!("rl:2fa:{}", ctx.user_id.0),
        10,
        Duration::from_secs(300),
    )
    .await
    {
        return too_many_2fa_attempts(retry_after);
    }

    let cipher = match pii_cipher(&state) {
        Ok(c) => c,
        Err(e) => return auth_err_to_response(e),
    };

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err_to_response(e),
    };

    let row = match sqlx::query!(
        "SELECT totp_secret FROM users WHERE id = $1 AND tenant_id = $2 AND is_active FOR UPDATE",
        ctx.user_id.0,
        ctx.tenant_id.0
    )
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err_to_response(e);
        }
    };

    let Some(row) = row else {
        let _ = tx.rollback().await;
        return auth_err_to_response(AuthError::AccountDisabled);
    };

    let Some(secret_encrypted) = row.totp_secret else {
        let _ = tx.rollback().await;
        return bad_request("2FA setup required");
    };

    let secret_plain = match cipher.decrypt(&secret_encrypted) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "failed to decrypt totp_secret");
            let _ = tx.rollback().await;
            return auth_err_to_response(AuthError::Internal("decryption failed".into()));
        }
    };

    if !totp::verify(&secret_plain, &body.code, now_unix()) {
        let _ = tx.rollback().await;
        return bad_request("invalid TOTP code");
    }

    let updated = sqlx::query!(
        "UPDATE users SET totp_enabled = TRUE, totp_enabled_at = now() \
         WHERE id = $1 AND tenant_id = $2",
        ctx.user_id.0,
        ctx.tenant_id.0
    )
    .execute(&mut *tx)
    .await;

    if let Err(e) = updated {
        let _ = tx.rollback().await;
        return internal_err_to_response(e);
    }

    if let Err(e) = audit_2fa_tx(
        &mut tx,
        ctx.tenant_id,
        ctx.user_id,
        "auth.2fa_enable",
        ctx.user_id,
    )
    .await
    {
        let _ = tx.rollback().await;
        return internal_err_to_response(e);
    }

    // Generate recovery codes in the same transaction so enable is atomic.
    let codes = match rotate_recovery_codes_tx(&mut tx, ctx.tenant_id, ctx.user_id).await {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err_to_response(e);
        }
    };

    if let Err(e) = tx.commit().await {
        return internal_err_to_response(e);
    }

    (
        StatusCode::OK,
        Json(json!({
            "totp_enabled": true,
            "recovery_codes": codes,
        })),
    )
        .into_response()
}

/// Rotate recovery codes: delete old ones and insert 10 fresh hashes.
/// Returns the plaintext codes; this is the only time they are visible.
async fn rotate_recovery_codes_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: TenantId,
    user_id: UserId,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query!(
        "DELETE FROM totp_recovery_codes WHERE user_id = $1 AND tenant_id = $2",
        user_id.0,
        tenant_id.0
    )
    .execute(&mut **tx)
    .await?;

    let codes = recovery_codes::generate_codes(10);
    for code in &codes {
        let hash = recovery_codes::hash_code(code);
        sqlx::query!(
            "INSERT INTO totp_recovery_codes (tenant_id, user_id, code_hash) VALUES ($1, $2, $3)",
            tenant_id.0,
            user_id.0,
            hash
        )
        .execute(&mut **tx)
        .await?;
    }
    Ok(codes)
}

/// POST /api/v1/auth/2fa/recovery-codes — regenerate recovery codes after TOTP check.
pub async fn regenerate_recovery_codes(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<RecoveryTotpBody>,
) -> Response {
    if body.code.len() != 6 || !body.code.chars().all(|c| c.is_ascii_digit()) {
        return bad_request("code must be 6 digits");
    }
    if let Err(retry_after) = ratelimit::check(
        &state.redis,
        &format!("rl:2fa:{}", ctx.user_id.0),
        10,
        Duration::from_secs(300),
    )
    .await
    {
        return too_many_2fa_attempts(retry_after);
    }

    let cipher = match pii_cipher(&state) {
        Ok(c) => c,
        Err(e) => return auth_err_to_response(e),
    };

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err_to_response(e),
    };

    let row = match sqlx::query!(
        "SELECT totp_secret, totp_enabled FROM users WHERE id = $1 AND tenant_id = $2 AND is_active FOR UPDATE",
        ctx.user_id.0,
        ctx.tenant_id.0
    )
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err_to_response(e);
        }
    };

    let Some(row) = row else {
        let _ = tx.rollback().await;
        return auth_err_to_response(AuthError::AccountDisabled);
    };

    if !row.totp_enabled {
        let _ = tx.rollback().await;
        return bad_request("2FA is not enabled");
    }

    let Some(secret_encrypted) = row.totp_secret else {
        let _ = tx.rollback().await;
        return bad_request("2FA is not set up");
    };

    let secret_plain = match cipher.decrypt(&secret_encrypted) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "failed to decrypt totp_secret");
            let _ = tx.rollback().await;
            return auth_err_to_response(AuthError::Internal("decryption failed".into()));
        }
    };

    if !totp::verify(&secret_plain, &body.code, now_unix()) {
        let _ = tx.rollback().await;
        return bad_request("invalid TOTP code");
    }

    let codes = match rotate_recovery_codes_tx(&mut tx, ctx.tenant_id, ctx.user_id).await {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err_to_response(e);
        }
    };

    if let Err(e) = tx.commit().await {
        return internal_err_to_response(e);
    }

    (StatusCode::OK, Json(json!({ "codes": codes }))).into_response()
}

/// POST /api/v1/auth/2fa/disable
pub async fn disable_2fa(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<TotpCodeBody>,
) -> Response {
    if body.code.len() != 6 || !body.code.chars().all(|c| c.is_ascii_digit()) {
        return bad_request("code must be 6 digits");
    }
    if let Err(retry_after) = ratelimit::check(
        &state.redis,
        &format!("rl:2fa:{}", ctx.user_id.0),
        10,
        Duration::from_secs(300),
    )
    .await
    {
        return too_many_2fa_attempts(retry_after);
    }

    let cipher = match pii_cipher(&state) {
        Ok(c) => c,
        Err(e) => return auth_err_to_response(e),
    };

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err_to_response(e),
    };

    let row = match sqlx::query!(
        "SELECT totp_secret FROM users WHERE id = $1 AND tenant_id = $2 AND is_active FOR UPDATE",
        ctx.user_id.0,
        ctx.tenant_id.0
    )
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err_to_response(e);
        }
    };

    let Some(row) = row else {
        let _ = tx.rollback().await;
        return auth_err_to_response(AuthError::AccountDisabled);
    };

    let Some(secret_encrypted) = row.totp_secret else {
        let _ = tx.rollback().await;
        return bad_request("2FA is not set up");
    };

    let secret_plain = match cipher.decrypt(&secret_encrypted) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "failed to decrypt totp_secret");
            let _ = tx.rollback().await;
            return auth_err_to_response(AuthError::Internal("decryption failed".into()));
        }
    };

    if !totp::verify(&secret_plain, &body.code, now_unix()) {
        let _ = tx.rollback().await;
        return bad_request("invalid TOTP code");
    }

    let updated = sqlx::query!(
        "UPDATE users SET totp_secret = NULL, totp_enabled = FALSE, totp_enabled_at = NULL \
         WHERE id = $1 AND tenant_id = $2",
        ctx.user_id.0,
        ctx.tenant_id.0
    )
    .execute(&mut *tx)
    .await;

    if let Err(e) = updated {
        let _ = tx.rollback().await;
        return internal_err_to_response(e);
    }

    if let Err(e) = audit_2fa_tx(
        &mut tx,
        ctx.tenant_id,
        ctx.user_id,
        "auth.2fa_disable",
        ctx.user_id,
    )
    .await
    {
        let _ = tx.rollback().await;
        return internal_err_to_response(e);
    }

    // Same transaction: a disable that cannot revoke sessions is not a disable.
    if let Err(e) = sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = now() \
         WHERE user_id = $1 AND tenant_id = $2 AND revoked_at IS NULL",
    )
    .bind(ctx.user_id.0)
    .bind(ctx.tenant_id.0)
    .execute(&mut *tx)
    .await
    {
        let _ = tx.rollback().await;
        return internal_err_to_response(e);
    }

    if let Err(e) =
        sqlx::query("DELETE FROM totp_recovery_codes WHERE user_id = $1 AND tenant_id = $2")
            .bind(ctx.user_id.0)
            .bind(ctx.tenant_id.0)
            .execute(&mut *tx)
            .await
    {
        let _ = tx.rollback().await;
        return internal_err_to_response(e);
    }

    if let Err(e) = tx.commit().await {
        return internal_err_to_response(e);
    }

    (StatusCode::OK, Json(json!({ "totp_enabled": false }))).into_response()
}

/// GET /api/v1/auth/sessions — caller's active refresh-token sessions.
pub async fn list_sessions(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    let rows = match sqlx::query!(
        r#"SELECT id, created_at, expires_at, ip_address, user_agent
           FROM refresh_tokens
           WHERE user_id = $1 AND tenant_id = $2 AND revoked_at IS NULL AND expires_at > now()
           ORDER BY created_at DESC"#,
        ctx.user_id.0,
        ctx.tenant_id.0
    )
    .fetch_all(&state.auth_pool)
    .await
    {
        Ok(r) => r,
        Err(e) => return internal_err_to_response(e),
    };

    let sessions: Vec<_> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "created_at": r.created_at.to_rfc3339(),
                "expires_at": r.expires_at.to_rfc3339(),
                "ip_address": r.ip_address.map(|ip| ip.to_string()),
                "user_agent": r.user_agent,
            })
        })
        .collect();

    (StatusCode::OK, Json(json!({ "sessions": sessions }))).into_response()
}

/// DELETE /api/v1/auth/sessions/:id — revoke one of the caller's own sessions.
pub async fn revoke_session(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(session_id): Path<i64>,
) -> Response {
    let done = match sqlx::query!(
        "UPDATE refresh_tokens SET revoked_at = now() \
         WHERE id = $1 AND user_id = $2 AND tenant_id = $3 AND revoked_at IS NULL",
        session_id,
        ctx.user_id.0,
        ctx.tenant_id.0
    )
    .execute(&state.auth_pool)
    .await
    {
        Ok(r) => r.rows_affected(),
        Err(e) => return internal_err_to_response(e),
    };

    if done == 0 {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "not_found".into(),
                message: "session not found".into(),
                details: vec![],
                request_id: uuid::Uuid::new_v4().to_string(),
                retry_after_seconds: None,
            }),
        )
            .into_response();
    }

    StatusCode::OK.into_response()
}

/// POST /api/v1/auth/sessions/revoke-all — revoke all of the caller's sessions.
pub async fn revoke_all_sessions(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(e) = sqlx::query!(
        "UPDATE refresh_tokens SET revoked_at = now() \
         WHERE user_id = $1 AND tenant_id = $2 AND revoked_at IS NULL",
        ctx.user_id.0,
        ctx.tenant_id.0
    )
    .execute(&state.auth_pool)
    .await
    {
        return internal_err_to_response(e);
    }

    StatusCode::OK.into_response()
}

fn pii_cipher(state: &AppState) -> Result<&talentos_db::pii::PiiCipher, AuthError> {
    state.pii.as_deref().ok_or_else(|| {
        tracing::error!("PII_ENCRYPTION_KEY not configured but TOTP secret required");
        AuthError::Internal("PII encryption not configured".into())
    })
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn audit_2fa_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: TenantId,
    actor_id: UserId,
    action: &str,
    target_id: UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, $3, 'user', $4, '{}'::jsonb)"#,
        tenant_id.0,
        actor_id.0.to_string(),
        action,
        target_id.0.to_string()
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn bad_request(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: "invalid_request".into(),
            message: message.into(),
            details: vec![],
            request_id: uuid::Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn too_many_2fa_attempts(retry_after: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(axum::http::header::RETRY_AFTER, retry_after.to_string())],
        Json(ErrorResponse {
            error: "rate_limited".into(),
            message: "Too many 2FA attempts. Try again later.".into(),
            details: vec![],
            request_id: uuid::Uuid::new_v4().to_string(),
            retry_after_seconds: Some(retry_after),
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
            request_id: uuid::Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn service_unavailable(message: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: "service_unavailable".into(),
            message: message.into(),
            details: vec![],
            request_id: uuid::Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

// --- Error helpers ---

/// B7/M8: map sqlx errors to a distinct internal-error variant returning 500,
/// never masquerading as 401 bad credentials.
fn internal_db_err(e: SqlxError) -> AuthError {
    tracing::error!(error = %e, "database error");
    AuthError::Internal(format!("db: {}", e))
}

fn internal_err_to_response(_e: SqlxError) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "internal_error".into(),
            message: "An unexpected error occurred".into(),
            details: vec![],
            request_id: uuid::Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn auth_err_to_response(e: AuthError) -> Response {
    let status = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::UNAUTHORIZED);
    (
        status,
        Json(ErrorResponse {
            error: e.error_code().into(),
            message: match e {
                AuthError::InvalidCredentials | AuthError::AccountDisabled => {
                    "Invalid email or password".into()
                }
                AuthError::TokenExpired => "Refresh token expired".into(),
                AuthError::RateLimited => "Too many attempts. Try again later.".into(),
                AuthError::InsufficientPermission { .. } => "Invalid email or password".into(),
                AuthError::TotpRequired => "TOTP code required".into(),
                AuthError::InvalidTotp => "Invalid TOTP code".into(),
                AuthError::TokenMalformed | AuthError::TokenSignatureInvalid => {
                    "Authentication failed".into()
                }
                AuthError::Internal(_) => "An unexpected error occurred".into(),
            },
            details: vec![],
            request_id: uuid::Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

//! Tenant administration: organization profile, payroll policy, system status.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use talentos_payroll::settings::PayrollSettings;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct TenantBody {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct EntityBody {
    pub name: String,
    pub tax_id: Option<String>,
    pub address: Option<Value>,
}

/// GET /api/v1/settings/payroll
pub async fn get_payroll(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "settings", "view_company") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let row = sqlx::query(
        "SELECT unpaid_leave_divisor, ot_weekday_rate, ot_weekend_rate, ot_holiday_rate,
                leave_approval_chain, approval_sla_hours, gl_salary_account, gl_net_account,
                gl_tax_account, gl_sso_account, gl_pf_account, gl_loan_account, gl_claim_account
           FROM tenant_payroll_settings WHERE tenant_id = $1",
    )
    .bind(ctx.tenant_id.0)
    .fetch_optional(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    use sqlx::Row;
    let settings = match row {
        Ok(Some(row)) => PayrollSettings {
            unpaid_leave_divisor: row.get("unpaid_leave_divisor"),
            ot_weekday_rate: row.get("ot_weekday_rate"),
            ot_weekend_rate: row.get("ot_weekend_rate"),
            ot_holiday_rate: row.get("ot_holiday_rate"),
            leave_approval_chain: row
                .get::<Option<Vec<String>>, _>("leave_approval_chain")
                .unwrap_or_else(|| vec!["hr_admin".to_string()]),
            approval_sla_hours: row.get("approval_sla_hours"),
            gl_salary_account: row.get("gl_salary_account"),
            gl_net_account: row.get("gl_net_account"),
            gl_tax_account: row.get("gl_tax_account"),
            gl_sso_account: row.get("gl_sso_account"),
            gl_pf_account: row.get("gl_pf_account"),
            gl_loan_account: row.get("gl_loan_account"),
            gl_claim_account: row.get("gl_claim_account"),
        },
        Ok(None) => PayrollSettings::standard(),
        Err(e) => return internal_err(&e.to_string()),
    };
    (StatusCode::OK, Json(settings)).into_response()
}

/// PUT /api/v1/settings/payroll
pub async fn put_payroll(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(settings): Json<PayrollSettings>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "settings", "manage_company") {
        return forbidden;
    }
    if !settings.is_valid() {
        return bad_request(
            "unpaid_leave_divisor must be 1..31; OT rates must be between 1 and 5; \
             leave_approval_chain must be 1..3 roles from {manager, hr_admin} ending with hr_admin; \
             approval_sla_hours must be 0..720; \
             GL account codes must be non-empty, at most 20 alphanumeric characters",
        );
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let done = sqlx::query(
        r#"INSERT INTO tenant_payroll_settings
             (tenant_id, unpaid_leave_divisor, ot_weekday_rate, ot_weekend_rate,
              ot_holiday_rate, leave_approval_chain, approval_sla_hours,
              gl_salary_account, gl_net_account, gl_tax_account, gl_sso_account,
              gl_pf_account, gl_loan_account, gl_claim_account, updated_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
           ON CONFLICT (tenant_id) DO UPDATE SET
               unpaid_leave_divisor = EXCLUDED.unpaid_leave_divisor,
               ot_weekday_rate = EXCLUDED.ot_weekday_rate,
               ot_weekend_rate = EXCLUDED.ot_weekend_rate,
               ot_holiday_rate = EXCLUDED.ot_holiday_rate,
               leave_approval_chain = EXCLUDED.leave_approval_chain,
               approval_sla_hours = EXCLUDED.approval_sla_hours,
               gl_salary_account = EXCLUDED.gl_salary_account,
               gl_net_account = EXCLUDED.gl_net_account,
               gl_tax_account = EXCLUDED.gl_tax_account,
               gl_sso_account = EXCLUDED.gl_sso_account,
               gl_pf_account = EXCLUDED.gl_pf_account,
               gl_loan_account = EXCLUDED.gl_loan_account,
               gl_claim_account = EXCLUDED.gl_claim_account,
               updated_at = now(),
               updated_by = EXCLUDED.updated_by"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(settings.unpaid_leave_divisor)
    .bind(settings.ot_weekday_rate)
    .bind(settings.ot_weekend_rate)
    .bind(settings.ot_holiday_rate)
    .bind(&settings.leave_approval_chain)
    .bind(settings.approval_sla_hours)
    .bind(&settings.gl_salary_account)
    .bind(&settings.gl_net_account)
    .bind(&settings.gl_tax_account)
    .bind(&settings.gl_sso_account)
    .bind(&settings.gl_pf_account)
    .bind(&settings.gl_loan_account)
    .bind(&settings.gl_claim_account)
    .bind(ctx.user_id.0)
    .execute(&mut *tx)
    .await;

    if let Err(e) = done {
        let _ = tx.rollback().await;
        return internal_err(&e.to_string());
    }
    let _ = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'settings.payroll', 'tenant', $3, $4)"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0.to_string())
    .bind(ctx.tenant_id.0.to_string())
    .bind(serde_json::to_value(&settings).unwrap_or_else(|_| json!({})))
    .execute(&mut *tx)
    .await;
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }
    (StatusCode::OK, Json(settings)).into_response()
}

/// GET /api/v1/settings/organization
pub async fn get_organization(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "settings", "view_company") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let tenant = sqlx::query("SELECT id, name, slug FROM tenants WHERE id = $1")
        .bind(ctx.tenant_id.0)
        .fetch_optional(&mut *tx)
        .await;
    let entities = sqlx::query(
        "SELECT id, name, tax_id, address, is_active FROM entities
          WHERE tenant_id = $1 ORDER BY created_at",
    )
    .bind(ctx.tenant_id.0)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    use sqlx::Row;
    let tenant = match tenant {
        Ok(Some(row)) => json!({
            "id": row.get::<Uuid, _>("id"),
            "name": row.get::<String, _>("name"),
            "slug": row.get::<String, _>("slug"),
        }),
        Ok(None) => return not_found("tenant"),
        Err(e) => return internal_err(&e.to_string()),
    };
    let entities = match entities {
        Ok(rows) => rows
            .iter()
            .map(|row| {
                json!({
                    "id": row.get::<Uuid, _>("id"),
                    "name": row.get::<String, _>("name"),
                    "tax_id": row.get::<Option<String>, _>("tax_id"),
                    "address": row.get::<Value, _>("address"),
                    "is_active": row.get::<bool, _>("is_active"),
                })
            })
            .collect::<Vec<_>>(),
        Err(e) => return internal_err(&e.to_string()),
    };
    (
        StatusCode::OK,
        Json(json!({ "tenant": tenant, "entities": entities })),
    )
        .into_response()
}

/// PUT /api/v1/settings/tenant
pub async fn put_tenant(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<TenantBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "settings", "manage_company") {
        return forbidden;
    }
    if body.name.trim().is_empty() {
        return bad_request("name is required");
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let done = sqlx::query("UPDATE tenants SET name = $1, updated_at = now() WHERE id = $2")
        .bind(body.name.trim())
        .bind(ctx.tenant_id.0)
        .execute(&mut *tx)
        .await;
    match done {
        Ok(done) if done.rows_affected() == 0 => {
            let _ = tx.rollback().await;
            return not_found("tenant");
        }
        Ok(_) => {}
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    }
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }
    (StatusCode::OK, Json(json!({ "name": body.name.trim() }))).into_response()
}

/// PUT /api/v1/settings/entities/:id
pub async fn put_entity(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<EntityBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "settings", "manage_company") {
        return forbidden;
    }
    if body.name.trim().is_empty() {
        return bad_request("name is required");
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let done = sqlx::query(
        "UPDATE entities
            SET name = $1, tax_id = $2, address = COALESCE($3, address), updated_at = now()
          WHERE id = $4 AND tenant_id = $5",
    )
    .bind(body.name.trim())
    .bind(body.tax_id.as_deref())
    .bind(body.address)
    .bind(id)
    .bind(ctx.tenant_id.0)
    .execute(&mut *tx)
    .await;
    match done {
        Ok(done) if done.rows_affected() == 0 => {
            let _ = tx.rollback().await;
            return not_found("entity");
        }
        Ok(_) => {}
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    }
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }
    (
        StatusCode::OK,
        Json(json!({ "id": id, "name": body.name.trim() })),
    )
        .into_response()
}

/// GET /api/v1/settings/system — operational readiness for the admin screen.
pub async fn system(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "settings", "view_company") {
        return forbidden;
    }

    use sqlx::Row;
    let role = sqlx::query(
        "SELECT current_user AS role, rolsuper, rolbypassrls
           FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(state.pool.pool())
    .await;
    let (role, superuser, bypass) = match role {
        Ok(row) => (
            row.get::<String, _>("role"),
            row.get::<bool, _>("rolsuper"),
            row.get::<bool, _>("rolbypassrls"),
        ),
        Err(e) => return internal_err(&e.to_string()),
    };

    // The tracking table only exists when migrations ran through sqlx-cli;
    // hand-applied dev databases have the schema but no ledger.
    let tracked: bool =
        sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations') IS NOT NULL")
            .fetch_one(state.pool.pool())
            .await
            .unwrap_or(false);
    let (applied, latest): (i64, Option<i64>) = if tracked {
        match sqlx::query(
            "SELECT count(*) AS applied, max(version) AS latest FROM _sqlx_migrations WHERE success",
        )
        .fetch_one(state.pool.pool())
        .await
        {
            Ok(row) => (row.get("applied"), row.get("latest")),
            Err(e) => return internal_err(&e.to_string()),
        }
    } else {
        (0, None)
    };

    let chrome = chromium_available();
    let pii_encryption = std::env::var("PII_ENCRYPTION_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);
    let model_gateway = std::env::var("HUAKATI_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);

    let set = |k: &str| std::env::var(k).is_ok_and(|v| !v.trim().is_empty());
    let email = set("EMAIL_API_KEY") && set("EMAIL_FROM");
    let line = set("LINE_CHANNEL_TOKEN");

    (
        StatusCode::OK,
        Json(json!({
            "database": {
                "role": role,
                "rls_enforced": !superuser && !bypass,
                "superuser": superuser,
                "bypass_rls": bypass,
            },
            "migrations": { "tracked": tracked, "applied": applied, "latest": latest },
            "payslip_renderer": { "available": chrome },
            "pii_encryption": { "configured": pii_encryption },
            "model_gateway": { "configured": model_gateway },
            "notifications": { "email": email, "line": line },
            "tenant_id": ctx.tenant_id.0,
        })),
    )
        .into_response()
}

fn chromium_available() -> bool {
    if let Ok(bin) = std::env::var("CHROME_BIN") {
        return std::path::Path::new(&bin).exists();
    }
    ["/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .iter()
        .any(|p| std::path::Path::new(p).exists())
}

fn bad_request(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, message)
}

fn not_found(what: &str) -> Response {
    error_response(StatusCode::NOT_FOUND, &format!("{what} not found"))
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "settings endpoint failed");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "An unexpected error occurred",
    )
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: "settings_error".into(),
            message: message.into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

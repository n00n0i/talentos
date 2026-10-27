//! Employee CRUD. Soft-delete only: payroll history must never point at a
//! vanished employee, so termination sets `status` + `termination_date`.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub q: String,
    /// `all` shows terminated employees too.
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct EmployeeBody {
    pub employee_code: String,
    pub first_name_th: String,
    pub last_name_th: String,
    pub first_name_en: Option<String>,
    pub last_name_en: Option<String>,
    pub position_title: String,
    pub department: Option<String>,
    pub base_salary: Decimal,
    pub hire_date: NaiveDate,
    pub entity_id: Option<Uuid>,
    pub tax_filing_status: Option<i16>,
    pub pf_member: Option<bool>,
    pub sso_enrolled: Option<bool>,
    pub bank_name: Option<String>,
    pub bank_account_number: Option<String>,
    pub national_id: Option<String>,
    pub sso_number: Option<String>,
    /// Link to a login account so the employee can see their own payslips.
    pub user_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct EmployeeDto {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub employee_code: String,
    pub first_name_th: String,
    pub last_name_th: String,
    pub first_name_en: Option<String>,
    pub last_name_en: Option<String>,
    pub position_title: String,
    pub department: Option<String>,
    pub base_salary: String,
    pub hire_date: String,
    pub termination_date: Option<String>,
    pub status: String,
    pub pf_member: bool,
    pub sso_enrolled: bool,
    /// Sensitive values are only ever returned masked.
    pub bank_name: Option<String>,
    pub bank_account_number_masked: Option<String>,
    pub national_id_masked: Option<String>,
    pub sso_number_masked: Option<String>,
    pub user_id: Option<Uuid>,
}

/// GET /api/v1/employees?q=&status=&limit=
pub async fn list(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<ListQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    let limit = query.limit.unwrap_or(200).clamp(1, 500);
    let status = query.status.unwrap_or_else(|| "active".to_string());
    let q = format!("%{}%", query.q.trim());

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let rows = sqlx::query(
        r#"SELECT id, entity_id, employee_code, first_name_th, last_name_th,
                  first_name_en, last_name_en, position_title, department,
                  base_salary, hire_date, termination_date, status, pf_member,
                  sso_enrolled, bank_name, bank_account_number, national_id, sso_number, user_id
             FROM employees
            WHERE tenant_id = $1
              AND ($2 = 'all' OR status = $2)
              AND ($3 = '%%' OR employee_code ILIKE $3 OR first_name_th ILIKE $3
                   OR last_name_th ILIKE $3 OR position_title ILIKE $3)
            ORDER BY employee_code
            LIMIT $4"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(&status)
    .bind(&q)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    match rows {
        Ok(rows) => {
            let employees: Vec<EmployeeDto> =
                rows.iter().map(|row| row_to_dto(row, &state.pii)).collect();
            (StatusCode::OK, Json(json!({ "employees": employees }))).into_response()
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

/// GET /api/v1/employees/:id
pub async fn get(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match fetch_one(&state, ctx.tenant_id.0, id).await {
        Ok(Some(employee)) => (StatusCode::OK, Json(employee)).into_response(),
        Ok(None) => not_found(),
        Err(e) => internal_err(&e),
    }
}

/// POST /api/v1/employees
pub async fn create(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<EmployeeBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "create") {
        return forbidden;
    }
    if let Some(message) = validate(&body) {
        return bad_request(&message);
    }
    let entity_id = match resolve_entity(&state, &ctx, body.entity_id).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    if let Some(user_id) = body.user_id {
        if let Err(response) = ensure_user_in_tenant(&state, &ctx, user_id).await {
            return response;
        }
    }
    let bank = match encrypt_field(&state.pii, body.bank_account_number.as_deref()) {
        Ok(v) => v,
        Err(response) => return response,
    };
    let national = match encrypt_field(&state.pii, body.national_id.as_deref()) {
        Ok(v) => v,
        Err(response) => return response,
    };
    let sso = match encrypt_field(&state.pii, body.sso_number.as_deref()) {
        Ok(v) => v,
        Err(response) => return response,
    };

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let id = Uuid::new_v4();
    let insert = sqlx::query(
        r#"INSERT INTO employees
             (id, tenant_id, entity_id, employee_code, first_name_th, last_name_th,
              first_name_en, last_name_en, position_title, department, base_salary,
              hire_date, tax_filing_status, pf_member, sso_enrolled,
              bank_name, bank_account_number, national_id, sso_number, user_id)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)"#,
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .bind(entity_id)
    .bind(body.employee_code.trim())
    .bind(body.first_name_th.trim())
    .bind(body.last_name_th.trim())
    .bind(body.first_name_en.as_deref())
    .bind(body.last_name_en.as_deref())
    .bind(body.position_title.trim())
    .bind(body.department.as_deref())
    .bind(body.base_salary)
    .bind(body.hire_date)
    .bind(body.tax_filing_status.unwrap_or(1))
    .bind(body.pf_member.unwrap_or(false))
    .bind(body.sso_enrolled.unwrap_or(true))
    .bind(body.bank_name.as_deref())
    .bind(bank.as_deref())
    .bind(national.as_deref())
    .bind(sso.as_deref())
    .bind(body.user_id)
    .execute(&mut *tx)
    .await;

    if let Err(e) = insert {
        let _ = tx.rollback().await;
        if is_unique_violation(&e) {
            return conflict("employee_code already exists for this entity");
        }
        return internal_err(&e.to_string());
    }

    let after = json!({ "employee_code": body.employee_code.trim() });
    let _ = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'employee.create', 'employee', $3, $4)"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0.to_string())
    .bind(id.to_string())
    .bind(after)
    .execute(&mut *tx)
    .await;

    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    match fetch_one(&state, ctx.tenant_id.0, id).await {
        Ok(Some(employee)) => (StatusCode::CREATED, Json(employee)).into_response(),
        Ok(None) => internal_err("employee not found after insert"),
        Err(e) => internal_err(&e),
    }
}

/// PUT /api/v1/employees/:id — replaces the editable fields (entity is fixed).
pub async fn update(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<EmployeeBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    if let Some(message) = validate(&body) {
        return bad_request(&message);
    }
    if let Some(user_id) = body.user_id {
        if let Err(response) = ensure_user_in_tenant(&state, &ctx, user_id).await {
            return response;
        }
    }
    let bank = match encrypt_field(&state.pii, body.bank_account_number.as_deref()) {
        Ok(v) => v,
        Err(response) => return response,
    };
    let national = match encrypt_field(&state.pii, body.national_id.as_deref()) {
        Ok(v) => v,
        Err(response) => return response,
    };
    let sso = match encrypt_field(&state.pii, body.sso_number.as_deref()) {
        Ok(v) => v,
        Err(response) => return response,
    };

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let result = sqlx::query(
        r#"UPDATE employees SET
             employee_code = $3, first_name_th = $4, last_name_th = $5,
             first_name_en = $6, last_name_en = $7, position_title = $8,
             department = $9, base_salary = $10, hire_date = $11,
             tax_filing_status = $12, pf_member = $13, sso_enrolled = $14,
             bank_name = $15,
             bank_account_number = COALESCE($16, bank_account_number),
             national_id = COALESCE($17, national_id),
             sso_number = COALESCE($18, sso_number),
             user_id = $19,
             updated_at = now()
           WHERE id = $1 AND tenant_id = $2"#,
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .bind(body.employee_code.trim())
    .bind(body.first_name_th.trim())
    .bind(body.last_name_th.trim())
    .bind(body.first_name_en.as_deref())
    .bind(body.last_name_en.as_deref())
    .bind(body.position_title.trim())
    .bind(body.department.as_deref())
    .bind(body.base_salary)
    .bind(body.hire_date)
    .bind(body.tax_filing_status.unwrap_or(1))
    .bind(body.pf_member.unwrap_or(false))
    .bind(body.sso_enrolled.unwrap_or(true))
    .bind(body.bank_name.as_deref())
    .bind(bank.as_deref())
    .bind(national.as_deref())
    .bind(sso.as_deref())
    .bind(body.user_id)
    .execute(&mut *tx)
    .await;

    match result {
        Ok(done) if done.rows_affected() == 0 => {
            let _ = tx.rollback().await;
            return not_found();
        }
        Ok(_) => {}
        Err(e) => {
            let _ = tx.rollback().await;
            if is_unique_violation(&e) {
                return conflict("employee_code already exists for this entity");
            }
            return internal_err(&e.to_string());
        }
    }

    let _ = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'employee.update', 'employee', $3, $4)"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0.to_string())
    .bind(id.to_string())
    .bind(json!({ "employee_code": body.employee_code.trim() }))
    .execute(&mut *tx)
    .await;

    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    match fetch_one(&state, ctx.tenant_id.0, id).await {
        Ok(Some(employee)) => (StatusCode::OK, Json(employee)).into_response(),
        Ok(None) => not_found(),
        Err(e) => internal_err(&e),
    }
}

/// DELETE /api/v1/employees/:id — soft terminate (history stays intact).
pub async fn terminate(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "delete") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let done = sqlx::query(
        r#"UPDATE employees
              SET status = 'terminated',
                  termination_date = COALESCE(termination_date, CURRENT_DATE),
                  updated_at = now()
            WHERE id = $1 AND tenant_id = $2"#,
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .execute(&mut *tx)
    .await;

    match done {
        Ok(done) if done.rows_affected() == 0 => {
            let _ = tx.rollback().await;
            return not_found();
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
           VALUES ($1, 'user', $2, 'employee.terminate', 'employee', $3, '{}'::jsonb)"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0.to_string())
    .bind(id.to_string())
    .execute(&mut *tx)
    .await;

    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }
    (
        StatusCode::OK,
        Json(json!({ "id": id, "status": "terminated" })),
    )
        .into_response()
}

// Same axum trade-off as the RBAC guard: a ready-to-send response is the error.
#[allow(clippy::result_large_err)]
async fn resolve_entity(
    state: &AppState,
    ctx: &talentos_kernel::TenantContext,
    requested: Option<Uuid>,
) -> Result<Uuid, Response> {
    if let Some(id) = requested.or(ctx.entity_id.map(|e| e.0)) {
        return Ok(id);
    }
    // Tenant-wide token (no entity claim): only unambiguous when the tenant has
    // exactly one active entity.
    let mut tx = state
        .pool
        .begin_tenant(ctx.tenant_id)
        .await
        .map_err(|e| internal_err(&e.to_string()))?;
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM entities WHERE tenant_id = $1 AND is_active ORDER BY created_at LIMIT 2",
    )
    .bind(ctx.tenant_id.0)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| internal_err(&e.to_string()))?;
    let _ = tx.rollback().await;

    match ids.as_slice() {
        [only] => Ok(*only),
        _ => Err(bad_request(
            "entity_id is required when the token spans multiple entities",
        )),
    }
}

// Same axum trade-off: a ready-to-send response is the error.
#[allow(clippy::result_large_err)]
async fn ensure_user_in_tenant(
    state: &AppState,
    ctx: &talentos_kernel::TenantContext,
    user_id: Uuid,
) -> Result<(), Response> {
    let mut tx = state
        .pool
        .begin_tenant(ctx.tenant_id)
        .await
        .map_err(|e| internal_err(&e.to_string()))?;
    let found: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE id = $1 AND tenant_id = $2 AND is_active")
            .bind(user_id)
            .bind(ctx.tenant_id.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| internal_err(&e.to_string()))?;
    let _ = tx.rollback().await;
    if found.is_none() {
        return Err(bad_request("user_id does not belong to this tenant"));
    }
    Ok(())
}

async fn fetch_one(
    state: &AppState,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<EmployeeDto>, String> {
    let mut tx = state
        .pool
        .begin_tenant(talentos_kernel::TenantId(tenant_id))
        .await
        .map_err(|e| e.to_string())?;
    let row = sqlx::query(
        r#"SELECT id, entity_id, employee_code, first_name_th, last_name_th,
                  first_name_en, last_name_en, position_title, department,
                  base_salary, hire_date, termination_date, status, pf_member,
                  sso_enrolled, bank_name, bank_account_number, national_id, sso_number, user_id
             FROM employees WHERE id = $1 AND tenant_id = $2"#,
    )
    .bind(id)
    .bind(tenant_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    let _ = tx.rollback().await;
    Ok(row.as_ref().map(|row| row_to_dto(row, &state.pii)))
}

fn row_to_dto(
    row: &sqlx::postgres::PgRow,
    pii: &Option<std::sync::Arc<talentos_db::pii::PiiCipher>>,
) -> EmployeeDto {
    use sqlx::Row;
    let base_salary: Decimal = row.get("base_salary");
    EmployeeDto {
        id: row.get("id"),
        entity_id: row.get("entity_id"),
        employee_code: row.get("employee_code"),
        first_name_th: row.get("first_name_th"),
        last_name_th: row.get("last_name_th"),
        first_name_en: row.get("first_name_en"),
        last_name_en: row.get("last_name_en"),
        position_title: row.get("position_title"),
        department: row.get("department"),
        base_salary: base_salary.to_string(),
        hire_date: row.get::<NaiveDate, _>("hire_date").to_string(),
        termination_date: row
            .get::<Option<NaiveDate>, _>("termination_date")
            .map(|d| d.to_string()),
        status: row.get("status"),
        pf_member: row.get("pf_member"),
        sso_enrolled: row.get("sso_enrolled"),
        bank_name: row.get("bank_name"),
        bank_account_number_masked: mask_value(pii, row.get("bank_account_number")),
        national_id_masked: mask_value(pii, row.get("national_id")),
        sso_number_masked: mask_value(pii, row.get("sso_number")),
        user_id: row.get("user_id"),
    }
}

fn mask_value(
    pii: &Option<std::sync::Arc<talentos_db::pii::PiiCipher>>,
    stored: Option<String>,
) -> Option<String> {
    let stored = stored?;
    match pii {
        Some(cipher) => cipher
            .decrypt(&stored)
            .ok()
            .map(|plain| talentos_db::pii::mask_secret(&plain)),
        None if talentos_db::pii::is_encrypted(&stored) => None,
        None => Some(talentos_db::pii::mask_secret(&stored)),
    }
}

// Same axum trade-off as the other guards: a ready-to-send response is the error.
#[allow(clippy::result_large_err)]
fn encrypt_field(
    pii: &Option<std::sync::Arc<talentos_db::pii::PiiCipher>>,
    value: Option<&str>,
) -> Result<Option<String>, Response> {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    match pii {
        Some(cipher) => cipher
            .encrypt(value)
            .map(Some)
            .map_err(|e| internal_err(&e.to_string())),
        None => Ok(Some(value.to_string())),
    }
}

fn validate(body: &EmployeeBody) -> Option<String> {
    if body.employee_code.trim().is_empty() {
        return Some("employee_code is required".into());
    }
    if body.first_name_th.trim().is_empty() || body.last_name_th.trim().is_empty() {
        return Some("first_name_th and last_name_th are required".into());
    }
    if body.position_title.trim().is_empty() {
        return Some("position_title is required".into());
    }
    if body.base_salary < Decimal::ZERO {
        return Some("base_salary must be >= 0".into());
    }
    if !(1..=5).contains(&body.tax_filing_status.unwrap_or(1)) {
        return Some("tax_filing_status must be 1..=5".into());
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

fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "employee not found")
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "employee endpoint failed");
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

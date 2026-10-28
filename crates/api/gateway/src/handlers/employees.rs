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
use std::str::FromStr;
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
    pub cost_center: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct ImportEmployeeRow {
    pub employee_code: Option<String>,
    pub first_name_th: Option<String>,
    pub last_name_th: Option<String>,
    pub position_title: Option<String>,
    pub base_salary: Option<String>,
    pub hire_date: Option<String>,
    pub first_name_en: Option<String>,
    pub last_name_en: Option<String>,
    pub department: Option<String>,
    pub entity_id: Option<Uuid>,
    pub tax_filing_status: Option<String>,
    pub pf_member: Option<String>,
    pub sso_enrolled: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportBody {
    #[serde(default)]
    pub dry_run: bool,
    pub rows: Vec<ImportEmployeeRow>,
}

#[derive(Debug, Serialize)]
pub struct ImportRowResult {
    pub row: usize,
    pub employee_code: String,
    pub status: String,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub dry_run: bool,
    pub total: usize,
    pub valid: usize,
    pub invalid: usize,
    pub created: usize,
    pub rows: Vec<ImportRowResult>,
}

#[derive(Debug, Clone)]
pub struct ParsedImportRow {
    pub employee_code: String,
    pub first_name_th: String,
    pub last_name_th: String,
    pub position_title: String,
    pub base_salary: Decimal,
    pub hire_date: NaiveDate,
    pub first_name_en: Option<String>,
    pub last_name_en: Option<String>,
    pub department: Option<String>,
    #[allow(dead_code)]
    pub entity_id: Option<Uuid>,
    pub tax_filing_status: i16,
    pub pf_member: bool,
    pub sso_enrolled: bool,
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
    pub cost_center: Option<String>,
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
                  first_name_en, last_name_en, position_title, department, cost_center,
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
              first_name_en, last_name_en, position_title, department, cost_center,
              base_salary, hire_date, tax_filing_status, pf_member, sso_enrolled,
              bank_name, bank_account_number, national_id, sso_number, user_id)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)"#,
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
    .bind(body.cost_center.as_deref())
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

/// POST /api/v1/employees/import
pub async fn import(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<ImportBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "create") {
        return forbidden;
    }

    let (mut results, parsed_rows) = validate_import_rows(&body.rows);
    let (resolved, entity_errors) = match resolve_import_entities(&state, &ctx, &body.rows).await {
        Ok(v) => v,
        Err(response) => return response,
    };

    for (idx, error) in entity_errors.into_iter().enumerate() {
        if let Some(message) = error {
            results[idx].errors.push(message);
            results[idx].status = "invalid".to_string();
        }
    }

    let target_entities: Vec<Uuid> = resolved
        .iter()
        .filter_map(|id| *id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    if !target_entities.is_empty() {
        let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
            Ok(tx) => tx,
            Err(e) => return internal_err(&e.to_string()),
        };
        let existing = sqlx::query(
            "SELECT entity_id, employee_code FROM employees WHERE tenant_id = $1 AND entity_id = ANY($2)",
        )
        .bind(ctx.tenant_id.0)
        .bind(&target_entities)
        .fetch_all(&mut *tx)
        .await;
        let _ = tx.rollback().await;

        let existing: Vec<(Uuid, String)> = match existing {
            Ok(rows) => {
                use sqlx::Row;
                rows.iter()
                    .map(|r| {
                        let entity_id: Uuid = r.get("entity_id");
                        let code: String = r.get("employee_code");
                        (entity_id, code)
                    })
                    .collect()
            }
            Err(e) => return internal_err(&e.to_string()),
        };

        for (idx, parsed) in parsed_rows.iter().enumerate() {
            if let Some(parsed) = parsed {
                if let Some(entity_id) = resolved[idx] {
                    if existing
                        .iter()
                        .any(|(e, c)| *e == entity_id && c == &parsed.employee_code)
                    {
                        results[idx].errors.push(format!(
                            "employee_code {} already exists",
                            parsed.employee_code
                        ));
                        results[idx].status = "invalid".to_string();
                    }
                }
            }
        }
    }

    let report = report_from_rows(body.dry_run, results);

    if body.dry_run || report.invalid > 0 {
        return (StatusCode::OK, Json(report)).into_response();
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };

    for (idx, parsed) in parsed_rows.iter().enumerate() {
        let Some(entity_id) = resolved[idx] else {
            let _ = tx.rollback().await;
            return internal_err("row without resolved entity reached commit");
        };
        let Some(parsed) = parsed else {
            let _ = tx.rollback().await;
            return internal_err("unparsed row reached commit");
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
        .bind(&parsed.employee_code)
        .bind(&parsed.first_name_th)
        .bind(&parsed.last_name_th)
        .bind(parsed.first_name_en.as_deref())
        .bind(parsed.last_name_en.as_deref())
        .bind(&parsed.position_title)
        .bind(parsed.department.as_deref())
        .bind(parsed.base_salary)
        .bind(parsed.hire_date)
        .bind(parsed.tax_filing_status)
        .bind(parsed.pf_member)
        .bind(parsed.sso_enrolled)
        .bind(None::<&str>)
        .bind(None::<&str>)
        .bind(None::<&str>)
        .bind(None::<&str>)
        .bind(None::<Uuid>)
        .execute(&mut *tx)
        .await;

        if let Err(e) = insert {
            let _ = tx.rollback().await;
            if is_unique_violation(&e) {
                return conflict("employee_code already exists for this entity");
            }
            return internal_err(&e.to_string());
        }

        let after = json!({ "employee_code": parsed.employee_code });
        let audit = sqlx::query(
            r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'employee.import', 'employee', $3, $4)"#,
        )
        .bind(ctx.tenant_id.0)
        .bind(ctx.user_id.0.to_string())
        .bind(id.to_string())
        .bind(after)
        .execute(&mut *tx)
        .await;

        if let Err(e) = audit {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    }

    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    (StatusCode::OK, Json(report)).into_response()
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
             cost_center = $20,
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
    .bind(body.cost_center.as_deref())
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
async fn resolve_import_entities(
    state: &AppState,
    ctx: &talentos_kernel::TenantContext,
    rows: &[ImportEmployeeRow],
) -> Result<(Vec<Option<Uuid>>, Vec<Option<String>>), Response> {
    let mut tx = state
        .pool
        .begin_tenant(ctx.tenant_id)
        .await
        .map_err(|e| internal_err(&e.to_string()))?;
    let active_entities: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM entities WHERE tenant_id = $1 AND is_active")
            .bind(ctx.tenant_id.0)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| internal_err(&e.to_string()))?;
    let _ = tx.rollback().await;

    let active_set: std::collections::HashSet<Uuid> = active_entities.iter().copied().collect();

    let default_entity = if let Some(id) = ctx.entity_id {
        Some(id.0)
    } else if active_entities.len() == 1 {
        Some(active_entities[0])
    } else {
        None
    };

    let mut resolved = Vec::with_capacity(rows.len());
    let mut errors = Vec::with_capacity(rows.len());

    for row in rows {
        let entity_id = row.entity_id.or(default_entity);
        let error = if let Some(id) = entity_id {
            if active_set.contains(&id) {
                None
            } else {
                Some("entity not in tenant".into())
            }
        } else {
            Some("entity_id is required when the token spans multiple entities".into())
        };
        resolved.push(entity_id);
        errors.push(error);
    }

    Ok((resolved, errors))
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
                  first_name_en, last_name_en, position_title, department, cost_center,
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
        cost_center: row.get("cost_center"),
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

fn validate_fields(
    employee_code: &str,
    first_name_th: &str,
    last_name_th: &str,
    position_title: &str,
    base_salary: Decimal,
    tax_filing_status: Option<i16>,
) -> Option<String> {
    if employee_code.trim().is_empty() {
        return Some("employee_code is required".into());
    }
    if first_name_th.trim().is_empty() || last_name_th.trim().is_empty() {
        return Some("first_name_th and last_name_th are required".into());
    }
    if position_title.trim().is_empty() {
        return Some("position_title is required".into());
    }
    if base_salary < Decimal::ZERO {
        return Some("base_salary must be >= 0".into());
    }
    if !(1..=5).contains(&tax_filing_status.unwrap_or(1)) {
        return Some("tax_filing_status must be 1..=5".into());
    }
    None
}

fn validate(body: &EmployeeBody) -> Option<String> {
    validate_fields(
        &body.employee_code,
        &body.first_name_th,
        &body.last_name_th,
        &body.position_title,
        body.base_salary,
        body.tax_filing_status,
    )
}

fn parse_bool(value: Option<&str>, default: bool, name: &str) -> Result<bool, String> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(default),
        Some(v) => match v.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" => Ok(false),
            _ => Err(format!(
                "{name} must be true/false, 1/0, or yes/no (got {v})"
            )),
        },
    }
}

fn parse_import_row(row: &ImportEmployeeRow) -> Result<ParsedImportRow, Vec<String>> {
    let mut errors = Vec::new();

    let required = |value: &Option<String>, _name: &str| -> Option<String> {
        value
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
    };

    let employee_code = required(&row.employee_code, "employee_code");
    if employee_code.is_none() {
        errors.push("employee_code is required".into());
    }

    let first_name_th = required(&row.first_name_th, "first_name_th");
    if first_name_th.is_none() {
        errors.push("first_name_th is required".into());
    }

    let last_name_th = required(&row.last_name_th, "last_name_th");
    if last_name_th.is_none() {
        errors.push("last_name_th is required".into());
    }

    let position_title = required(&row.position_title, "position_title");
    if position_title.is_none() {
        errors.push("position_title is required".into());
    }

    let base_salary = match row
        .base_salary
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(v) => match Decimal::from_str(v) {
            Ok(d) if d < Decimal::ZERO => {
                errors.push("base_salary must be >= 0".into());
                None
            }
            Ok(d) if d > Decimal::new(999_999_999_999_i64, 2) => {
                errors.push("base_salary exceeds the maximum".into());
                None
            }
            Ok(d) if d.round_dp(2) != d => {
                errors.push("base_salary must have at most 2 decimal places".into());
                None
            }
            Ok(d) => Some(d),
            Err(_) => {
                errors.push("base_salary must be a number".into());
                None
            }
        },
        None => {
            errors.push("base_salary is required".into());
            None
        }
    };

    let hire_date = match row
        .hire_date
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(v) => match NaiveDate::parse_from_str(v, "%Y-%m-%d") {
            Ok(d) => Some(d),
            Err(_) => {
                errors.push("hire_date must be YYYY-MM-DD".into());
                None
            }
        },
        None => {
            errors.push("hire_date is required".into());
            None
        }
    };

    let tax_filing_status = match row
        .tax_filing_status
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(v) => match v.parse::<i16>() {
            Ok(n) if (1..=5).contains(&n) => Some(n),
            Ok(_) => {
                errors.push("tax_filing_status must be 1..=5".into());
                None
            }
            Err(_) => {
                errors.push("tax_filing_status must be a number".into());
                None
            }
        },
        None => Some(1),
    };

    let pf_member = match parse_bool(row.pf_member.as_deref(), false, "pf_member") {
        Ok(v) => Some(v),
        Err(e) => {
            errors.push(e);
            None
        }
    };

    let sso_enrolled = match parse_bool(row.sso_enrolled.as_deref(), true, "sso_enrolled") {
        Ok(v) => Some(v),
        Err(e) => {
            errors.push(e);
            None
        }
    };

    let optional_text = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
    };

    let first_name_en = optional_text(&row.first_name_en);
    let last_name_en = optional_text(&row.last_name_en);
    let department = optional_text(&row.department);

    if !errors.is_empty() {
        return Err(errors);
    }

    let (
        Some(employee_code),
        Some(first_name_th),
        Some(last_name_th),
        Some(position_title),
        Some(base_salary),
        Some(hire_date),
        Some(tax_filing_status),
        Some(pf_member),
        Some(sso_enrolled),
    ) = (
        employee_code,
        first_name_th,
        last_name_th,
        position_title,
        base_salary,
        hire_date,
        tax_filing_status,
        pf_member,
        sso_enrolled,
    )
    else {
        return Err(errors);
    };

    Ok(ParsedImportRow {
        employee_code,
        first_name_th,
        last_name_th,
        position_title,
        base_salary,
        hire_date,
        first_name_en,
        last_name_en,
        department,
        entity_id: row.entity_id,
        tax_filing_status,
        pf_member,
        sso_enrolled,
    })
}

fn validate_import_rows(
    rows: &[ImportEmployeeRow],
) -> (Vec<ImportRowResult>, Vec<Option<ParsedImportRow>>) {
    let parsed: Vec<Result<ParsedImportRow, Vec<String>>> =
        rows.iter().map(parse_import_row).collect();

    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for raw in rows {
        if let Some(code) = raw
            .employee_code
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            *counts.entry(code.to_string()).or_insert(0) += 1;
        }
    }

    let results = rows
        .iter()
        .zip(parsed.iter())
        .enumerate()
        .map(|(idx, (raw, parsed))| {
            let mut errors = match parsed {
                Err(e) => e.clone(),
                Ok(_) => Vec::new(),
            };
            if let Some(code) = raw
                .employee_code
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                if counts.get(code).copied().unwrap_or(0) > 1 {
                    errors.push(format!("employee_code {code} is duplicated in file"));
                }
            }
            let status = if errors.is_empty() {
                "valid".to_string()
            } else {
                "invalid".to_string()
            };
            ImportRowResult {
                row: idx + 1,
                employee_code: raw.employee_code.clone().unwrap_or_default(),
                status,
                errors,
            }
        })
        .collect();

    let parsed_options = parsed.into_iter().map(Result::ok).collect();
    (results, parsed_options)
}

fn report_from_rows(dry_run: bool, rows: Vec<ImportRowResult>) -> ImportReport {
    let total = rows.len();
    let invalid = rows.iter().filter(|r| r.status == "invalid").count();
    let valid = total - invalid;
    let created = if dry_run || invalid > 0 { 0 } else { valid };
    ImportReport {
        dry_run,
        total,
        valid,
        invalid,
        created,
        rows,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn row(code: &str, salary: &str, hire_date: &str, tax: Option<&str>) -> ImportEmployeeRow {
        ImportEmployeeRow {
            employee_code: Some(code.into()),
            first_name_th: Some("สมชาย".into()),
            last_name_th: Some("ใจดี".into()),
            position_title: Some("Dev".into()),
            base_salary: Some(salary.into()),
            hire_date: Some(hire_date.into()),
            first_name_en: None,
            last_name_en: None,
            department: None,
            entity_id: None,
            tax_filing_status: tax.map(|s| s.into()),
            pf_member: None,
            sso_enrolled: None,
        }
    }

    #[test]
    fn import_detects_duplicate_code_inside_file() {
        let rows = vec![
            row("E001", "1", "2024-01-01", None),
            row("E001", "1", "2024-01-01", None),
        ];
        let (results, _) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "invalid");
        assert_eq!(results[1].status, "invalid");
        assert!(results[1].errors.iter().any(|e| e.contains("duplicated")));
    }

    #[test]
    fn import_treats_whitespace_variants_as_duplicates() {
        let rows = vec![
            row("E001", "1", "2024-01-01", None),
            row(" E001 ", "1", "2024-01-01", None),
        ];
        let (results, _) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "invalid");
        assert_eq!(results[1].status, "invalid");
    }

    #[test]
    fn import_rejects_blank_employee_code() {
        let rows = vec![row("  ", "1", "2024-01-01", None)];
        let (results, _) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "invalid");
        assert!(results[0]
            .errors
            .iter()
            .any(|e| e.contains("employee_code is required")));
    }

    #[test]
    fn import_rejects_blank_required_cell() {
        let rows = vec![ImportEmployeeRow {
            employee_code: Some("E002".into()),
            first_name_th: None,
            last_name_th: Some("ใจดี".into()),
            position_title: Some("Dev".into()),
            base_salary: Some("1".into()),
            hire_date: Some("2024-01-01".into()),
            first_name_en: None,
            last_name_en: None,
            department: None,
            entity_id: None,
            tax_filing_status: None,
            pf_member: None,
            sso_enrolled: None,
        }];
        let (results, _) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "invalid");
        assert!(results[0]
            .errors
            .iter()
            .any(|e| e.contains("first_name_th is required")));
    }

    #[test]
    fn import_rejects_negative_salary() {
        let rows = vec![row("E003", "-1", "2024-01-01", None)];
        let (results, _) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "invalid");
        assert!(results[0].errors.iter().any(|e| e.contains("base_salary")));
    }

    #[test]
    fn import_rejects_non_numeric_salary() {
        let rows = vec![row("E004", "abc", "2024-01-01", None)];
        let (results, _) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "invalid");
        assert!(results[0]
            .errors
            .iter()
            .any(|e| e.contains("base_salary must be a number")));
    }

    #[test]
    fn import_rejects_bad_date() {
        let rows = vec![row("E005", "1", "01-01-2024", None)];
        let (results, _) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "invalid");
        assert!(results[0]
            .errors
            .iter()
            .any(|e| e.contains("hire_date must be YYYY-MM-DD")));
    }

    #[test]
    fn import_rejects_bad_tax_filing_status_number() {
        let rows = vec![row("E006", "1", "2024-01-01", Some("7"))];
        let (results, _) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "invalid");
        assert!(results[0]
            .errors
            .iter()
            .any(|e| e.contains("tax_filing_status")));
    }

    #[test]
    fn import_rejects_non_numeric_tax_filing_status() {
        let rows = vec![row("E007", "1", "2024-01-01", Some("abc"))];
        let (results, _) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "invalid");
        assert!(results[0]
            .errors
            .iter()
            .any(|e| e.contains("tax_filing_status must be a number")));
    }

    #[test]
    fn import_preserves_entity_id_in_parsed_row() {
        let id = Uuid::new_v4();
        let rows = vec![ImportEmployeeRow {
            employee_code: Some("E012".into()),
            first_name_th: Some("สมชาย".into()),
            last_name_th: Some("ใจดี".into()),
            position_title: Some("Dev".into()),
            base_salary: Some("1".into()),
            hire_date: Some("2024-01-01".into()),
            first_name_en: None,
            last_name_en: None,
            department: None,
            entity_id: Some(id),
            tax_filing_status: None,
            pf_member: None,
            sso_enrolled: None,
        }];
        let (_, parsed) = validate_import_rows(&rows);
        assert_eq!(parsed[0].as_ref().unwrap().entity_id, Some(id));
    }

    #[test]
    fn import_accepts_common_boolean_strings() {
        let rows = vec![
            ImportEmployeeRow {
                employee_code: Some("E008".into()),
                first_name_th: Some("สมชาย".into()),
                last_name_th: Some("ใจดี".into()),
                position_title: Some("Dev".into()),
                base_salary: Some("1".into()),
                hire_date: Some("2024-01-01".into()),
                first_name_en: None,
                last_name_en: None,
                department: None,
                entity_id: None,
                tax_filing_status: None,
                pf_member: Some("yes".into()),
                sso_enrolled: Some("1".into()),
            },
            ImportEmployeeRow {
                employee_code: Some("E009".into()),
                first_name_th: Some("สมชาย".into()),
                last_name_th: Some("ใจดี".into()),
                position_title: Some("Dev".into()),
                base_salary: Some("1".into()),
                hire_date: Some("2024-01-01".into()),
                first_name_en: None,
                last_name_en: None,
                department: None,
                entity_id: None,
                tax_filing_status: None,
                pf_member: Some("no".into()),
                sso_enrolled: Some("0".into()),
            },
        ];
        let (results, parsed) = validate_import_rows(&rows);
        assert!(results.iter().all(|r| r.status == "valid"));
        assert!(parsed[0].as_ref().unwrap().pf_member);
        assert!(parsed[0].as_ref().unwrap().sso_enrolled);
        assert!(!parsed[1].as_ref().unwrap().pf_member);
        assert!(!parsed[1].as_ref().unwrap().sso_enrolled);
    }

    #[test]
    fn import_valid_rows_pass() {
        let rows = vec![
            row("E010", "1", "2024-01-01", Some("2")),
            row("E011", "0", "2024-01-01", None),
        ];
        let (results, _) = validate_import_rows(&rows);
        assert!(results.iter().all(|r| r.status == "valid"));
    }

    #[test]
    fn import_rejects_salary_over_maximum() {
        let rows = vec![row("E013", "10000000000", "2024-01-01", None)];
        let (results, _) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "invalid");
        assert!(results[0]
            .errors
            .iter()
            .any(|e| e.contains("base_salary exceeds the maximum")));
    }

    #[test]
    fn import_rejects_salary_with_more_than_2_decimals() {
        let rows = vec![row("E014", "100.999", "2024-01-01", None)];
        let (results, _) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "invalid");
        assert!(results[0]
            .errors
            .iter()
            .any(|e| e.contains("base_salary must have at most 2 decimal places")));
    }

    #[test]
    fn import_accepts_salary_with_2_decimals() {
        let rows = vec![row("E015", "100.50", "2024-01-01", None)];
        let (results, parsed) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "valid");
        assert_eq!(
            parsed[0].as_ref().unwrap().base_salary,
            Decimal::from_str("100.50").unwrap()
        );
    }

    #[test]
    fn import_accepts_max_salary() {
        let rows = vec![row("E016", "9999999999.99", "2024-01-01", None)];
        let (results, parsed) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "valid");
        assert_eq!(
            parsed[0].as_ref().unwrap().base_salary,
            Decimal::from_str("9999999999.99").unwrap()
        );
    }

    #[test]
    fn import_detects_duplicates_even_when_other_fields_invalid() {
        let rows = vec![
            row("E017", "abc", "2024-01-01", None),
            row("E017", "1", "2024-01-01", None),
        ];
        let (results, _) = validate_import_rows(&rows);
        assert_eq!(results[0].status, "invalid");
        assert_eq!(results[1].status, "invalid");
        assert!(results[0]
            .errors
            .iter()
            .any(|e| e.contains("duplicated") || e.contains("base_salary")));
        assert!(results[1].errors.iter().any(|e| e.contains("duplicated")));
    }

    #[test]
    fn report_from_rows_commit_with_invalid_yields_zero_created() {
        let rows = vec![
            ImportRowResult {
                row: 1,
                employee_code: "A".into(),
                status: "valid".into(),
                errors: vec![],
            },
            ImportRowResult {
                row: 2,
                employee_code: "B".into(),
                status: "invalid".into(),
                errors: vec!["bad".into()],
            },
        ];
        let report = report_from_rows(false, rows);
        assert_eq!(report.total, 2);
        assert_eq!(report.valid, 1);
        assert!(report.valid < report.total);
        assert_eq!(report.invalid, 1);
        assert_eq!(report.created, 0);
        assert!(!report.dry_run);
    }

    #[test]
    fn report_counts_are_correct() {
        let rows = vec![
            ImportRowResult {
                row: 1,
                employee_code: "A".into(),
                status: "valid".into(),
                errors: vec![],
            },
            ImportRowResult {
                row: 2,
                employee_code: "B".into(),
                status: "invalid".into(),
                errors: vec!["bad".into()],
            },
        ];
        let report = report_from_rows(true, rows);
        assert_eq!(report.total, 2);
        assert_eq!(report.valid, 1);
        assert_eq!(report.invalid, 1);
        assert_eq!(report.created, 0);
        assert!(report.dry_run);

        let rows = vec![ImportRowResult {
            row: 1,
            employee_code: "A".into(),
            status: "valid".into(),
            errors: vec![],
        }];
        let report = report_from_rows(false, rows);
        assert_eq!(report.created, 1);
    }
}

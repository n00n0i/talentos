//! Leave endpoints: self-service requests, balances, and approval decisions.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use std::str::FromStr;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ApproveLeaveRequest {
    pub approve: bool,
    /// Optional note; persisted with the approval audit entry.
    #[allow(dead_code)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateLeaveBody {
    pub leave_type: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub reason: Option<String>,
    pub employee_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct EmployeeQuery {
    pub employee_id: Option<Uuid>,
    pub year: Option<i32>,
    pub status: Option<String>,
}

/// Employees may only submit for their own linked employee; submitting for
/// another employee requires team/HR view permission.
#[allow(clippy::result_large_err)]
fn authorize_create_for_employee(
    ctx: &talentos_kernel::TenantContext,
    requested: Option<Uuid>,
    own_employee: Option<Uuid>,
) -> Result<(), Response> {
    require(ctx, "leave_request", "submit_self")?;
    // Submitting for someone else needs team/HR rights. An unlinked HR account
    // has no own employee record and must still be allowed through view_team.
    if let Some(requested_employee) = requested {
        if own_employee != Some(requested_employee) {
            require(ctx, "leave_request", "view_team")?;
        }
    }
    Ok(())
}

/// POST /api/v1/leave/requests — create a request for self (or a given employee).
pub async fn create(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<CreateLeaveBody>,
) -> Response {
    let own_employee =
        match talentos_hr_runner::employee_of_user(&state.pool, ctx.tenant_id, ctx.user_id.0).await
        {
            Ok(id) => id,
            Err(e) => return internal_err(&e.to_string()),
        };

    if let Err(forbidden) = authorize_create_for_employee(&ctx, body.employee_id, own_employee) {
        return forbidden;
    }

    let input = talentos_hr_runner::LeaveInput {
        leave_type: body.leave_type,
        start_date: body.start_date,
        end_date: body.end_date,
        reason: body.reason,
        employee_id: body.employee_id,
    };

    match talentos_hr_runner::create_request(&state.pool, ctx.tenant_id, ctx.user_id.0, input).await
    {
        Ok(view) => (StatusCode::CREATED, Json(view)).into_response(),
        Err(e) => map_leave_error(e),
    }
}

/// GET /api/v1/leave/balances?employee_id=&year=
pub async fn balances(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<EmployeeQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_request", "view_team") {
        return forbidden;
    }
    let employee_id =
        match resolve_employee(&state, ctx.tenant_id, ctx.user_id.0, q.employee_id).await {
            Ok(Some(id)) => id,
            Ok(None) => return not_found(),
            Err(e) => return internal_err(&e),
        };
    let year = q.year.unwrap_or_else(|| chrono::Utc::now().year());

    match talentos_hr_runner::list_balances(&state.pool, ctx.tenant_id, employee_id, year).await {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => map_leave_error(e),
    }
}

/// GET /api/v1/leave/requests?employee_id=&status=
pub async fn list(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<EmployeeQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_request", "view_team") {
        return forbidden;
    }
    let employee_id =
        match resolve_employee(&state, ctx.tenant_id, ctx.user_id.0, q.employee_id).await {
            Ok(Some(id)) => id,
            Ok(None) => return not_found(),
            Err(e) => return internal_err(&e),
        };

    match talentos_hr_runner::list_requests(
        &state.pool,
        ctx.tenant_id,
        employee_id,
        q.status.as_deref(),
    )
    .await
    {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => map_leave_error(e),
    }
}

/// GET /api/v1/me/leave
pub async fn me_leave(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_request", "view_self") {
        return forbidden;
    }
    let employee_id =
        match talentos_hr_runner::employee_of_user(&state.pool, ctx.tenant_id, ctx.user_id.0).await
        {
            Ok(Some(id)) => id,
            Ok(None) => {
                return (
                    StatusCode::OK,
                    Json(json!({ "employee_linked": false, "balances": [], "requests": [] })),
                )
                    .into_response()
            }
            Err(e) => return internal_err(&e.to_string()),
        };

    // Thai business year (UTC+7) without a panic-prone offset conversion.
    let year = (chrono::Utc::now() + chrono::Duration::hours(7)).year();
    let balances = match talentos_hr_runner::list_balances(
        &state.pool,
        ctx.tenant_id,
        employee_id,
        year,
    )
    .await
    {
        Ok(list) => list,
        Err(e) => return map_leave_error(e),
    };
    let requests = match talentos_hr_runner::list_requests(
        &state.pool,
        ctx.tenant_id,
        employee_id,
        None,
    )
    .await
    {
        Ok(list) => list,
        Err(e) => return map_leave_error(e),
    };

    (
        StatusCode::OK,
        Json(json!({
            "employee_linked": true,
            "balances": balances,
            "requests": requests,
        })),
    )
        .into_response()
}

/// POST /api/v1/me/leave/:id/cancel
pub async fn me_cancel(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_request", "submit_self") {
        return forbidden;
    }
    match talentos_hr_runner::cancel_request(&state.pool, ctx.tenant_id, ctx.user_id.0, id).await {
        Ok(status) => (
            StatusCode::OK,
            Json(json!({ "request_id": id.to_string(), "status": status })),
        )
            .into_response(),
        Err(e) => map_leave_error(e),
    }
}

#[derive(Debug, Deserialize)]
pub struct BalanceImportRequestRow {
    pub employee_code: String,
    pub leave_type: String,
    pub year: String,
    pub total_days: String,
    pub used_days: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportBalancesBody {
    #[serde(default)]
    pub dry_run: bool,
    pub rows: Vec<BalanceImportRequestRow>,
}

/// POST /api/v1/leave/balances/import
pub async fn import_balances(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<ImportBalancesBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_balance", "manage") {
        return forbidden;
    }

    let mut service_rows = Vec::with_capacity(body.rows.len());
    let mut parse_results: Vec<Option<talentos_hr_runner::leave::BalanceImportRowResult>> =
        Vec::with_capacity(body.rows.len());
    let mut service_indices: Vec<usize> = Vec::with_capacity(body.rows.len());

    for (idx, raw) in body.rows.iter().enumerate() {
        let mut errors = Vec::new();
        let code = raw.employee_code.trim().to_string();
        let typ = raw.leave_type.trim().to_string();

        if code.is_empty() {
            errors.push("employee_code is required".into());
        }
        if typ.is_empty() {
            errors.push("leave_type is required".into());
        }

        let year = match raw.year.trim() {
            "" => {
                errors.push("year is required".into());
                None
            }
            s => match s.parse::<i32>() {
                Ok(y) => Some(y),
                Err(_) => {
                    errors.push("year must be an integer".into());
                    None
                }
            },
        };

        let total_days = parse_decimal_for_import(&raw.total_days, "total_days", &mut errors);
        let used_days = match raw.used_days.as_deref().map(str::trim) {
            None | Some("") => Some(Decimal::ZERO),
            Some(s) => parse_decimal_for_import(s, "used_days", &mut errors),
        };

        if errors.is_empty() {
            service_rows.push(talentos_hr_runner::leave::BalanceImportRow {
                employee_code: code,
                leave_type: typ,
                year: year.unwrap_or(0),
                total_days: total_days.unwrap_or(Decimal::ZERO),
                used_days,
            });
            service_indices.push(idx);
            parse_results.push(None);
        } else {
            parse_results.push(Some(talentos_hr_runner::leave::BalanceImportRowResult {
                row: idx + 1,
                employee_code: raw.employee_code.clone(),
                leave_type: raw.leave_type.clone(),
                year: year.unwrap_or(0),
                status: "invalid".into(),
                errors,
            }));
        }
    }

    let has_parse_errors = parse_results.iter().any(|r| r.is_some());
    // Even with parse errors, validate the clean rows so lookup errors surface in
    // the preview. Force a dry run so a broken file can never write.
    let service_report = match talentos_hr_runner::leave::import_balances(
        &state.pool,
        ctx.tenant_id,
        body.dry_run || has_parse_errors,
        service_rows,
    )
    .await
    {
        Ok(report) => report,
        Err(e) => return internal_err(&e.to_string()),
    };

    let mut service_iter = service_report.rows.iter();
    let rows: Vec<talentos_hr_runner::leave::BalanceImportRowResult> = parse_results
        .into_iter()
        .map(|parsed| match parsed {
            Some(parsed) => parsed,
            None => service_iter.next().cloned().unwrap_or(
                talentos_hr_runner::leave::BalanceImportRowResult {
                    row: 0,
                    employee_code: String::new(),
                    leave_type: String::new(),
                    year: 0,
                    status: "invalid".into(),
                    errors: vec!["row could not be validated".into()],
                },
            ),
        })
        .collect();

    let total = rows.len();
    let invalid = rows.iter().filter(|r| r.status == "invalid").count();
    let valid = total - invalid;
    let created = if body.dry_run || invalid > 0 {
        0
    } else {
        valid
    };

    let report = talentos_hr_runner::leave::BalanceImportReport {
        dry_run: body.dry_run,
        total,
        valid,
        invalid,
        created,
        rows,
    };

    (StatusCode::OK, Json(report)).into_response()
}

#[allow(dead_code)]
fn merge_balance_import_rows(
    parse_results: Vec<Option<talentos_hr_runner::leave::BalanceImportRowResult>>,
    service_rows: &[talentos_hr_runner::leave::BalanceImportRowResult],
) -> Vec<talentos_hr_runner::leave::BalanceImportRowResult> {
    let mut service_iter = service_rows.iter();
    parse_results
        .into_iter()
        .map(|parsed| match parsed {
            Some(parsed) => parsed,
            None => service_iter.next().cloned().unwrap_or(
                talentos_hr_runner::leave::BalanceImportRowResult {
                    row: 0,
                    employee_code: String::new(),
                    leave_type: String::new(),
                    year: 0,
                    status: "invalid".into(),
                    errors: vec!["row could not be validated".into()],
                },
            ),
        })
        .collect()
}

fn parse_decimal_for_import(value: &str, field: &str, errors: &mut Vec<String>) -> Option<Decimal> {
    match value.trim() {
        "" => {
            errors.push(format!("{field} is required"));
            None
        }
        s => match Decimal::from_str(s) {
            Ok(d) => Some(d),
            Err(_) => {
                errors.push(format!("{field} must be a number"));
                None
            }
        },
    }
}

/// GET /api/v1/leave/approvals
pub async fn approvals(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_request", "view_team") {
        return forbidden;
    }
    match talentos_hr_runner::pending_approvals(&state.pool, ctx.tenant_id).await {
        Ok(requests) => (StatusCode::OK, Json(json!({ "requests": requests }))).into_response(),
        Err(e) => map_leave_error(e),
    }
}

async fn resolve_employee(
    state: &AppState,
    tenant_id: talentos_kernel::TenantId,
    user_id: Uuid,
    requested: Option<Uuid>,
) -> Result<Option<Uuid>, String> {
    if requested.is_some() {
        return Ok(requested);
    }
    talentos_hr_runner::employee_of_user(&state.pool, tenant_id, user_id)
        .await
        .map_err(|e| e.to_string())
}

/// POST /api/v1/leave/requests/:id/decision
pub async fn decide(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<ApproveLeaveRequest>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_request", "approve_team") {
        return forbidden;
    }
    match talentos_hr_runner::decide_request(
        &state.pool,
        ctx.tenant_id,
        ctx.user_id.0,
        &ctx.roles,
        id,
        body.approve,
    )
    .await
    {
        Ok(outcome) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "request_id": id.to_string(),
                "status": outcome.status,
                "step": outcome.step,
                "total_steps": outcome.total_steps,
                "approver_id": ctx.user_id.to_string(),
            })),
        )
            .into_response(),
        Err(e) => map_leave_error(e),
    }
}

fn envelope(code: &str, message: &str) -> ErrorResponse {
    ErrorResponse {
        error: code.into(),
        message: message.into(),
        details: vec![],
        request_id: Uuid::new_v4().to_string(),
        retry_after_seconds: None,
    }
}

fn unprocessable(message: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(envelope("invalid_request", message)),
    )
        .into_response()
}

fn conflict(message: &str) -> Response {
    (StatusCode::CONFLICT, Json(envelope("conflict", message))).into_response()
}

fn map_leave_error(e: talentos_hr_runner::LeaveError) -> Response {
    match e {
        talentos_hr_runner::LeaveError::NotFound => not_found(),
        talentos_hr_runner::LeaveError::NotCancellable => {
            conflict("leave request cannot be cancelled in its current status")
        }
        talentos_hr_runner::LeaveError::Overlap(s, e) => {
            conflict(&format!("overlaps an existing leave from {s} to {e}"))
        }
        talentos_hr_runner::LeaveError::InsufficientBalance { .. } => unprocessable(&e.to_string()),
        talentos_hr_runner::LeaveError::UnknownType
        | talentos_hr_runner::LeaveError::BadRange
        | talentos_hr_runner::LeaveError::TooLong => unprocessable(&e.to_string()),
        talentos_hr_runner::LeaveError::Forbidden => (
            StatusCode::FORBIDDEN,
            Json(envelope(
                "forbidden",
                "approver is not authorised for this step",
            )),
        )
            .into_response(),
        talentos_hr_runner::LeaveError::EmployeeNotFound
        | talentos_hr_runner::LeaveError::NotActive => (
            StatusCode::FORBIDDEN,
            Json(envelope(
                "forbidden",
                "no active employee record for this account",
            )),
        )
            .into_response(),
        _ => internal_err(&e.to_string()),
    }
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(envelope("not_found", "not found")),
    )
        .into_response()
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "leave endpoint failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(envelope("internal_error", "An unexpected error occurred")),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use talentos_kernel::{TenantContext, TenantId, UserId};
    use uuid::Uuid;

    fn ctx_with_roles(roles: &[&str]) -> TenantContext {
        TenantContext {
            tenant_id: TenantId(Uuid::new_v4()),
            entity_id: None,
            user_id: UserId(Uuid::new_v4()),
            roles: roles.iter().map(|r| r.to_string()).collect(),
        }
    }

    #[test]
    fn employee_can_create_for_self() {
        let own = Uuid::new_v4();
        let ctx = ctx_with_roles(&["employee"]);
        assert!(authorize_create_for_employee(&ctx, None, Some(own)).is_ok());
        assert!(authorize_create_for_employee(&ctx, Some(own), Some(own)).is_ok());
    }

    #[test]
    fn employee_cannot_create_for_another_employee() {
        let own = Uuid::new_v4();
        let other = Uuid::new_v4();
        let ctx = ctx_with_roles(&["employee"]);
        assert!(authorize_create_for_employee(&ctx, Some(other), Some(own)).is_err());
    }

    #[test]
    fn manager_can_create_for_another_employee() {
        let own = Uuid::new_v4();
        let other = Uuid::new_v4();
        let ctx = ctx_with_roles(&["manager"]);
        assert!(authorize_create_for_employee(&ctx, Some(other), Some(own)).is_ok());
    }

    #[test]
    fn unlinked_hr_can_create_for_another() {
        let ctx = ctx_with_roles(&["hr_admin"]);
        let other = uuid::Uuid::new_v4();
        assert!(authorize_create_for_employee(&ctx, Some(other), None).is_ok());
    }

    #[test]
    fn unlinked_employee_cannot_create_for_another() {
        let other = Uuid::new_v4();
        let ctx = ctx_with_roles(&["employee"]);
        assert!(authorize_create_for_employee(&ctx, Some(other), None).is_err());
    }
}

#[cfg(test)]
mod balance_import_merge_tests {
    use super::*;

    fn result(row: usize, status: &str) -> talentos_hr_runner::leave::BalanceImportRowResult {
        talentos_hr_runner::leave::BalanceImportRowResult {
            row,
            employee_code: format!("E{row}"),
            leave_type: "annual".into(),
            year: 2026,
            status: status.into(),
            errors: vec![],
        }
    }

    #[test]
    fn mixed_parse_and_service_rows_merge_without_panicking() {
        let parsed = vec![Some(result(1, "invalid")), None];
        let service = vec![result(2, "valid")];
        let merged = merge_balance_import_rows(parsed, &service);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[1].status, "valid");
    }
}

//! Employee loans / salary advances.
//!
//! HR creates a loan with a fixed installment; payroll deducts it each run
//! until the balance reaches zero. One active loan per employee.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;
use talentos_hr_runner::loans::{list_payments, payoff as record_payoff, PayoffError};

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub employee_id: Option<Uuid>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoanBody {
    pub employee_id: Uuid,
    pub principal: Decimal,
    pub installment_amount: Decimal,
    pub start_date: NaiveDate,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PayoffBody {
    pub amount: Decimal,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoanDto {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub employee_name: String,
    pub principal: String,
    pub balance: String,
    pub paid_amount: String,
    pub installment_amount: String,
    pub start_date: String,
    pub status: String,
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct PaymentDto {
    pub id: i64,
    pub loan_id: Uuid,
    pub amount: String,
    pub paid_at: String,
    pub note: Option<String>,
    pub created_by: Uuid,
}

/// GET /api/v1/loans?employee_id=&status=
pub async fn list(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<ListQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }

    let status = query.status.unwrap_or_else(|| "all".to_string());
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };

    let rows = sqlx::query(
        r#"SELECT l.id, l.employee_id, l.principal, l.balance, l.installment_amount,
                  l.start_date, l.status, l.note, l.created_at,
                  e.employee_code, e.first_name_th, e.last_name_th
             FROM employee_loans l
             JOIN employees e ON e.id = l.employee_id
            WHERE l.tenant_id = $1
              AND ($2 = 'all' OR l.status = $2)
              AND ($3::uuid IS NULL OR l.employee_id = $3)
            ORDER BY l.created_at DESC"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(&status)
    .bind(query.employee_id)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    match rows {
        Ok(rows) => {
            let loans: Vec<LoanDto> = rows.iter().map(row_to_dto).collect();
            (StatusCode::OK, Json(serde_json::json!({ "loans": loans }))).into_response()
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

/// POST /api/v1/loans
pub async fn create(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<LoanBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    if let Some(message) = validate(&body) {
        return bad_request(&message);
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };

    let employee_exists: bool = match sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM employees WHERE id = $1 AND tenant_id = $2)",
    )
    .bind(body.employee_id)
    .bind(ctx.tenant_id.0)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };
    if !employee_exists {
        let _ = tx.rollback().await;
        return bad_request("employee not found");
    }

    let id = Uuid::new_v4();
    let insert = sqlx::query(
        r#"INSERT INTO employee_loans
             (id, tenant_id, employee_id, principal, balance, installment_amount,
              start_date, status, note, created_by)
           VALUES ($1,$2,$3,$4,$4,$5,$6,'active',$7,$8)"#,
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .bind(body.employee_id)
    .bind(body.principal)
    .bind(body.installment_amount)
    .bind(body.start_date)
    .bind(body.note.as_deref())
    .bind(ctx.user_id.0)
    .execute(&mut *tx)
    .await;

    if let Err(e) = insert {
        let _ = tx.rollback().await;
        if is_unique_violation(&e) {
            return conflict("employee already has an active loan");
        }
        return internal_err(&e.to_string());
    }

    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    match fetch_one(&state, ctx.tenant_id.0, id).await {
        Ok(Some(loan)) => (StatusCode::CREATED, Json(loan)).into_response(),
        Ok(None) => internal_err("loan not found after insert"),
        Err(e) => internal_err(&e),
    }
}

/// POST /api/v1/loans/:id/cancel
pub async fn cancel(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };

    let status: Option<String> = match sqlx::query_scalar(
        "SELECT status FROM employee_loans WHERE id = $1 AND tenant_id = $2",
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };

    match status.as_deref() {
        None => {
            let _ = tx.rollback().await;
            return not_found();
        }
        Some("active") => {}
        Some(_) => {
            let _ = tx.rollback().await;
            return conflict("only active loans can be cancelled");
        }
    }

    let done = sqlx::query(
        "UPDATE employee_loans SET status = 'cancelled', updated_at = now() WHERE id = $1 AND tenant_id = $2"
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .execute(&mut *tx)
    .await;

    if let Err(e) = done {
        let _ = tx.rollback().await;
        return internal_err(&e.to_string());
    }

    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    match fetch_one(&state, ctx.tenant_id.0, id).await {
        Ok(Some(loan)) => (StatusCode::OK, Json(loan)).into_response(),
        Ok(None) => not_found(),
        Err(e) => internal_err(&e),
    }
}

/// POST /api/v1/loans/:id/payoff
pub async fn payoff(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<PayoffBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }

    match record_payoff(
        &state.pool,
        ctx.tenant_id,
        ctx.user_id.0,
        id,
        body.amount,
        body.note.as_deref(),
    )
    .await
    {
        Ok(()) => match fetch_one(&state, ctx.tenant_id.0, id).await {
            Ok(Some(loan)) => (StatusCode::OK, Json(loan)).into_response(),
            Ok(None) => not_found(),
            Err(e) => internal_err(&e),
        },
        Err(PayoffError::NotFound) => not_found(),
        Err(PayoffError::NotActive | PayoffError::InvalidAmount) => {
            conflict("amount must be greater than 0 and not exceed the remaining balance")
        }
        Err(PayoffError::Db(e)) => internal_err(&e.to_string()),
    }
}

/// GET /api/v1/loans/:id/payments
pub async fn payments(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }

    match list_payments(&state.pool, ctx.tenant_id, id).await {
        Ok(rows) => {
            let payments: Vec<PaymentDto> = rows
                .into_iter()
                .map(|p| PaymentDto {
                    id: p.id,
                    loan_id: p.loan_id,
                    amount: p.amount.to_string(),
                    paid_at: p.paid_at.to_rfc3339(),
                    note: p.note,
                    created_by: p.created_by,
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "payments": payments })),
            )
                .into_response()
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

/// GET /api/v1/me/loans
pub async fn me_loans(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "payslip", "view_self") {
        return forbidden;
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };

    let rows = sqlx::query(
        r#"SELECT l.id, l.employee_id, l.principal, l.balance, l.installment_amount,
                  l.start_date, l.status, l.note, l.created_at,
                  e.employee_code, e.first_name_th, e.last_name_th
             FROM employee_loans l
             JOIN employees e ON e.id = l.employee_id
            WHERE l.tenant_id = $1 AND e.user_id = $2
            ORDER BY l.created_at DESC"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    match rows {
        Ok(rows) => {
            let loans: Vec<LoanDto> = rows.iter().map(row_to_dto).collect();
            (StatusCode::OK, Json(serde_json::json!({ "loans": loans }))).into_response()
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

async fn fetch_one(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<Option<LoanDto>, String> {
    let mut tx = state
        .pool
        .begin_tenant(talentos_kernel::TenantId(tenant_id))
        .await
        .map_err(|e| e.to_string())?;
    let row = sqlx::query(
        r#"SELECT l.id, l.employee_id, l.principal, l.balance, l.installment_amount,
                  l.start_date, l.status, l.note, l.created_at,
                  e.employee_code, e.first_name_th, e.last_name_th
             FROM employee_loans l
             JOIN employees e ON e.id = l.employee_id
            WHERE l.id = $1 AND l.tenant_id = $2"#,
    )
    .bind(id)
    .bind(tenant_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    let _ = tx.rollback().await;
    Ok(row.as_ref().map(row_to_dto))
}

fn row_to_dto(row: &sqlx::postgres::PgRow) -> LoanDto {
    use sqlx::Row;
    let principal: Decimal = row.get("principal");
    let balance: Decimal = row.get("balance");
    let installment: Decimal = row.get("installment_amount");
    LoanDto {
        id: row.get("id"),
        employee_id: row.get("employee_id"),
        employee_code: row.get("employee_code"),
        employee_name: format!(
            "{} {}",
            row.get::<String, _>("first_name_th"),
            row.get::<String, _>("last_name_th")
        ),
        principal: principal.to_string(),
        balance: balance.to_string(),
        paid_amount: (principal - balance).to_string(),
        installment_amount: installment.to_string(),
        start_date: row.get::<NaiveDate, _>("start_date").to_string(),
        status: row.get("status"),
        note: row.get("note"),
        created_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
            .to_rfc3339(),
    }
}

fn validate(body: &LoanBody) -> Option<String> {
    if body.principal <= Decimal::ZERO {
        return Some("principal must be greater than 0".into());
    }
    if body.installment_amount <= Decimal::ZERO {
        return Some("installment_amount must be greater than 0".into());
    }
    if body.installment_amount > body.principal {
        return Some("installment_amount must not exceed principal".into());
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
    error_response(StatusCode::NOT_FOUND, "not_found", "loan not found")
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "loan endpoint failed");
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

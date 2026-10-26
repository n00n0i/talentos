//! Attendance records. Only OT feeds payroll today; hours_worked is recorded so
//! absence reporting can be added without a schema change.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

const OT_KINDS: &[&str] = &["weekday", "weekend", "holiday"];

#[derive(Debug, Deserialize)]
pub struct UpsertBody {
    pub employee_id: Uuid,
    pub work_date: NaiveDate,
    pub hours_worked: Option<Decimal>,
    pub ot_hours: Option<Decimal>,
    pub ot_kind: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub employee_id: Option<Uuid>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[derive(Debug, Serialize)]
pub struct AttendanceDto {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub work_date: String,
    pub hours_worked: String,
    pub ot_hours: String,
    pub ot_kind: String,
    pub note: Option<String>,
}

/// GET /api/v1/attendance?employee_id=&from=&to=
pub async fn list(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<ListQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "read") {
        return forbidden;
    }
    let today = chrono::Utc::now().date_naive();
    let from = query.from.unwrap_or_else(|| {
        NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap_or(today)
    });
    let to = query.to.unwrap_or(today);

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let rows = sqlx::query(
        r#"SELECT a.id, a.employee_id, e.employee_code, a.work_date, a.hours_worked,
                  a.ot_hours, a.ot_kind, a.note
             FROM attendance_records a
             JOIN employees e ON e.id = a.employee_id AND e.tenant_id = a.tenant_id
            WHERE a.tenant_id = $1
              AND ($2::uuid IS NULL OR a.employee_id = $2)
              AND a.work_date BETWEEN $3 AND $4
            ORDER BY a.work_date DESC, e.employee_code
            LIMIT 500"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(query.employee_id)
    .bind(from)
    .bind(to)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    match rows {
        Ok(rows) => {
            use sqlx::Row;
            let records: Vec<AttendanceDto> = rows
                .iter()
                .map(|row| AttendanceDto {
                    id: row.get("id"),
                    employee_id: row.get("employee_id"),
                    employee_code: row.get("employee_code"),
                    work_date: row.get::<NaiveDate, _>("work_date").to_string(),
                    hours_worked: row.get::<Decimal, _>("hours_worked").to_string(),
                    ot_hours: row.get::<Decimal, _>("ot_hours").to_string(),
                    ot_kind: row.get("ot_kind"),
                    note: row.get("note"),
                })
                .collect();
            (StatusCode::OK, Json(json!({ "records": records }))).into_response()
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

/// POST /api/v1/attendance — one record per employee per day (upsert).
pub async fn upsert(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<UpsertBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "manage") {
        return forbidden;
    }
    let hours = body.hours_worked.unwrap_or(Decimal::from(8));
    let ot = body.ot_hours.unwrap_or(Decimal::ZERO);
    let kind = body.ot_kind.as_deref().unwrap_or("weekday");
    if !OT_KINDS.contains(&kind) {
        return bad_request("ot_kind must be weekday, weekend or holiday");
    }
    if hours < Decimal::ZERO || hours > Decimal::from(24) {
        return bad_request("hours_worked must be between 0 and 24");
    }
    if ot < Decimal::ZERO || ot > Decimal::from(12) {
        return bad_request("ot_hours must be between 0 and 12");
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let employee = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM employees WHERE id = $1 AND tenant_id = $2 AND status <> 'terminated'",
    )
    .bind(body.employee_id)
    .bind(ctx.tenant_id.0)
    .fetch_optional(&mut *tx)
    .await;
    if !matches!(employee, Ok(Some(_))) {
        let _ = tx.rollback().await;
        return bad_request("employee not found or terminated");
    }

    let id = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO attendance_records
             (tenant_id, employee_id, work_date, hours_worked, ot_hours, ot_kind, note, created_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
           ON CONFLICT (employee_id, work_date) DO UPDATE SET
               hours_worked = EXCLUDED.hours_worked,
               ot_hours = EXCLUDED.ot_hours,
               ot_kind = EXCLUDED.ot_kind,
               note = EXCLUDED.note,
               updated_at = now()
           RETURNING id"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(body.employee_id)
    .bind(body.work_date)
    .bind(hours)
    .bind(ot)
    .bind(kind)
    .bind(body.note.as_deref())
    .bind(ctx.user_id.0)
    .fetch_one(&mut *tx)
    .await;

    let id = match id {
        Ok(id) => id,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    (
        StatusCode::OK,
        Json(json!({
            "id": id,
            "employee_id": body.employee_id,
            "work_date": body.work_date,
            "hours_worked": hours.to_string(),
            "ot_hours": ot.to_string(),
            "ot_kind": kind,
        })),
    )
        .into_response()
}

/// DELETE /api/v1/attendance/:id
pub async fn delete(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "manage") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let done = sqlx::query("DELETE FROM attendance_records WHERE id = $1 AND tenant_id = $2")
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
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }
    StatusCode::NO_CONTENT.into_response()
}

fn bad_request(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, message)
}

fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "attendance record not found")
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "attendance endpoint failed");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "An unexpected error occurred",
    )
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: "attendance_error".into(),
            message: message.into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

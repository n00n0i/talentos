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
    /// Omit to classify automatically: tenant holiday > weekend > weekday.
    pub ot_kind: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub employee_id: Option<Uuid>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
pub struct HolidayBody {
    pub holiday_date: NaiveDate,
    pub name_th: String,
}

#[derive(Debug, Deserialize)]
pub struct HolidayQuery {
    pub year: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct HolidayDto {
    pub id: Uuid,
    pub holiday_date: String,
    pub name_th: String,
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
    if let Some(kind) = body.ot_kind.as_deref() {
        if !OT_KINDS.contains(&kind) {
            return bad_request("ot_kind must be weekday, weekend or holiday");
        }
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

    let is_holiday: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM tenant_holidays WHERE tenant_id = $1 AND holiday_date = $2)",
    )
    .bind(ctx.tenant_id.0)
    .bind(body.work_date)
    .fetch_one(&mut *tx)
    .await
    .unwrap_or(false);
    let kind = body
        .ot_kind
        .clone()
        .unwrap_or_else(|| classify_day(body.work_date, is_holiday).to_string());

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
    .bind(&kind)
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

/// GET /api/v1/attendance/holidays?year=2026
pub async fn list_holidays(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<HolidayQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "read") {
        return forbidden;
    }
    let year = query.year.unwrap_or_else(|| chrono::Utc::now().year());
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let rows = sqlx::query(
        "SELECT id, holiday_date, name_th FROM tenant_holidays
          WHERE tenant_id = $1 AND EXTRACT(YEAR FROM holiday_date) = $2
          ORDER BY holiday_date",
    )
    .bind(ctx.tenant_id.0)
    .bind(year)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    match rows {
        Ok(rows) => {
            use sqlx::Row;
            let holidays: Vec<HolidayDto> = rows
                .iter()
                .map(|row| HolidayDto {
                    id: row.get("id"),
                    holiday_date: row.get::<NaiveDate, _>("holiday_date").to_string(),
                    name_th: row.get("name_th"),
                })
                .collect();
            (StatusCode::OK, Json(json!({ "holidays": holidays }))).into_response()
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

/// POST /api/v1/attendance/holidays
pub async fn upsert_holiday(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<HolidayBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "manage") {
        return forbidden;
    }
    if body.name_th.trim().is_empty() {
        return bad_request("name_th is required");
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO tenant_holidays (tenant_id, holiday_date, name_th, created_by)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (tenant_id, holiday_date) DO UPDATE SET name_th = EXCLUDED.name_th
           RETURNING id"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(body.holiday_date)
    .bind(body.name_th.trim())
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
        Json(
            json!({ "id": id, "holiday_date": body.holiday_date, "name_th": body.name_th.trim() }),
        ),
    )
        .into_response()
}

/// DELETE /api/v1/attendance/holidays/:id
pub async fn delete_holiday(
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
    let done = sqlx::query("DELETE FROM tenant_holidays WHERE id = $1 AND tenant_id = $2")
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

fn classify_day(date: NaiveDate, is_holiday: bool) -> &'static str {
    if is_holiday {
        return "holiday";
    }
    match date.weekday() {
        chrono::Weekday::Sat | chrono::Weekday::Sun => "weekend",
        _ => "weekday",
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("date")
    }

    #[test]
    fn holiday_wins_over_weekday_and_weekend() {
        assert_eq!(classify_day(date(2026, 10, 5), true), "holiday");
        assert_eq!(classify_day(date(2026, 10, 3), true), "holiday");
    }

    #[test]
    fn weekend_is_detected_without_a_calendar() {
        assert_eq!(classify_day(date(2026, 10, 3), false), "weekend");
        assert_eq!(classify_day(date(2026, 10, 4), false), "weekend");
        assert_eq!(classify_day(date(2026, 10, 5), false), "weekday");
    }
}

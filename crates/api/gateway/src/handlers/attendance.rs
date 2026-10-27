//! Attendance records. Only OT feeds payroll today; hours_worked is recorded so
//! absence reporting can be added without a schema change.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use talentos_hr_runner::attendance::{AttendanceError, ShiftInput};
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

#[derive(Debug, Deserialize)]
pub struct ShiftCreateBody {
    pub name: String,
    pub start: String,
    pub end: String,
    pub break_minutes: i32,
    pub grace_minutes: i32,
    /// Required when the caller's token is not entity-scoped.
    pub entity_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct ShiftAssignBody {
    pub employee_id: Uuid,
    pub effective_from: NaiveDate,
}

#[derive(Debug, Deserialize)]
pub struct GeofenceBody {
    pub lat: f64,
    pub lng: f64,
    pub radius_m: i32,
}

#[derive(Debug, Deserialize)]
pub struct DayListQuery {
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub employee_id: Option<Uuid>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CorrectBody {
    pub in_at: DateTime<Utc>,
    pub out_at: DateTime<Utc>,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportRowBody {
    pub employee_code: String,
    pub date: String,
    #[serde(rename = "in")]
    pub in_time: String,
    #[serde(rename = "out")]
    pub out_time: String,
}

#[derive(Debug, Deserialize)]
pub struct ImportBody {
    pub rows: Vec<ImportRowBody>,
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

/// GET /api/v1/shifts
pub async fn list_shifts(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "read") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let rows = sqlx::query(
        "SELECT id, name, start_time, end_time, break_minutes, grace_minutes
           FROM work_shifts
          WHERE tenant_id = $1
          ORDER BY name",
    )
    .bind(ctx.tenant_id.0)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    match rows {
        Ok(rows) => {
            use sqlx::Row;
            let shifts: Result<Vec<talentos_hr_runner::attendance::ShiftView>, sqlx::Error> = rows
                .into_iter()
                .map(|row| {
                    Ok(talentos_hr_runner::attendance::ShiftView {
                        id: row.try_get("id")?,
                        name: row.try_get("name")?,
                        start: row.try_get("start_time")?,
                        end: row.try_get("end_time")?,
                        break_minutes: row.try_get("break_minutes")?,
                        grace_minutes: row.try_get("grace_minutes")?,
                    })
                })
                .collect();
            match shifts {
                Ok(shifts) => (StatusCode::OK, Json(json!({ "shifts": shifts }))).into_response(),
                Err(e) => internal_err(&e.to_string()),
            }
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

/// POST /api/v1/shifts
pub async fn create_shift(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<ShiftCreateBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "shifts") {
        return forbidden;
    }
    let start = match parse_time(&body.start) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let end = match parse_time(&body.end) {
        Ok(t) => t,
        Err(resp) => return resp,
    };

    let entity_id = match resolve_entity_id(&ctx, body.entity_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let input = ShiftInput {
        name: body.name,
        start,
        end,
        break_minutes: body.break_minutes,
        grace_minutes: body.grace_minutes,
    };

    let shift = match talentos_hr_runner::attendance::create_shift(
        &state.pool,
        ctx.tenant_id,
        entity_id,
        input,
    )
    .await
    {
        Ok(shift) => shift,
        Err(e) => return map_attendance_error(e),
    };

    let audit = async {
        let mut tx = state.pool.begin_tenant(ctx.tenant_id).await.ok()?;
        sqlx::query(
            r#"INSERT INTO audit_log
                 (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
               VALUES ($1, 'user', $2, 'attendance.shift.create', 'work_shift', $3, $4)"#,
        )
        .bind(ctx.tenant_id.0)
        .bind(ctx.user_id.0.to_string())
        .bind(shift.id.to_string())
        .bind(json!({"name": &shift.name}))
        .execute(&mut *tx)
        .await
        .ok()?;
        tx.commit().await.ok()
    };
    let _ = audit.await;

    (StatusCode::CREATED, Json(shift)).into_response()
}

/// POST /api/v1/shifts/:id/assign
pub async fn assign_shift(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<ShiftAssignBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "shifts") {
        return forbidden;
    }
    if let Err(e) = talentos_hr_runner::attendance::assign_shift(
        &state.pool,
        ctx.tenant_id,
        body.employee_id,
        id,
        body.effective_from,
    )
    .await
    {
        return map_attendance_error(e);
    }

    let audit = async {
        let mut tx = state.pool.begin_tenant(ctx.tenant_id).await.ok()?;
        sqlx::query(
            r#"INSERT INTO audit_log
                 (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
               VALUES ($1, 'user', $2, 'attendance.shift.assign', 'shift_assignment', $3, $4)"#,
        )
        .bind(ctx.tenant_id.0)
        .bind(ctx.user_id.0.to_string())
        .bind(id.to_string())
        .bind(json!({"employee_id": body.employee_id, "effective_from": body.effective_from}))
        .execute(&mut *tx)
        .await
        .ok()?;
        tx.commit().await.ok()
    };
    let _ = audit.await;

    StatusCode::NO_CONTENT.into_response()
}

/// PUT /api/v1/settings/entities/:id/geofence
pub async fn set_geofence(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(entity_id): Path<Uuid>,
    Json(body): Json<GeofenceBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "shifts") {
        return forbidden;
    }
    if let Err(e) = talentos_hr_runner::attendance::set_geofence(
        &state.pool,
        ctx.tenant_id,
        entity_id,
        body.lat,
        body.lng,
        body.radius_m,
    )
    .await
    {
        return map_attendance_error(e);
    }

    let audit = async {
        let mut tx = state.pool.begin_tenant(ctx.tenant_id).await.ok()?;
        sqlx::query(
            r#"INSERT INTO audit_log
                 (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
               VALUES ($1, 'user', $2, 'attendance.geofence.update', 'entity', $3, $4)"#,
        )
        .bind(ctx.tenant_id.0)
        .bind(ctx.user_id.0.to_string())
        .bind(entity_id.to_string())
        .bind(json!({"lat": body.lat, "lng": body.lng, "radius_m": body.radius_m}))
        .execute(&mut *tx)
        .await
        .ok()?;
        tx.commit().await.ok()
    };
    let _ = audit.await;

    StatusCode::NO_CONTENT.into_response()
}

pub(crate) fn map_attendance_error(e: AttendanceError) -> Response {
    match e {
        AttendanceError::InvalidImport
        | AttendanceError::OutOfRange
        | AttendanceError::InvalidCorrection(_) => unprocessable(&e.to_string()),
        AttendanceError::NotFound
        | AttendanceError::NoShift
        | AttendanceError::NoGeofence
        | AttendanceError::EmployeeNotLinked => not_found_message(&e.to_string()),
        AttendanceError::OutsideGeofence { .. } => bad_request(&e.to_string()),
        AttendanceError::Rule(_) | AttendanceError::Db(_) => internal_err(&e.to_string()),
    }
}

#[allow(clippy::result_large_err)]
fn resolve_entity_id(
    ctx: &talentos_kernel::TenantContext,
    body_id: Option<Uuid>,
) -> Result<Uuid, Response> {
    if let Some(id) = body_id {
        return Ok(id);
    }
    if let Some(entity_id) = ctx.entity_id {
        return Ok(entity_id.0);
    }
    Err(unprocessable(
        "entity_id is required when token spans entities",
    ))
}

#[allow(clippy::result_large_err)]
fn parse_time(value: &str) -> Result<NaiveTime, Response> {
    NaiveTime::parse_from_str(value, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(value, "%H:%M:%S"))
        .map_err(|_| unprocessable("start/end must be HH:MM or HH:MM:SS"))
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

/// GET /api/v1/attendance/days?from=&to=&employee_id=&status=
pub async fn list_days(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<DayListQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "read") {
        return forbidden;
    }
    match talentos_hr_runner::attendance::list_days(
        &state.pool,
        ctx.tenant_id,
        query.from,
        query.to,
        query.employee_id,
        query.status.as_deref(),
    )
    .await
    {
        Ok(days) => (StatusCode::OK, Json(json!({ "days": days }))).into_response(),
        Err(e) => map_attendance_error(e),
    }
}

/// POST /api/v1/attendance/days/:id/approve
pub async fn approve_day(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "approve") {
        return forbidden;
    }
    match talentos_hr_runner::attendance::approve_day(&state.pool, ctx.tenant_id, ctx.user_id.0, id)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_attendance_error(e),
    }
}

/// POST /api/v1/attendance/days/:id/correct
pub async fn correct_day(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<CorrectBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "approve") {
        return forbidden;
    }
    let note = body.note.unwrap_or_default();
    match talentos_hr_runner::attendance::correct_day(
        &state.pool,
        ctx.tenant_id,
        ctx.user_id.0,
        id,
        body.in_at,
        body.out_at,
        &note,
    )
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_attendance_error(e),
    }
}

/// POST /api/v1/attendance/import
pub async fn import(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<ImportBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "manage") {
        return forbidden;
    }
    let rows: Vec<talentos_hr_runner::attendance::ImportRow> = body
        .rows
        .into_iter()
        .map(|r| talentos_hr_runner::attendance::ImportRow {
            employee_code: r.employee_code,
            date: r.date,
            in_time: r.in_time,
            out_time: r.out_time,
        })
        .collect();
    match talentos_hr_runner::attendance::import_device_rows(
        &state.pool,
        ctx.tenant_id,
        ctx.user_id.0,
        rows,
    )
    .await
    {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(e) => map_attendance_error(e),
    }
}

fn bad_request(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, message)
}

fn not_found() -> Response {
    not_found_message("attendance record not found")
}

fn not_found_message(message: &str) -> Response {
    error_response(StatusCode::NOT_FOUND, message)
}

fn unprocessable(message: &str) -> Response {
    error_response(StatusCode::UNPROCESSABLE_ENTITY, message)
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

    #[test]
    fn parses_hhmm_and_hhmmss_time_strings() {
        assert_eq!(
            parse_time("08:30").unwrap(),
            NaiveTime::from_hms_opt(8, 30, 0).unwrap()
        );
        assert_eq!(
            parse_time("08:30:45").unwrap(),
            NaiveTime::from_hms_opt(8, 30, 45).unwrap()
        );
    }

    #[test]
    fn rejects_invalid_time_strings() {
        assert!(parse_time("25:00").is_err());
        assert!(parse_time("08:30:99").is_err());
        assert!(parse_time("not-a-time").is_err());
    }
}

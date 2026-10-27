//! Attendance service: shifts, assignments, geofence settings, and mobile punch.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;
use serde_json::json;
use sqlx::Row;
use talentos_attendance::{
    bangkok_offset, derive_day, AttendanceRuleError, OtKind, Punch, PunchKind, Shift,
};
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

/// Input for creating a work shift.
#[derive(Debug, Clone)]
pub struct ShiftInput {
    pub name: String,
    pub start: NaiveTime,
    pub end: NaiveTime,
    pub break_minutes: i32,
    pub grace_minutes: i32,
}

/// Work shift as stored and returned by the service.
#[derive(Debug, Clone, Serialize)]
pub struct ShiftView {
    pub id: Uuid,
    pub name: String,
    pub start: NaiveTime,
    pub end: NaiveTime,
    pub break_minutes: i32,
    pub grace_minutes: i32,
}

/// Input for a mobile punch.
#[derive(Debug, Clone, Copy)]
pub struct PunchInput {
    pub kind: PunchKind,
    pub lat: f64,
    pub lng: f64,
    pub accuracy_m: Option<f64>,
}

/// Derived attendance state for a work date.
#[derive(Debug, Serialize)]
pub struct DayView {
    pub work_date: NaiveDate,
    pub worked_minutes: i32,
    pub late_minutes: i32,
    pub early_leave_minutes: i32,
    pub ot_minutes: i32,
    pub ot_kind: String,
    pub incomplete: bool,
}

/// Attendance service errors.
#[derive(Debug, thiserror::Error)]
pub enum AttendanceError {
    #[error("no geofence configured")]
    NoGeofence,
    #[error("outside geofence: {distance}m > {allowed}m")]
    OutsideGeofence { distance: f64, allowed: f64 },
    #[error("employee not linked to this tenant")]
    EmployeeNotLinked,
    #[error("no shift assigned")]
    NoShift,
    #[error("record not found")]
    NotFound,
    #[error("invalid import")]
    InvalidImport,
    #[error("attendance rule error: {0}")]
    Rule(#[from] AttendanceRuleError),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

impl ShiftInput {
    fn validate(&self) -> Result<(), AttendanceError> {
        if !(0..=240).contains(&self.break_minutes) {
            return Err(AttendanceError::InvalidImport);
        }
        if !(0..=60).contains(&self.grace_minutes) {
            return Err(AttendanceError::InvalidImport);
        }
        Ok(())
    }
}

/// Create a work shift under the given entity, verifying it belongs to the tenant.
pub async fn create_shift(
    pool: &TenantPool,
    tenant_id: TenantId,
    entity_id: Uuid,
    input: ShiftInput,
) -> Result<ShiftView, AttendanceError> {
    input.validate()?;

    let mut tx = pool.begin_tenant(tenant_id).await?;

    let entity_belongs: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM entities WHERE id = $1 AND tenant_id = $2)",
    )
    .bind(entity_id)
    .bind(tenant_id.0)
    .fetch_one(&mut *tx)
    .await?;
    if !entity_belongs {
        return Err(AttendanceError::EmployeeNotLinked);
    }

    let row = sqlx::query(
        r#"INSERT INTO work_shifts
             (tenant_id, entity_id, name, start_time, end_time, break_minutes, grace_minutes)
           VALUES ($1,$2,$3,$4,$5,$6,$7)
           RETURNING id, name, start_time, end_time, break_minutes, grace_minutes"#,
    )
    .bind(tenant_id.0)
    .bind(entity_id)
    .bind(&input.name)
    .bind(input.start)
    .bind(input.end)
    .bind(input.break_minutes)
    .bind(input.grace_minutes)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(map_shift(row)?)
}

/// Assign a shift to an employee, effective from `effective_from`.
/// The shift and employee must belong to the same entity.
pub async fn assign_shift(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
    shift_id: Uuid,
    effective_from: NaiveDate,
) -> Result<(), AttendanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;

    // Verify both employee and shift belong to this tenant and share an entity.
    let employee_entity: Option<Uuid> =
        sqlx::query_scalar("SELECT entity_id FROM employees WHERE id = $1 AND tenant_id = $2")
            .bind(employee_id)
            .bind(tenant_id.0)
            .fetch_optional(&mut *tx)
            .await?;
    let employee_entity = employee_entity.ok_or(AttendanceError::EmployeeNotLinked)?;

    let shift_entity: Option<Uuid> =
        sqlx::query_scalar("SELECT entity_id FROM work_shifts WHERE id = $1 AND tenant_id = $2")
            .bind(shift_id)
            .bind(tenant_id.0)
            .fetch_optional(&mut *tx)
            .await?;
    let shift_entity = shift_entity.ok_or(AttendanceError::NotFound)?;

    if employee_entity != shift_entity {
        return Err(AttendanceError::EmployeeNotLinked);
    }

    sqlx::query(
        r#"INSERT INTO shift_assignments
             (tenant_id, employee_id, shift_id, effective_from)
           VALUES ($1,$2,$3,$4)
           ON CONFLICT (employee_id, effective_from) DO UPDATE SET
               shift_id = EXCLUDED.shift_id,
               updated_at = now()"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(shift_id)
    .bind(effective_from)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Set the geofence for exactly one entity, verifying it belongs to the tenant.
pub async fn set_geofence(
    pool: &TenantPool,
    tenant_id: TenantId,
    entity_id: Uuid,
    lat: f64,
    lng: f64,
    radius_m: i32,
) -> Result<(), AttendanceError> {
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lng) {
        return Err(AttendanceError::InvalidImport);
    }
    if !(50..=5000).contains(&radius_m) {
        return Err(AttendanceError::InvalidImport);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let updated = sqlx::query(
        "UPDATE entities
            SET geofence_lat = $1, geofence_lng = $2, geofence_radius_m = $3, updated_at = now()
          WHERE id = $4 AND tenant_id = $5",
    )
    .bind(lat)
    .bind(lng)
    .bind(radius_m)
    .bind(entity_id)
    .bind(tenant_id.0)
    .execute(&mut *tx)
    .await?;

    if updated.rows_affected() == 0 {
        return Err(AttendanceError::NotFound);
    }

    tx.commit().await?;
    Ok(())
}

/// Return the active shift for an employee on a given work date.
/// Latest `effective_from` that is <= work_date wins.
pub async fn active_shift(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
    work_date: NaiveDate,
) -> Result<Option<ShiftView>, AttendanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let result = active_shift_in_tx(&mut tx, employee_id, work_date).await?;
    let _ = tx.rollback().await;
    Ok(result)
}

async fn active_shift_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    employee_id: Uuid,
    work_date: NaiveDate,
) -> Result<Option<ShiftView>, sqlx::Error> {
    let row = sqlx::query(
        r#"SELECT s.id, s.name, s.start_time, s.end_time, s.break_minutes, s.grace_minutes
             FROM shift_assignments a
             JOIN work_shifts s ON s.id = a.shift_id
            WHERE a.tenant_id = current_tenant_id()
              AND a.employee_id = $1
              AND a.effective_from <= $2
              AND (a.effective_until IS NULL OR a.effective_until >= $2)
            ORDER BY a.effective_from DESC
            LIMIT 1"#,
    )
    .bind(employee_id)
    .bind(work_date)
    .fetch_optional(&mut **tx)
    .await?;

    row.map(map_shift).transpose()
}

/// Record a mobile punch using the current server time.
pub async fn punch(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    input: PunchInput,
) -> Result<DayView, AttendanceError> {
    punch_at(pool, tenant_id, user_id, input, Utc::now()).await
}

/// Record a mobile punch at a fixed timestamp (public for tests).
pub async fn punch_at(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    input: PunchInput,
    occurred_at: DateTime<Utc>,
) -> Result<DayView, AttendanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;

    let employee = sqlx::query(
        "SELECT id, entity_id FROM employees
          WHERE tenant_id = $1 AND user_id = $2 AND status <> 'terminated'",
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let employee = employee.ok_or(AttendanceError::EmployeeNotLinked)?;
    let employee_id: Uuid = employee.try_get("id")?;
    let entity_id: Uuid = employee.try_get("entity_id")?;

    let (work_date, shift) = resolve_work_date(&mut tx, employee_id, occurred_at).await?;
    let shift = shift.ok_or(AttendanceError::NoShift)?;

    let (distance, allowed) = check_geofence(&mut tx, entity_id, &input).await?;
    if distance > allowed {
        return Err(AttendanceError::OutsideGeofence { distance, allowed });
    }

    let event_id = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO attendance_events
             (tenant_id, employee_id, kind, source, occurred_at, distance_m, accuracy_m)
           VALUES ($1,$2,$3,'mobile',$4,$5,$6)
           RETURNING id"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(kind_to_str(input.kind))
    .bind(occurred_at)
    .bind(distance)
    .bind(input.accuracy_m)
    .fetch_one(&mut *tx)
    .await?;

    let day = recompute_day(&mut tx, employee_id, &shift, work_date).await?;

    sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'attendance.punch', 'attendance_event', $3, $4)"#,
    )
    .bind(tenant_id.0)
    .bind(user_id.to_string())
    .bind(event_id.to_string())
    .bind(json!({
        "employee_id": employee_id,
        "work_date": work_date,
        "kind": kind_to_str(input.kind),
        "distance_m": distance,
        "accuracy_m": input.accuracy_m,
    }))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(day)
}

/// Resolve the work date for a punch, preferring an enclosing overnight window.
async fn resolve_work_date(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    employee_id: Uuid,
    occurred_at: DateTime<Utc>,
) -> Result<(NaiveDate, Option<ShiftView>), AttendanceError> {
    let bkk = bangkok_offset();
    let local = occurred_at.with_timezone(&bkk).naive_local();
    let today = local.date();
    let yesterday = today - Duration::days(1);

    let today_shift = active_shift_in_tx(tx, employee_id, today).await?;
    let yesterday_shift = active_shift_in_tx(tx, employee_id, yesterday).await?;

    if yesterday_shift
        .as_ref()
        .is_some_and(|s| shift_encloses(s, occurred_at, yesterday))
    {
        return Ok((yesterday, yesterday_shift));
    }
    if today_shift
        .as_ref()
        .is_some_and(|s| shift_encloses(s, occurred_at, today))
    {
        return Ok((today, today_shift));
    }

    Ok((today, today_shift))
}

/// Determine whether `at` falls inside `shift`'s window for `candidate_date`.
fn shift_encloses(shift: &ShiftView, at: DateTime<Utc>, candidate_date: NaiveDate) -> bool {
    let rules = Shift {
        start: shift.start,
        end: shift.end,
        break_minutes: shift.break_minutes,
        grace_minutes: shift.grace_minutes,
    };
    work_date_for(at, &rules) == candidate_date
}

/// Resolve a punch timestamp to the Bangkok work date it belongs to for a given shift.
pub fn work_date_for(at: DateTime<Utc>, shift: &Shift) -> NaiveDate {
    let bkk = bangkok_offset();
    let local = at.with_timezone(&bkk).naive_local();
    let today = local.date();
    let yesterday = today - Duration::days(1);

    let yest_start = yesterday.and_time(shift.start);
    let yest_end = if shift.end > shift.start {
        yesterday.and_time(shift.end)
    } else {
        today.and_time(shift.end)
    };
    if local >= yest_start && local <= yest_end {
        return yesterday;
    }

    let today_start = today.and_time(shift.start);
    let today_end = if shift.end > shift.start {
        today.and_time(shift.end)
    } else {
        (today + Duration::days(1)).and_time(shift.end)
    };
    if local >= today_start && local <= today_end {
        return today;
    }

    today
}

/// Check the punch against the entity geofence and return (distance, allowed).
async fn check_geofence(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    entity_id: Uuid,
    input: &PunchInput,
) -> Result<(f64, f64), AttendanceError> {
    let row = sqlx::query(
        "SELECT geofence_lat, geofence_lng, geofence_radius_m
           FROM entities WHERE id = $1 AND tenant_id = current_tenant_id()",
    )
    .bind(entity_id)
    .fetch_one(&mut **tx)
    .await?;

    let lat: Option<rust_decimal::Decimal> = row.try_get("geofence_lat")?;
    let lng: Option<rust_decimal::Decimal> = row.try_get("geofence_lng")?;
    let radius: i32 = row.try_get("geofence_radius_m")?;

    let (lat, lng) = match (lat, lng) {
        (Some(lat), Some(lng)) => (lat.to_f64().unwrap_or(0.0), lng.to_f64().unwrap_or(0.0)),
        _ => return Err(AttendanceError::NoGeofence),
    };

    let distance = haversine_m(input.lat, input.lng, lat, lng);
    let accuracy = input.accuracy_m.unwrap_or(0.0).min(50.0);
    let allowed = f64::from(radius) + accuracy;
    Ok((distance, allowed))
}

/// Haversine distance in metres between two WGS-84 points.
fn haversine_m(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlng = (lng2 - lng1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlng / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    R * c
}

/// Recompute and upsert the attendance day summary from stored punches.
async fn recompute_day(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    employee_id: Uuid,
    shift: &ShiftView,
    work_date: NaiveDate,
) -> Result<DayView, AttendanceError> {
    let bkk = bangkok_offset();
    let shift_start_bkk = bkk
        .from_local_datetime(&work_date.and_time(shift.start))
        .single()
        .ok_or_else(|| sqlx::Error::RowNotFound)?;
    let shift_end_bkk = if shift.end > shift.start {
        bkk.from_local_datetime(&work_date.and_time(shift.end))
    } else {
        bkk.from_local_datetime(&(work_date + Duration::days(1)).and_time(shift.end))
    }
    .single()
    .ok_or_else(|| sqlx::Error::RowNotFound)?;

    let rows = sqlx::query(
        "SELECT kind, occurred_at FROM attendance_events
          WHERE tenant_id = current_tenant_id()
            AND employee_id = $1
            AND occurred_at BETWEEN $2 AND $3
          ORDER BY occurred_at",
    )
    .bind(employee_id)
    .bind(shift_start_bkk.with_timezone(&Utc))
    .bind(shift_end_bkk.with_timezone(&Utc))
    .fetch_all(&mut **tx)
    .await?;

    let mut punches = Vec::with_capacity(rows.len());
    for row in rows {
        let kind_str: String = row.try_get("kind")?;
        let at: DateTime<Utc> = row.try_get("occurred_at")?;
        punches.push(Punch {
            kind: match kind_str.as_str() {
                "in" => PunchKind::In,
                "out" => PunchKind::Out,
                _ => continue,
            },
            at,
        });
    }

    let is_holiday: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM tenant_holidays
                        WHERE tenant_id = current_tenant_id() AND holiday_date = $1)",
    )
    .bind(work_date)
    .fetch_one(&mut **tx)
    .await?;

    let is_weekend = matches!(
        work_date.weekday(),
        chrono::Weekday::Sat | chrono::Weekday::Sun
    );

    let rules = Shift {
        start: shift.start,
        end: shift.end,
        break_minutes: shift.break_minutes,
        grace_minutes: shift.grace_minutes,
    };
    let summary = derive_day(&rules, &punches, work_date, is_holiday, is_weekend)?;

    let ot_kind_str = match summary.ot_kind {
        OtKind::Weekday => "weekday",
        OtKind::Weekend => "weekend",
        OtKind::Holiday => "holiday",
    };

    sqlx::query(
        r#"INSERT INTO attendance_days
             (tenant_id, employee_id, work_date, shift_id,
              worked_minutes, late_minutes, early_leave_minutes,
              ot_minutes, ot_kind, incomplete, status, computed_at)
           VALUES (current_tenant_id(),$1,$2,$3,$4,$5,$6,$7,$8,$9,'pending', now())
           ON CONFLICT (employee_id, work_date) DO UPDATE SET
               shift_id = EXCLUDED.shift_id,
               worked_minutes = EXCLUDED.worked_minutes,
               late_minutes = EXCLUDED.late_minutes,
               early_leave_minutes = EXCLUDED.early_leave_minutes,
               ot_minutes = EXCLUDED.ot_minutes,
               ot_kind = EXCLUDED.ot_kind,
               incomplete = EXCLUDED.incomplete,
               status = 'pending',
               computed_at = now(),
               updated_at = now()"#,
    )
    .bind(employee_id)
    .bind(work_date)
    .bind(shift.id)
    .bind(summary.worked_minutes)
    .bind(summary.late_minutes)
    .bind(summary.early_leave_minutes)
    .bind(summary.ot_minutes)
    .bind(ot_kind_str)
    .bind(summary.incomplete)
    .execute(&mut **tx)
    .await?;

    Ok(DayView {
        work_date,
        worked_minutes: summary.worked_minutes,
        late_minutes: summary.late_minutes,
        early_leave_minutes: summary.early_leave_minutes,
        ot_minutes: summary.ot_minutes,
        ot_kind: ot_kind_str.to_string(),
        incomplete: summary.incomplete,
    })
}

fn kind_to_str(kind: PunchKind) -> &'static str {
    match kind {
        PunchKind::In => "in",
        PunchKind::Out => "out",
    }
}

fn map_shift(row: sqlx::postgres::PgRow) -> Result<ShiftView, sqlx::Error> {
    Ok(ShiftView {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        start: row.try_get("start_time")?,
        end: row.try_get("end_time")?,
        break_minutes: row.try_get("break_minutes")?,
        grace_minutes: row.try_get("grace_minutes")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(break_minutes: i32, grace_minutes: i32) -> ShiftInput {
        ShiftInput {
            name: "test".into(),
            start: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
            break_minutes,
            grace_minutes,
        }
    }

    #[test]
    fn break_minutes_out_of_range_is_invalid() {
        assert!(matches!(
            input(-1, 0).validate(),
            Err(AttendanceError::InvalidImport)
        ));
        assert!(matches!(
            input(241, 0).validate(),
            Err(AttendanceError::InvalidImport)
        ));
    }

    #[test]
    fn grace_minutes_out_of_range_is_invalid() {
        assert!(matches!(
            input(0, -1).validate(),
            Err(AttendanceError::InvalidImport)
        ));
        assert!(matches!(
            input(0, 61).validate(),
            Err(AttendanceError::InvalidImport)
        ));
    }
}

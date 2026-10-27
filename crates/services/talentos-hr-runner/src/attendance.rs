//! Attendance service: shifts, assignments, and geofence settings.

use chrono::{NaiveDate, NaiveTime};
use serde::Serialize;
use sqlx::Row;
use talentos_attendance::AttendanceRuleError;
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
#[derive(Debug, Serialize)]
pub struct ShiftView {
    pub id: Uuid,
    pub name: String,
    pub start: NaiveTime,
    pub end: NaiveTime,
    pub break_minutes: i32,
    pub grace_minutes: i32,
}

/// Attendance service errors.
#[derive(Debug, thiserror::Error)]
pub enum AttendanceError {
    #[error("no geofence configured")]
    NoGeofence,
    #[error("outside geofence")]
    OutsideGeofence,
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

/// Create a work shift under the tenant's default entity.
pub async fn create_shift(
    pool: &TenantPool,
    tenant_id: TenantId,
    input: ShiftInput,
) -> Result<ShiftView, AttendanceError> {
    input.validate()?;

    let mut tx = pool.begin_tenant(tenant_id).await?;

    // Resolve the tenant's single/default entity; shifts are entity-scoped in
    // the schema, but the attendance API exposes one geofence per tenant today.
    let entity_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM entities WHERE tenant_id = $1 ORDER BY created_at LIMIT 1",
    )
    .bind(tenant_id.0)
    .fetch_one(&mut *tx)
    .await?;

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
pub async fn assign_shift(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
    shift_id: Uuid,
    effective_from: NaiveDate,
) -> Result<(), AttendanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;

    // Verify both employee and shift belong to this tenant (RLS filters, but an
    // explicit existence check gives a clear error instead of a constraint trap).
    let employee_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM employees WHERE id = $1 AND tenant_id = $2)",
    )
    .bind(employee_id)
    .bind(tenant_id.0)
    .fetch_one(&mut *tx)
    .await?;
    if !employee_exists {
        return Err(AttendanceError::EmployeeNotLinked);
    }

    let shift_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM work_shifts WHERE id = $1 AND tenant_id = $2)",
    )
    .bind(shift_id)
    .bind(tenant_id.0)
    .fetch_one(&mut *tx)
    .await?;
    if !shift_exists {
        return Err(AttendanceError::NotFound);
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

/// Set the tenant's geofence on all of its entities.
pub async fn set_geofence(
    pool: &TenantPool,
    tenant_id: TenantId,
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
    sqlx::query(
        "UPDATE entities
            SET geofence_lat = $1, geofence_lng = $2, geofence_radius_m = $3, updated_at = now()
          WHERE tenant_id = $4",
    )
    .bind(lat)
    .bind(lng)
    .bind(radius_m)
    .bind(tenant_id.0)
    .execute(&mut *tx)
    .await?;

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

    let row = sqlx::query(
        r#"SELECT s.id, s.name, s.start_time, s.end_time, s.break_minutes, s.grace_minutes
             FROM shift_assignments a
             JOIN work_shifts s ON s.id = a.shift_id
            WHERE a.tenant_id = $1
              AND a.employee_id = $2
              AND a.effective_from <= $3
              AND (a.effective_until IS NULL OR a.effective_until >= $3)
            ORDER BY a.effective_from DESC
            LIMIT 1"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(work_date)
    .fetch_optional(&mut *tx)
    .await?;

    let _ = tx.rollback().await;
    Ok(row.map(map_shift).transpose()?)
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

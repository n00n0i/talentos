//! Attendance day approval, correction, and payroll integration tests.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::attendance::{
    approve_day, assign_shift, correct_day, create_shift, list_days, punch_at, set_geofence,
    AttendanceError, PunchInput, ShiftInput,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_employee};

const OFFICE_LAT: f64 = 13.7563;
const OFFICE_LNG: f64 = 100.5018;

fn bkk_date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn bkk_time(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
    let bkk = chrono::FixedOffset::east_opt(7 * 60 * 60).unwrap();
    bkk.with_ymd_and_hms(y, mo, d, h, mi, 0)
        .unwrap()
        .with_timezone(&Utc)
}

async fn seed_shift(
    pool: &TenantPool,
    tenant_id: Uuid,
    entity_id: Uuid,
    employee_id: Uuid,
    start: NaiveTime,
    end: NaiveTime,
) -> Uuid {
    let shift = create_shift(
        pool,
        TenantId(tenant_id),
        entity_id,
        ShiftInput {
            name: "day".into(),
            start,
            end,
            break_minutes: 60,
            grace_minutes: 0,
        },
    )
    .await
    .expect("create shift");

    assign_shift(
        pool,
        TenantId(tenant_id),
        employee_id,
        shift.id,
        bkk_date(2026, 1, 1),
    )
    .await
    .expect("assign shift");

    shift.id
}

async fn day_id_of(
    pool: &PgPool,
    tenant_id: Uuid,
    employee_id: Uuid,
    work_date: NaiveDate,
) -> Option<Uuid> {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query(
        "SELECT id FROM attendance_days
          WHERE tenant_id = $1 AND employee_id = $2 AND work_date = $3",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .bind(work_date)
    .fetch_optional(&mut *tx)
    .await
    .expect("fetch day id");
    let _ = tx.rollback().await;
    row.map(|r| r.get::<Uuid, _>("id"))
}

async fn day_status(
    pool: &PgPool,
    tenant_id: Uuid,
    employee_id: Uuid,
    work_date: NaiveDate,
) -> Option<String> {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query(
        "SELECT status FROM attendance_days
          WHERE tenant_id = $1 AND employee_id = $2 AND work_date = $3",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .bind(work_date)
    .fetch_optional(&mut *tx)
    .await
    .expect("fetch status");
    let _ = tx.rollback().await;
    row.map(|r| r.get::<String, _>("status"))
}

async fn attendance_record(
    pool: &PgPool,
    tenant_id: Uuid,
    employee_id: Uuid,
    work_date: NaiveDate,
) -> Option<(Decimal, Decimal, String)> {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query(
        "SELECT hours_worked, ot_hours, ot_kind FROM attendance_records
          WHERE tenant_id = $1 AND employee_id = $2 AND work_date = $3",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .bind(work_date)
    .fetch_optional(&mut *tx)
    .await
    .expect("fetch record");
    let _ = tx.rollback().await;
    row.map(|r| {
        (
            r.get::<Decimal, _>("hours_worked"),
            r.get::<Decimal, _>("ot_hours"),
            r.get::<String, _>("ot_kind"),
        )
    })
}

async fn audit_count(pool: &PgPool, tenant_id: Uuid, action: &str, resource_id: Uuid) -> i64 {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query(
        "SELECT count(*)::int8 FROM audit_log
          WHERE tenant_id = $1 AND action = $2 AND resource_id = $3",
    )
    .bind(tenant_id)
    .bind(action)
    .bind(resource_id.to_string())
    .fetch_one(&mut *tx)
    .await
    .expect("count audit");
    let _ = tx.rollback().await;
    row.get::<i64, _>(0)
}

async fn user_id_of(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid) -> Uuid {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query("SELECT user_id FROM employees WHERE id = $1")
        .bind(employee_id)
        .fetch_one(&mut *tx)
        .await
        .expect("fetch user_id");
    let _ = tx.rollback().await;
    row.get::<Uuid, _>("user_id")
}

#[tokio::test]
async fn pending_day_is_not_paid() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    set_geofence(
        &pool,
        TenantId(tenant_id),
        entity_id,
        OFFICE_LAT,
        OFFICE_LNG,
        100,
    )
    .await
    .expect("set geofence");
    seed_shift(
        &pool,
        tenant_id,
        entity_id,
        employee_id,
        bkk_time(9, 0),
        bkk_time(18, 0),
    )
    .await;

    let input_in = PunchInput {
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    let input_out = PunchInput {
        kind: talentos_attendance::PunchKind::Out,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input_in,
        utc(2026, 10, 27, 9, 0),
    )
    .await
    .expect("punch in");
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input_out,
        utc(2026, 10, 27, 18, 0),
    )
    .await
    .expect("punch out");

    assert_eq!(
        day_status(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await,
        Some("pending".into())
    );
    assert!(
        attendance_record(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27))
            .await
            .is_none(),
        "pending day must not feed payroll"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn approve_writes_payroll_row_with_hours_and_ot_kind() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    set_geofence(
        &pool,
        TenantId(tenant_id),
        entity_id,
        OFFICE_LAT,
        OFFICE_LNG,
        100,
    )
    .await
    .expect("set geofence");
    seed_shift(
        &pool,
        tenant_id,
        entity_id,
        employee_id,
        bkk_time(9, 0),
        bkk_time(18, 0),
    )
    .await;

    let input_in = PunchInput {
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    let input_out = PunchInput {
        kind: talentos_attendance::PunchKind::Out,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input_in,
        utc(2026, 10, 27, 9, 0),
    )
    .await
    .expect("punch in");
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input_out,
        utc(2026, 10, 27, 18, 0),
    )
    .await
    .expect("punch out");

    let work_date = bkk_date(2026, 10, 27);
    let day_id = day_id_of(&raw, tenant_id, employee_id, work_date)
        .await
        .expect("day row");

    approve_day(&pool, TenantId(tenant_id), user_id, day_id)
        .await
        .expect("approve day");

    assert_eq!(
        day_status(&raw, tenant_id, employee_id, work_date).await,
        Some("approved".into())
    );

    let record = attendance_record(&raw, tenant_id, employee_id, work_date)
        .await
        .expect("payroll row after approval");
    // 09:00-18:00 with 60-minute break -> 8.00 hours, no OT on a weekday.
    assert_eq!(record.0, Decimal::from(8));
    assert_eq!(record.1, Decimal::ZERO);
    assert_eq!(record.2, "weekday");

    assert_eq!(
        audit_count(&raw, tenant_id, "attendance.approve", day_id).await,
        1,
        "approval must be audited"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn correction_resets_approved_day_and_payroll_row_is_unchanged_until_reapproval() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    set_geofence(
        &pool,
        TenantId(tenant_id),
        entity_id,
        OFFICE_LAT,
        OFFICE_LNG,
        100,
    )
    .await
    .expect("set geofence");
    seed_shift(
        &pool,
        tenant_id,
        entity_id,
        employee_id,
        bkk_time(9, 0),
        bkk_time(18, 0),
    )
    .await;

    let input_in = PunchInput {
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    let input_out = PunchInput {
        kind: talentos_attendance::PunchKind::Out,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input_in,
        utc(2026, 10, 27, 9, 0),
    )
    .await
    .expect("punch in");
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input_out,
        utc(2026, 10, 27, 18, 0),
    )
    .await
    .expect("punch out");

    let work_date = bkk_date(2026, 10, 27);
    let day_id = day_id_of(&raw, tenant_id, employee_id, work_date)
        .await
        .expect("day row");

    approve_day(&pool, TenantId(tenant_id), user_id, day_id)
        .await
        .expect("first approval");
    let first_record = attendance_record(&raw, tenant_id, employee_id, work_date)
        .await
        .expect("first payroll row");

    // Correct to a shorter day (09:00-12:00 Bangkok).
    correct_day(
        &pool,
        TenantId(tenant_id),
        user_id,
        day_id,
        utc(2026, 10, 27, 9, 0),
        utc(2026, 10, 27, 12, 0),
        "forgot to clock out",
    )
    .await
    .expect("correct day");

    assert_eq!(
        day_status(&raw, tenant_id, employee_id, work_date).await,
        Some("pending".into()),
        "correction must reset day to pending"
    );
    assert_eq!(
        attendance_record(&raw, tenant_id, employee_id, work_date).await,
        Some(first_record.clone()),
        "payroll row must stay unchanged until re-approval"
    );
    assert_eq!(
        audit_count(&raw, tenant_id, "attendance.correct", day_id).await,
        1,
        "correction must be audited"
    );

    // Re-approve updates the payroll row.
    approve_day(&pool, TenantId(tenant_id), user_id, day_id)
        .await
        .expect("re-approval");
    let second_record = attendance_record(&raw, tenant_id, employee_id, work_date)
        .await
        .expect("updated payroll row");
    assert_ne!(first_record, second_record);
    assert_eq!(second_record.0, Decimal::from(2)); // 120 minutes / 60 = 2.00

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn tenant_b_cannot_approve_tenant_a_day() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_a, entity_a, employee_a) = seed_tenant_employee(&raw).await;
    let user_a = user_id_of(&raw, tenant_a, employee_a).await;

    set_geofence(
        &pool,
        TenantId(tenant_a),
        entity_a,
        OFFICE_LAT,
        OFFICE_LNG,
        100,
    )
    .await
    .expect("set geofence");
    seed_shift(
        &pool,
        tenant_a,
        entity_a,
        employee_a,
        bkk_time(9, 0),
        bkk_time(18, 0),
    )
    .await;

    let input = PunchInput {
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    punch_at(
        &pool,
        TenantId(tenant_a),
        user_a,
        input,
        utc(2026, 10, 27, 9, 0),
    )
    .await
    .expect("punch");

    let day_id = day_id_of(&raw, tenant_a, employee_a, bkk_date(2026, 10, 27))
        .await
        .expect("day row");

    let (tenant_b, _entity_b, employee_b) = seed_tenant_employee(&raw).await;
    let user_b = user_id_of(&raw, tenant_b, employee_b).await;

    let result = approve_day(&pool, TenantId(tenant_b), user_b, day_id).await;
    assert!(
        matches!(result, Err(AttendanceError::NotFound)),
        "tenant B must not see tenant A's day, got {result:?}"
    );

    cleanup(&raw, tenant_a).await;
    cleanup(&raw, tenant_b).await;
}

#[tokio::test]
async fn list_days_filters_by_date_range_employee_and_status() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    set_geofence(
        &pool,
        TenantId(tenant_id),
        entity_id,
        OFFICE_LAT,
        OFFICE_LNG,
        100,
    )
    .await
    .expect("set geofence");
    seed_shift(
        &pool,
        tenant_id,
        entity_id,
        employee_id,
        bkk_time(9, 0),
        bkk_time(18, 0),
    )
    .await;

    let input = PunchInput {
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input,
        utc(2026, 10, 27, 9, 0),
    )
    .await
    .expect("punch");

    let all = list_days(
        &pool,
        TenantId(tenant_id),
        bkk_date(2026, 10, 1),
        bkk_date(2026, 10, 31),
        None,
        None,
    )
    .await
    .expect("list days");
    assert_eq!(all.len(), 1);

    let filtered = list_days(
        &pool,
        TenantId(tenant_id),
        bkk_date(2026, 10, 1),
        bkk_date(2026, 10, 31),
        Some(employee_id),
        Some("approved"),
    )
    .await
    .expect("list approved");
    assert!(filtered.is_empty());

    cleanup(&raw, tenant_id).await;
}

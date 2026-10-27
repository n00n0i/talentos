//! Attendance mobile punch + geofence + day recompute tests.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, TimeZone, Utc};
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::attendance::{
    assign_shift, create_shift, punch_at, set_geofence, AttendanceError, PunchInput, ShiftInput,
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

/// Convert a Bangkok wall-clock time to UTC.
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

async fn seed_holiday(pool: &PgPool, tenant_id: Uuid, date: NaiveDate) {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "INSERT INTO tenant_holidays (tenant_id, holiday_date, name_th, created_by)
         VALUES ($1, $2, 'Holiday', NULL)
         ON CONFLICT (tenant_id, holiday_date) DO NOTHING",
    )
    .bind(tenant_id)
    .bind(date)
    .execute(&mut *tx)
    .await
    .expect("insert holiday");
    tx.commit().await.expect("commit holiday");
}

async fn approve_day(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid, date: NaiveDate) {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "UPDATE attendance_days SET status = 'approved', approved_at = now()
          WHERE tenant_id = $1 AND employee_id = $2 AND work_date = $3",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .bind(date)
    .execute(&mut *tx)
    .await
    .expect("approve day");
    tx.commit().await.expect("commit approve");
}

async fn day_status(
    pool: &PgPool,
    tenant_id: Uuid,
    employee_id: Uuid,
    date: NaiveDate,
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
    .bind(date)
    .fetch_optional(&mut *tx)
    .await
    .expect("fetch status");
    let _ = tx.rollback().await;
    row.map(|r| r.get::<String, _>("status"))
}

#[tokio::test]
async fn punch_inside_geofence_is_accepted() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    set_geofence(&pool, TenantId(tenant_id), entity_id, OFFICE_LAT, OFFICE_LNG, 100)
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
    let day = punch_at(&pool, TenantId(tenant_id), user_id, input, utc(2026, 10, 27, 2, 0))
        .await
        .expect("inside punch should succeed");
    assert_eq!(day.work_date, bkk_date(2026, 10, 27));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn punch_outside_geofence_is_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    set_geofence(&pool, TenantId(tenant_id), entity_id, OFFICE_LAT, OFFICE_LNG, 100)
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
        lat: OFFICE_LAT + 0.01,
        lng: OFFICE_LNG + 0.01,
        accuracy_m: Some(10.0),
    };
    let result = punch_at(&pool, TenantId(tenant_id), user_id, input, utc(2026, 10, 27, 2, 0)).await;
    assert!(
        matches!(result, Err(AttendanceError::OutsideGeofence { .. })),
        "expected OutsideGeofence, got {result:?}"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn punch_within_accuracy_tolerance_is_accepted() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    // radius 100, accuracy 40, distance 130 -> allowed = 140.
    set_geofence(&pool, TenantId(tenant_id), entity_id, OFFICE_LAT, OFFICE_LNG, 100)
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

    let distance_m = 130.0;
    // Approximate offset: 1 degree lat ~ 111 km; 130 m ~ 0.00117 degrees.
    let delta = distance_m / 111_000.0;
    let input = PunchInput {
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT + delta,
        lng: OFFICE_LNG,
        accuracy_m: Some(40.0),
    };
    let result = punch_at(&pool, TenantId(tenant_id), user_id, input, utc(2026, 10, 27, 2, 0)).await;
    assert!(result.is_ok(), "expected acceptance, got {result:?}");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn punch_without_geofence_is_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    // No geofence configured.
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
    let result = punch_at(&pool, TenantId(tenant_id), user_id, input, utc(2026, 10, 27, 2, 0)).await;
    assert!(
        matches!(result, Err(AttendanceError::NoGeofence)),
        "expected NoGeofence, got {result:?}"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn unlinked_user_is_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;

    set_geofence(&pool, TenantId(tenant_id), entity_id, OFFICE_LAT, OFFICE_LNG, 100)
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

    let unlinked_user = Uuid::new_v4();
    let input = PunchInput {
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    let result = punch_at(&pool, TenantId(tenant_id), unlinked_user, input, utc(2026, 10, 27, 2, 0))
        .await;
    assert!(
        matches!(result, Err(AttendanceError::EmployeeNotLinked)),
        "expected EmployeeNotLinked, got {result:?}"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn overnight_punch_lands_on_previous_work_date() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    set_geofence(&pool, TenantId(tenant_id), entity_id, OFFICE_LAT, OFFICE_LNG, 100)
        .await
        .expect("set geofence");
    seed_shift(
        &pool,
        tenant_id,
        entity_id,
        employee_id,
        bkk_time(22, 0),
        bkk_time(6, 0),
    )
    .await;

    // 01:00 Bangkok on 2026-10-28 is 18:00 UTC on 2026-10-27.
    let input = PunchInput {
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    let day = punch_at(&pool, TenantId(tenant_id), user_id, input, utc(2026, 10, 27, 18, 0))
        .await
        .expect("overnight punch should succeed");
    assert_eq!(day.work_date, bkk_date(2026, 10, 27));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn holiday_punch_marks_day_holiday() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    set_geofence(&pool, TenantId(tenant_id), entity_id, OFFICE_LAT, OFFICE_LNG, 100)
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
    seed_holiday(&raw, tenant_id, bkk_date(2026, 10, 27)).await;

    let input = PunchInput {
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    let day = punch_at(&pool, TenantId(tenant_id), user_id, input, utc(2026, 10, 27, 2, 0))
        .await
        .expect("holiday punch should succeed");
    assert_eq!(day.work_date, bkk_date(2026, 10, 27));
    assert_eq!(day.ot_kind, "holiday");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn two_punches_same_second_stay_consistent() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    set_geofence(&pool, TenantId(tenant_id), entity_id, OFFICE_LAT, OFFICE_LNG, 100)
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

    let same_time = utc(2026, 10, 27, 2, 0);
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

    let first = punch_at(&pool, TenantId(tenant_id), user_id, input_in, same_time)
        .await
        .expect("first same-second punch");
    let second = punch_at(&pool, TenantId(tenant_id), user_id, input_out, same_time)
        .await
        .expect("second same-second punch");

    // Both punches happened at the same instant; worked minutes should be 0
    // (in and out at the same time) and the day must stay consistent.
    assert_eq!(first.worked_minutes, 0);
    assert_eq!(second.worked_minutes, 0);
    assert_eq!(second.ot_kind, "weekday");

    let count = event_count(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await;
    assert_eq!(count, 2, "two distinct events should exist");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn negative_accuracy_is_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    set_geofence(&pool, TenantId(tenant_id), entity_id, OFFICE_LAT, OFFICE_LNG, 100)
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
        accuracy_m: Some(-10.0),
    };
    let result = punch_at(&pool, TenantId(tenant_id), user_id, input, utc(2026, 10, 27, 2, 0)).await;
    assert!(
        matches!(result, Err(AttendanceError::InvalidImport)),
        "expected InvalidImport for negative accuracy, got {result:?}"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn new_punch_resets_approved_day_to_pending() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    set_geofence(&pool, TenantId(tenant_id), entity_id, OFFICE_LAT, OFFICE_LNG, 100)
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

    punch_at(&pool, TenantId(tenant_id), user_id, input_in, utc(2026, 10, 27, 2, 0))
        .await
        .expect("first punch");
    punch_at(&pool, TenantId(tenant_id), user_id, input_out, utc(2026, 10, 27, 11, 0))
        .await
        .expect("second punch");

    approve_day(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await;
    assert_eq!(
        day_status(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await,
        Some("approved".into())
    );

    // A new punch after approval resets the day to pending.
    punch_at(&pool, TenantId(tenant_id), user_id, input_out, utc(2026, 10, 27, 11, 1))
        .await
        .expect("post-approval punch");
    assert_eq!(
        day_status(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await,
        Some("pending".into())
    );

    cleanup(&raw, tenant_id).await;
}

async fn event_count(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid, date: NaiveDate) -> i64 {
    let bkk = chrono::FixedOffset::east_opt(7 * 60 * 60).unwrap();
    let start = bkk
        .with_ymd_and_hms(date.year(), date.month(), date.day(), 0, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    let end = bkk
        .with_ymd_and_hms(date.year(), date.month(), date.day(), 23, 59, 59)
        .unwrap()
        .with_timezone(&Utc);

    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query(
        "SELECT count(*)::int8 FROM attendance_events
          WHERE tenant_id = $1 AND employee_id = $2 AND occurred_at BETWEEN $3 AND $4",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .bind(start)
    .bind(end)
    .fetch_one(&mut *tx)
    .await
    .expect("count events");
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

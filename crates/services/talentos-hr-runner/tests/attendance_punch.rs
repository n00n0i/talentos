//! Attendance mobile punch + geofence + day recompute tests.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, TimeZone, Utc};
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::attendance::{
    assign_shift, create_shift, punch, punch_at, set_geofence, AttendanceError, PunchInput,
    ShiftInput,
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

/// Convert a Bangkok wall-clock time to the UTC instant.
fn bkk_at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
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
        client_time: None,
        client_id: None,
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    let day = punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input,
        bkk_at(2026, 10, 27, 9, 0),
    )
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
        client_time: None,
        client_id: None,
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT + 0.01,
        lng: OFFICE_LNG + 0.01,
        accuracy_m: Some(10.0),
    };
    let result = punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input,
        bkk_at(2026, 10, 27, 9, 0),
    )
    .await;
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

    let distance_m = 130.0;
    // Approximate offset: 1 degree lat ~ 111 km; 130 m ~ 0.00117 degrees.
    let delta = distance_m / 111_000.0;
    let input = PunchInput {
        client_time: None,
        client_id: None,
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT + delta,
        lng: OFFICE_LNG,
        accuracy_m: Some(40.0),
    };
    let result = punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input,
        bkk_at(2026, 10, 27, 9, 0),
    )
    .await;
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
        client_time: None,
        client_id: None,
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    let result = punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input,
        bkk_at(2026, 10, 27, 9, 0),
    )
    .await;
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

    let unlinked_user = Uuid::new_v4();
    let input = PunchInput {
        client_time: None,
        client_id: None,
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    let result = punch_at(
        &pool,
        TenantId(tenant_id),
        unlinked_user,
        input,
        bkk_at(2026, 10, 27, 9, 0),
    )
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
        bkk_time(22, 0),
        bkk_time(6, 0),
    )
    .await;

    // 01:00 Bangkok on 2026-10-28 falls inside the 27th's 22:00-06:00 shift.
    let input = PunchInput {
        client_time: None,
        client_id: None,
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    let day = punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input,
        bkk_at(2026, 10, 28, 1, 0),
    )
    .await
    .expect("overnight punch should succeed");
    assert_eq!(day.work_date, bkk_date(2026, 10, 27));

    cleanup(&raw, tenant_id).await;
}

async fn punch_pair(
    pool: &TenantPool,
    tenant_id: Uuid,
    user_id: Uuid,
    in_at: DateTime<Utc>,
    out_at: DateTime<Utc>,
) -> talentos_hr_runner::attendance::DayView {
    let mk = |kind| PunchInput {
        client_time: None,
        client_id: None,
        kind,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    punch_at(
        pool,
        TenantId(tenant_id),
        user_id,
        mk(talentos_attendance::PunchKind::In),
        in_at,
    )
    .await
    .expect("punch in");
    punch_at(
        pool,
        TenantId(tenant_id),
        user_id,
        mk(talentos_attendance::PunchKind::Out),
        out_at,
    )
    .await
    .expect("punch out")
}

#[tokio::test]
async fn early_arrival_and_late_leave_count_with_ot_after_shift_end() {
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

    let day = punch_pair(
        &pool,
        tenant_id,
        user_id,
        bkk_at(2026, 10, 27, 8, 50),
        bkk_at(2026, 10, 27, 19, 30),
    )
    .await;
    assert_eq!(day.work_date, bkk_date(2026, 10, 27));
    assert_eq!(day.worked_minutes, 580);
    assert_eq!(day.ot_minutes, 90);
    assert_eq!(day.ot_kind, "weekday");
    assert!(!day.incomplete);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn overnight_shift_out_after_shift_end_counts_ot_on_start_date() {
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
        bkk_time(22, 0),
        bkk_time(6, 0),
    )
    .await;

    // 22:00 Bangkok on the 27th is 15:00 UTC; 06:05 on the 28th is 23:05 UTC the 27th.
    let day = punch_pair(
        &pool,
        tenant_id,
        user_id,
        bkk_at(2026, 10, 27, 22, 0),
        bkk_at(2026, 10, 28, 6, 5),
    )
    .await;
    assert_eq!(day.work_date, bkk_date(2026, 10, 27));
    assert_eq!(day.worked_minutes, 425);
    assert_eq!(day.ot_minutes, 5);
    assert!(!day.incomplete);

    let orphan = day_status(&raw, tenant_id, employee_id, bkk_date(2026, 10, 28)).await;
    assert_eq!(orphan, None, "out punch must not open a day on the 28th");

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
    seed_holiday(&raw, tenant_id, bkk_date(2026, 10, 27)).await;

    let input = PunchInput {
        client_time: None,
        client_id: None,
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    let day = punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input,
        bkk_at(2026, 10, 27, 9, 0),
    )
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

    let same_time = bkk_at(2026, 10, 27, 9, 0);
    let input_in = PunchInput {
        client_time: None,
        client_id: None,
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    let input_out = PunchInput {
        client_time: None,
        client_id: None,
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
        client_time: None,
        client_id: None,
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(-10.0),
    };
    let result = punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input,
        bkk_at(2026, 10, 27, 9, 0),
    )
    .await;
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
        client_time: None,
        client_id: None,
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    let input_out = PunchInput {
        client_time: None,
        client_id: None,
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
        bkk_at(2026, 10, 27, 9, 0),
    )
    .await
    .expect("first punch");
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input_out.clone(),
        bkk_at(2026, 10, 27, 18, 0),
    )
    .await
    .expect("second punch");

    approve_day(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await;
    assert_eq!(
        day_status(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await,
        Some("approved".into())
    );

    // A new punch after approval resets the day to pending.
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        input_out,
        bkk_at(2026, 10, 27, 18, 1),
    )
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

async fn audit_count(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid) -> i64 {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let count = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log
          WHERE tenant_id = $1 AND action = 'attendance.punch'
            AND after_state->>'employee_id' = $2",
    )
    .bind(tenant_id)
    .bind(employee_id.to_string())
    .fetch_one(&mut *tx)
    .await
    .expect("audit count");
    let _ = tx.rollback().await;
    count
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
async fn client_id_replay_is_idempotent() {
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

    let client = Uuid::new_v4().to_string();
    let punch_in = |client_id: Option<String>| PunchInput {
        client_time: None,
        client_id,
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };

    // A replayed punch with the same client id lands on the existing event.
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        punch_in(Some(client.clone())),
        bkk_at(2026, 10, 27, 9, 0),
    )
    .await
    .expect("first punch");
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        punch_in(Some(client)),
        bkk_at(2026, 10, 27, 9, 0),
    )
    .await
    .expect("replayed punch");
    assert_eq!(
        event_count(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await,
        1,
        "same client id must not duplicate the event"
    );
    assert_eq!(
        audit_count(&raw, tenant_id, employee_id).await,
        1,
        "a replay must not write a second audit row"
    );

    // A distinct client id creates a new event.
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        punch_in(Some(Uuid::new_v4().to_string())),
        bkk_at(2026, 10, 27, 9, 5),
    )
    .await
    .expect("distinct punch");
    assert_eq!(
        event_count(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await,
        2,
        "a distinct client id must create a second event"
    );
    assert_eq!(
        audit_count(&raw, tenant_id, employee_id).await,
        2,
        "each real punch writes one audit row"
    );

    cleanup(&raw, tenant_id).await;
}

/// A punch queued offline must keep its original instant, not the flush time.
#[tokio::test]
async fn client_time_preserves_offline_punch_instant() {
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

    let client_id = Uuid::new_v4().to_string();
    let occurred_at = Utc::now() - chrono::Duration::hours(2);
    let input = |client_time: Option<DateTime<Utc>>| PunchInput {
        client_time,
        client_id: Some(client_id.clone()),
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(5.0),
    };

    punch(
        &pool,
        TenantId(tenant_id),
        user_id,
        input(Some(occurred_at)),
    )
    .await
    .expect("offline punch");

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let stored: DateTime<Utc> =
        sqlx::query_scalar("SELECT occurred_at FROM attendance_events WHERE client_id = $1")
            .bind(&client_id)
            .fetch_one(&mut *tx)
            .await
            .expect("occurred_at");
    let _ = tx.rollback().await;
    assert_eq!(
        stored.timestamp_micros(),
        occurred_at.timestamp_micros(),
        "queued punch must keep its original time"
    );

    let too_old = punch(
        &pool,
        TenantId(tenant_id),
        user_id,
        input(Some(Utc::now() - chrono::Duration::days(8))),
    )
    .await
    .expect_err("8-day-old client_time must be rejected");
    assert!(matches!(too_old, AttendanceError::InvalidClientTime));

    cleanup(&raw, tenant_id).await;
}

/// Replaying a punch whose response was lost must not reopen an approved day.
#[tokio::test]
async fn replay_after_approval_keeps_day_approved() {
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

    let client = Uuid::new_v4().to_string();
    let punch_in = |client_id: Option<String>| PunchInput {
        client_time: None,
        client_id,
        kind: talentos_attendance::PunchKind::In,
        lat: OFFICE_LAT,
        lng: OFFICE_LNG,
        accuracy_m: Some(10.0),
    };
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        punch_in(Some(client.clone())),
        bkk_at(2026, 10, 27, 9, 0),
    )
    .await
    .expect("in punch");
    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        PunchInput {
            client_time: None,
            client_id: Some(Uuid::new_v4().to_string()),
            kind: talentos_attendance::PunchKind::Out,
            lat: OFFICE_LAT,
            lng: OFFICE_LNG,
            accuracy_m: Some(10.0),
        },
        bkk_at(2026, 10, 27, 18, 0),
    )
    .await
    .expect("out punch");

    approve_day(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await;

    punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        punch_in(Some(client)),
        bkk_at(2026, 10, 27, 9, 0),
    )
    .await
    .expect("replayed punch");

    assert_eq!(
        day_status(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await,
        Some("approved".into()),
        "a replay must not turn an approved day back into pending"
    );
    assert_eq!(
        audit_count(&raw, tenant_id, employee_id).await,
        2,
        "the replay must not audit again"
    );

    cleanup(&raw, tenant_id).await;
}

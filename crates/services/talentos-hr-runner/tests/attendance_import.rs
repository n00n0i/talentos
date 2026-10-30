//! Attendance device CSV/JSON import tests.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, TimeZone, Utc};
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::attendance::{
    assign_shift, create_shift, import_device_rows, list_days, set_geofence, ImportRow, PunchInput,
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

/// Bangkok wall-clock time to the real UTC instant.
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
) -> Uuid {
    let shift = create_shift(
        pool,
        TenantId(tenant_id),
        entity_id,
        ShiftInput {
            name: "day".into(),
            start: bkk_time(9, 0),
            end: bkk_time(18, 0),
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

#[tokio::test]
async fn two_valid_rows_and_unknown_employee() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    seed_shift(&pool, tenant_id, entity_id, employee_id).await;

    let rows = vec![
        ImportRow {
            employee_code: "EMP000".into(),
            date: "2026-10-27".into(),
            in_time: "09:00".into(),
            out_time: "18:00".into(),
        },
        ImportRow {
            employee_code: "EMP000".into(),
            date: "2026-10-28".into(),
            in_time: "09:00".into(),
            out_time: "18:00".into(),
        },
        ImportRow {
            employee_code: "UNKNOWN".into(),
            date: "2026-10-27".into(),
            in_time: "09:00".into(),
            out_time: "18:00".into(),
        },
    ];

    let result = import_device_rows(&pool, TenantId(tenant_id), Uuid::new_v4(), rows)
        .await
        .expect("batch should succeed");

    assert_eq!(result.imported, 2);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].row, 3);
    assert!(
        result.errors[0].message.contains("employee"),
        "error should mention employee: {}",
        result.errors[0].message
    );

    assert_eq!(
        event_count(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await,
        2
    );
    assert_eq!(
        event_count(&raw, tenant_id, employee_id, bkk_date(2026, 10, 28)).await,
        2
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn reimport_does_not_duplicate_events() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    seed_shift(&pool, tenant_id, entity_id, employee_id).await;

    let rows = vec![ImportRow {
        employee_code: "EMP000".into(),
        date: "2026-10-27".into(),
        in_time: "09:00".into(),
        out_time: "18:00".into(),
    }];

    let first = import_device_rows(&pool, TenantId(tenant_id), Uuid::new_v4(), rows.clone())
        .await
        .expect("first import");
    assert_eq!(first.imported, 1);

    let second = import_device_rows(&pool, TenantId(tenant_id), Uuid::new_v4(), rows)
        .await
        .expect("second import");
    assert_eq!(second.imported, 1);

    assert_eq!(
        event_count(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await,
        2,
        "re-import must replace, not duplicate"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn overnight_row_puts_out_on_next_day_and_keeps_start_work_date() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let shift = create_shift(
        &pool,
        TenantId(tenant_id),
        entity_id,
        ShiftInput {
            name: "night".into(),
            start: bkk_time(22, 0),
            end: bkk_time(6, 0),
            break_minutes: 60,
            grace_minutes: 0,
        },
    )
    .await
    .expect("create shift");
    assign_shift(
        &pool,
        TenantId(tenant_id),
        employee_id,
        shift.id,
        bkk_date(2026, 1, 1),
    )
    .await
    .expect("assign shift");

    let result = import_device_rows(
        &pool,
        TenantId(tenant_id),
        Uuid::new_v4(),
        vec![
            ImportRow {
                employee_code: "EMP000".into(),
                date: "2026-10-27".into(),
                in_time: "22:00".into(),
                out_time: "06:05".into(),
            },
            // The same instants can be re-imported without duplicates.
            ImportRow {
                employee_code: "EMP000".into(),
                date: "2026-10-27".into(),
                in_time: "22:00".into(),
                out_time: "06:05".into(),
            },
        ],
    )
    .await
    .expect("import");
    assert_eq!(result.imported, 2, "errors: {:?}", result.errors);

    let days = list_days(
        &pool,
        TenantId(tenant_id),
        bkk_date(2026, 10, 26),
        bkk_date(2026, 10, 29),
        None,
        None,
    )
    .await
    .expect("list days");
    assert_eq!(days.len(), 1, "no orphan day on the 28th: {days:?}");
    assert_eq!(days[0].work_date, bkk_date(2026, 10, 27));
    assert_eq!(days[0].worked_minutes, 425);
    assert_eq!(days[0].ot_minutes, 5);
    assert!(!days[0].incomplete);
    assert_eq!(
        event_count(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await
            + event_count(&raw, tenant_id, employee_id, bkk_date(2026, 10, 28)).await,
        2
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn mobile_punched_day_is_skipped() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    seed_shift(&pool, tenant_id, entity_id, employee_id).await;

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

    // Mobile punch first.
    talentos_hr_runner::attendance::punch_at(
        &pool,
        TenantId(tenant_id),
        user_id,
        PunchInput {
            client_time: None,
            client_id: None,
            kind: talentos_attendance::PunchKind::In,
            lat: OFFICE_LAT,
            lng: OFFICE_LNG,
            accuracy_m: Some(10.0),
        },
        bkk_at(2026, 10, 27, 9, 0),
    )
    .await
    .expect("mobile punch");

    let rows = vec![
        ImportRow {
            employee_code: "EMP000".into(),
            date: "2026-10-28".into(),
            in_time: "09:00".into(),
            out_time: "18:00".into(),
        },
        ImportRow {
            employee_code: "EMP000".into(),
            date: "2026-10-27".into(),
            in_time: "09:00".into(),
            out_time: "18:00".into(),
        },
    ];

    let result = import_device_rows(&pool, TenantId(tenant_id), Uuid::new_v4(), rows)
        .await
        .expect("import should succeed");

    assert_eq!(result.imported, 1);
    assert_eq!(result.skipped, 1);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].row, 2);
    assert!(
        result.errors[0].message.contains("mobile"),
        "skip message should mention mobile: {}",
        result.errors[0].message
    );

    // Mobile event must remain, no import events added.
    assert_eq!(
        event_count_by_source(
            &raw,
            tenant_id,
            employee_id,
            bkk_date(2026, 10, 27),
            "mobile"
        )
        .await,
        1
    );
    assert_eq!(
        event_count_by_source(
            &raw,
            tenant_id,
            employee_id,
            bkk_date(2026, 10, 27),
            "import"
        )
        .await,
        0
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn no_shift_reports_error() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, _entity_id, employee_id) = seed_tenant_employee(&raw).await;
    // No shift assigned.

    let rows = vec![ImportRow {
        employee_code: "EMP000".into(),
        date: "2026-10-27".into(),
        in_time: "09:00".into(),
        out_time: "18:00".into(),
    }];

    let result = import_device_rows(&pool, TenantId(tenant_id), Uuid::new_v4(), rows)
        .await
        .expect("batch should succeed");

    assert_eq!(result.imported, 0);
    assert_eq!(result.errors.len(), 1);
    assert!(
        result.errors[0].message.contains("shift"),
        "error should mention shift: {}",
        result.errors[0].message
    );
    assert_eq!(
        event_count(&raw, tenant_id, employee_id, bkk_date(2026, 10, 27)).await,
        0
    );

    cleanup(&raw, tenant_id).await;
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

fn day_bounds(date: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    let bkk = chrono::FixedOffset::east_opt(7 * 60 * 60).unwrap();
    let start = bkk
        .with_ymd_and_hms(date.year(), date.month(), date.day(), 0, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    let end = bkk
        .with_ymd_and_hms(date.year(), date.month(), date.day(), 23, 59, 59)
        .unwrap()
        .with_timezone(&Utc);
    (start, end)
}

async fn event_count(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid, date: NaiveDate) -> i64 {
    let (start, end) = day_bounds(date);
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

async fn event_count_by_source(
    pool: &PgPool,
    tenant_id: Uuid,
    employee_id: Uuid,
    date: NaiveDate,
    source: &str,
) -> i64 {
    let (start, end) = day_bounds(date);
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query(
        "SELECT count(*)::int8 FROM attendance_events
          WHERE tenant_id = $1 AND employee_id = $2 AND occurred_at BETWEEN $3 AND $4
            AND source = $5",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .bind(start)
    .bind(end)
    .bind(source)
    .fetch_one(&mut *tx)
    .await
    .expect("count events");
    let _ = tx.rollback().await;
    row.get::<i64, _>(0)
}

//! Attendance shift service tests against PostgreSQL.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::NaiveTime;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::attendance::{create_shift, set_geofence, ShiftInput};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_employee};

#[tokio::test]
async fn create_and_assign_shift_round_trip() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, _entity_id, employee_id) = seed_tenant_employee(&raw).await;

    let input = ShiftInput {
        name: "morning".into(),
        start: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
        end: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        break_minutes: 60,
        grace_minutes: 5,
    };

    let shift = create_shift(&pool, TenantId(tenant_id), input)
        .await
        .expect("create shift should succeed");
    assert_eq!(shift.name, "morning");
    assert_eq!(shift.break_minutes, 60);
    assert_eq!(shift.grace_minutes, 5);

    talentos_hr_runner::attendance::assign_shift(
        &pool,
        TenantId(tenant_id),
        employee_id,
        shift.id,
        chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
    )
    .await
    .expect("assign shift should succeed");

    let active = talentos_hr_runner::attendance::active_shift(
        &pool,
        TenantId(tenant_id),
        employee_id,
        chrono::NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(),
    )
    .await
    .expect("active_shift should succeed")
    .expect("employee should have an active shift");
    assert_eq!(active.id, shift.id);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn shifts_are_isolated_by_tenant() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_a, _entity_a, _employee_a) = seed_tenant_employee(&raw).await;
    let (tenant_b, _entity_b, _employee_b) = seed_tenant_employee(&raw).await;

    let input = ShiftInput {
        name: "morning".into(),
        start: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
        end: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        break_minutes: 60,
        grace_minutes: 0,
    };

    let shift_a = create_shift(&pool, TenantId(tenant_a), input.clone())
        .await
        .expect("create shift A");

    // Tenant A sees its shift; tenant B does not.
    let count_a = count_shifts(&pool, tenant_a).await;
    let count_b = count_shifts(&pool, tenant_b).await;
    assert_eq!(count_a, 1);
    assert_eq!(count_b, 0);

    // Tenant B cannot resolve A's shift for assignment.
    let result = talentos_hr_runner::attendance::assign_shift(
        &pool,
        TenantId(tenant_b),
        _employee_b,
        shift_a.id,
        chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
    )
    .await;
    assert!(result.is_err(), "tenant B must not assign tenant A's shift");

    cleanup(&raw, tenant_a).await;
    cleanup(&raw, tenant_b).await;
}

#[tokio::test]
async fn invalid_geofence_radius_is_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, _entity_id, _employee_id) = seed_tenant_employee(&raw).await;

    for radius in [0, 49, 5001] {
        let result = set_geofence(&pool, TenantId(tenant_id), 13.7563, 100.5018, radius).await;
        assert!(result.is_err(), "radius {radius} must be rejected");
    }

    set_geofence(&pool, TenantId(tenant_id), 13.7563, 100.5018, 100)
        .await
        .expect("valid radius should succeed");

    cleanup(&raw, tenant_id).await;
}

async fn count_shifts(pool: &TenantPool, tenant_id: Uuid) -> i64 {
    let mut tx = pool.begin_tenant(TenantId(tenant_id)).await.expect("begin");
    let row = sqlx::query("SELECT count(*)::int8 FROM work_shifts")
        .fetch_one(&mut *tx)
        .await
        .expect("count");
    let _ = tx.rollback().await;
    row.get::<i64, _>(0)
}

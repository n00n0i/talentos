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
use common::{cleanup, database_url, seed_tenant_employee, seed_tenant_entities};

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

    let shift = create_shift(&pool, TenantId(tenant_id), _entity_id, input)
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

    let shift_a = create_shift(&pool, TenantId(tenant_a), _entity_a, input.clone())
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
        let result = set_geofence(
            &pool,
            TenantId(tenant_id),
            _entity_id,
            13.7563,
            100.5018,
            radius,
        )
        .await;
        assert!(result.is_err(), "radius {radius} must be rejected");
    }

    set_geofence(
        &pool,
        TenantId(tenant_id),
        _entity_id,
        13.7563,
        100.5018,
        100,
    )
    .await
    .expect("valid radius should succeed");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn two_entities_in_one_tenant_stay_isolated() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, mut entities) = seed_tenant_entities(&raw, 2).await;
    let (entity_a, _employee_a) = entities.pop().expect("entity A");
    let (entity_b, _employee_b) = entities.pop().expect("entity B");

    let input = ShiftInput {
        name: "night".into(),
        start: NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
        end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
        break_minutes: 60,
        grace_minutes: 0,
    };

    create_shift(&pool, TenantId(tenant_id), entity_b, input)
        .await
        .expect("create shift in entity B");

    // Entity A has no shifts; entity B has one.
    assert_eq!(count_shifts_for_entity(&pool, tenant_id, entity_a).await, 0);
    assert_eq!(count_shifts_for_entity(&pool, tenant_id, entity_b).await, 1);

    // Setting geofence on B leaves A unchanged (null).
    set_geofence(&pool, TenantId(tenant_id), entity_b, 13.0, 100.0, 200)
        .await
        .expect("set geofence B");
    let (lat_a, lng_a) = geofence_of(&pool, tenant_id, entity_a).await;
    assert!(
        lat_a.is_none() && lng_a.is_none(),
        "entity A geofence must stay null"
    );
    let (lat_b, lng_b) = geofence_of(&pool, tenant_id, entity_b).await;
    assert!(
        lat_b.is_some() && lng_b.is_some(),
        "entity B geofence must be set"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn assign_employee_to_shift_from_different_entity_fails() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, mut entities) = seed_tenant_entities(&raw, 2).await;
    let (_entity_a, employee_a) = entities.pop().expect("entity A");
    let (entity_b, _employee_b) = entities.pop().expect("entity B");

    let input = ShiftInput {
        name: "morning".into(),
        start: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
        end: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        break_minutes: 60,
        grace_minutes: 0,
    };
    let shift_b = create_shift(&pool, TenantId(tenant_id), entity_b, input)
        .await
        .expect("create shift B");

    let result = talentos_hr_runner::attendance::assign_shift(
        &pool,
        TenantId(tenant_id),
        employee_a,
        shift_b.id,
        chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
    )
    .await;
    assert!(result.is_err(), "cross-entity assignment must fail");

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

async fn count_shifts_for_entity(pool: &TenantPool, tenant_id: Uuid, entity_id: Uuid) -> i64 {
    let mut tx = pool.begin_tenant(TenantId(tenant_id)).await.expect("begin");
    let row = sqlx::query("SELECT count(*)::int8 FROM work_shifts WHERE entity_id = $1")
        .bind(entity_id)
        .fetch_one(&mut *tx)
        .await
        .expect("count");
    let _ = tx.rollback().await;
    row.get::<i64, _>(0)
}

async fn geofence_of(
    pool: &TenantPool,
    tenant_id: Uuid,
    entity_id: Uuid,
) -> (Option<rust_decimal::Decimal>, Option<rust_decimal::Decimal>) {
    let mut tx = pool.begin_tenant(TenantId(tenant_id)).await.expect("begin");
    let row = sqlx::query("SELECT geofence_lat, geofence_lng FROM entities WHERE id = $1")
        .bind(entity_id)
        .fetch_one(&mut *tx)
        .await
        .expect("geofence");
    let _ = tx.rollback().await;
    (
        row.try_get::<rust_decimal::Decimal, _>("geofence_lat").ok(),
        row.try_get::<rust_decimal::Decimal, _>("geofence_lng").ok(),
    )
}

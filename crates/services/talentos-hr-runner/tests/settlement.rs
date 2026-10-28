//! Final settlement service tests.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::NaiveDate;
use rust_decimal_macros::dec;
use sqlx::PgPool;
use talentos_db::TenantPool;
use talentos_hr_runner::settlement::{
    compute_and_store, list_settlements, SettlementError, SettlementInput,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn input(termination: NaiveDate, unused: i32, notice: i32, severance: bool) -> SettlementInput {
    SettlementInput {
        termination_date: termination,
        unused_leave_days: unused,
        notice_days: notice,
        include_severance: severance,
    }
}

#[tokio::test]
async fn proration_on_day_15() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    // Seed uses base_salary = 30,000, hire_date = 2024-01-01.
    let view = compute_and_store(
        &pool,
        tenant,
        user_id,
        entities[0].1,
        input(d(2026, 1, 15), 0, 0, false),
    )
    .await
    .expect("compute settlement");

    assert_eq!(view.last_month_pay, dec!(15000.00));
    assert_eq!(view.leave_pay, dec!(0.00));
    assert_eq!(view.notice_pay, dec!(0.00));
    assert_eq!(view.severance_pay, dec!(0.00));
    assert_eq!(view.total_gross, dec!(15000.00));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn unused_leave_pay() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let view = compute_and_store(
        &pool,
        tenant,
        user_id,
        entities[0].1,
        input(d(2026, 1, 1), 5, 0, false),
    )
    .await
    .expect("compute settlement");

    assert_eq!(view.leave_pay, dec!(5000.00));
    // last_month_pay for day 1 adds 1,000.
    assert_eq!(view.total_gross, dec!(6000.00));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn notice_pay() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let view = compute_and_store(
        &pool,
        tenant,
        user_id,
        entities[0].1,
        input(d(2026, 1, 1), 0, 30, false),
    )
    .await
    .expect("compute settlement");

    assert_eq!(view.notice_pay, dec!(30000.00));
    // last_month_pay for day 1 adds 1,000.
    assert_eq!(view.total_gross, dec!(31000.00));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn severance_tier_two_years() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    // Hired 2024-01-01; terminated 2025-12-31 is one completed year.
    let view = compute_and_store(
        &pool,
        tenant,
        user_id,
        entities[0].1,
        input(d(2025, 12, 31), 0, 0, true),
    )
    .await
    .expect("compute settlement");

    // 1 completed year -> the 1–3 year tier is 90 days -> 30,000 * 90 / 30.
    assert_eq!(view.years_of_service, 1.0);
    assert_eq!(view.severance_pay, dec!(90000.00));
    assert_eq!(view.breakdown["severance_days"], 90);
    // termination day 31 is capped at 30 days -> 30,000 + severance 90,000.
    assert_eq!(view.total_gross, dec!(120000.00));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn zero_severance_when_excluded() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let view = compute_and_store(
        &pool,
        tenant,
        user_id,
        entities[0].1,
        input(d(2026, 1, 1), 0, 0, false),
    )
    .await
    .expect("compute settlement");

    assert_eq!(view.severance_pay, dec!(0.00));
    // last_month_pay for day 1 is still due.
    assert_eq!(view.total_gross, dec!(1000.00));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn totals_round_to_two_decimal_places() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let view = compute_and_store(
        &pool,
        tenant,
        user_id,
        entities[0].1,
        input(d(2026, 1, 15), 5, 30, true),
    )
    .await
    .expect("compute settlement");

    // daily_rate = 30,000 / 30 = 1,000
    // last_month = 15 * 1,000 = 15,000
    // leave = 5 * 1,000 = 5,000
    // notice = 30 * 1,000 = 30,000
    // severance = 90,000
    assert_eq!(view.last_month_pay, dec!(15000.00));
    assert_eq!(view.leave_pay, dec!(5000.00));
    assert_eq!(view.notice_pay, dec!(30000.00));
    assert_eq!(view.severance_pay, dec!(90000.00));
    assert_eq!(view.total_gross, dec!(140000.00));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn unknown_employee_returns_not_found() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let err = compute_and_store(
        &pool,
        tenant,
        user_id,
        Uuid::new_v4(),
        input(d(2026, 1, 1), 0, 0, false),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, SettlementError::EmployeeNotFound));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn tenant_isolation_hides_cross_tenant_settlement() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool_a = TenantPool::connect(&url, 2).await.expect("tenant pool a");
    let pool_b = TenantPool::connect(&url, 2).await.expect("tenant pool b");
    let (tenant_a, entities_a) = seed_tenant_entities(&raw, 1).await;
    let (tenant_b, _entities_b) = seed_tenant_entities(&raw, 1).await;
    let user_a = user_id_of(&raw, tenant_a, entities_a[0].1).await;

    let view = compute_and_store(
        &pool_a,
        TenantId(tenant_a),
        user_a,
        entities_a[0].1,
        input(d(2026, 1, 1), 0, 0, false),
    )
    .await
    .expect("compute settlement");

    // Tenant B cannot see the settlement.
    let list_b = list_settlements(&pool_b, TenantId(tenant_b), None)
        .await
        .expect("list tenant b");
    assert!(list_b.is_empty());

    // Tenant A can see it.
    let list_a = list_settlements(&pool_a, TenantId(tenant_a), None)
        .await
        .expect("list tenant a");
    assert_eq!(list_a.len(), 1);
    assert_eq!(list_a[0].id, view.id);

    cleanup(&raw, tenant_a).await;
    cleanup(&raw, tenant_b).await;
}

#[tokio::test]
async fn termination_before_hire_is_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let err = compute_and_store(
        &pool,
        tenant,
        user_id,
        entities[0].1,
        input(d(2023, 12, 31), 0, 0, false),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, SettlementError::InvalidTerminationDate));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn negative_days_are_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let err = compute_and_store(
        &pool,
        tenant,
        user_id,
        entities[0].1,
        input(d(2026, 1, 1), -1, 0, false),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        err,
        SettlementError::NegativeDays("unused_leave_days")
    ));

    cleanup(&raw, tenant_id).await;
}

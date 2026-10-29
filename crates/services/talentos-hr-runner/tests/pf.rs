//! PF rate history service tests against PostgreSQL.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use talentos_db::TenantPool;
use talentos_hr_runner::{list_rates, rate_for, upsert_rate, PfError};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_employee};

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[tokio::test]
async fn defaults_to_five_percent_without_history() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL unset");
        return;
    };
    let raw = sqlx::PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 4).await.expect("pool");
    let (tenant_id, _entity_id, employee_id) = seed_tenant_employee(&raw).await;

    let rate = rate_for(&pool, TenantId(tenant_id), employee_id, date(2026, 10, 31))
        .await
        .expect("rate");
    assert_eq!(rate, Decimal::new(5, 2));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn latest_rate_not_later_than_as_of_wins() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL unset");
        return;
    };
    let raw = sqlx::PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 4).await.expect("pool");
    let (tenant_id, _entity_id, employee_id) = seed_tenant_employee(&raw).await;

    upsert_rate(
        &pool,
        TenantId(tenant_id),
        employee_id,
        date(2026, 1, 1),
        Decimal::new(3, 2),
        None,
    )
    .await
    .expect("3% from Jan");
    upsert_rate(
        &pool,
        TenantId(tenant_id),
        employee_id,
        date(2026, 7, 1),
        Decimal::new(10, 2),
        None,
    )
    .await
    .expect("10% from Jul");

    // Before July the January rate applies; on/after July the newer rate wins.
    assert_eq!(
        rate_for(&pool, TenantId(tenant_id), employee_id, date(2026, 6, 30))
            .await
            .expect("rate"),
        Decimal::new(3, 2)
    );
    assert_eq!(
        rate_for(&pool, TenantId(tenant_id), employee_id, date(2026, 7, 1))
            .await
            .expect("rate"),
        Decimal::new(10, 2)
    );
    assert_eq!(
        rate_for(&pool, TenantId(tenant_id), employee_id, date(2026, 12, 31))
            .await
            .expect("rate"),
        Decimal::new(10, 2)
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn future_rate_is_ignored() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL unset");
        return;
    };
    let raw = sqlx::PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 4).await.expect("pool");
    let (tenant_id, _entity_id, employee_id) = seed_tenant_employee(&raw).await;

    upsert_rate(
        &pool,
        TenantId(tenant_id),
        employee_id,
        date(2026, 9, 1),
        Decimal::new(8, 2),
        None,
    )
    .await
    .expect("8% from Sep");

    // A rate effective after the as_of date must not be picked.
    assert_eq!(
        rate_for(&pool, TenantId(tenant_id), employee_id, date(2026, 8, 31))
            .await
            .expect("rate"),
        Decimal::new(5, 2),
        "no rate effective before 31 Aug, so default 5%"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn upsert_same_date_replaces_the_rate() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL unset");
        return;
    };
    let raw = sqlx::PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 4).await.expect("pool");
    let (tenant_id, _entity_id, employee_id) = seed_tenant_employee(&raw).await;

    upsert_rate(
        &pool,
        TenantId(tenant_id),
        employee_id,
        date(2026, 5, 1),
        Decimal::new(4, 2),
        None,
    )
    .await
    .expect("4%");
    upsert_rate(
        &pool,
        TenantId(tenant_id),
        employee_id,
        date(2026, 5, 1),
        Decimal::new(6, 2),
        None,
    )
    .await
    .expect("6% same date");

    let rates = list_rates(&pool, TenantId(tenant_id), employee_id)
        .await
        .expect("list");
    assert_eq!(rates.len(), 1, "same date upserts, no second row");
    assert_eq!(rates[0].rate, Decimal::new(6, 2));
    assert_eq!(
        rate_for(&pool, TenantId(tenant_id), employee_id, date(2026, 5, 1))
            .await
            .expect("rate"),
        Decimal::new(6, 2)
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn rate_out_of_bounds_is_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL unset");
        return;
    };
    let raw = sqlx::PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 4).await.expect("pool");
    let (tenant_id, _entity_id, employee_id) = seed_tenant_employee(&raw).await;

    assert!(matches!(
        upsert_rate(
            &pool,
            TenantId(tenant_id),
            employee_id,
            date(2026, 1, 1),
            Decimal::new(16, 2),
            None
        )
        .await,
        Err(PfError::RateOutOfRange)
    ));
    assert!(matches!(
        upsert_rate(
            &pool,
            TenantId(tenant_id),
            employee_id,
            date(2026, 1, 1),
            Decimal::new(-1, 2),
            None
        )
        .await,
        Err(PfError::RateOutOfRange)
    ));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn rates_are_scoped_to_the_tenant() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL unset");
        return;
    };
    let raw = sqlx::PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 4).await.expect("pool");
    let (tenant_id, _entity_id, employee_id) = seed_tenant_employee(&raw).await;

    // A second tenant + employee that must not see the first tenant's rate.
    let other_tenant = Uuid::new_v4();
    let other_employee = Uuid::new_v4();
    let other_entity = Uuid::new_v4();
    let other_user = Uuid::new_v4();
    {
        let mut tx = raw.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(other_tenant.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'Other', $2)")
            .bind(other_tenant)
            .bind(format!("other-{}", &other_tenant.to_string()[..8]))
            .execute(&mut *tx)
            .await
            .expect("tenant");
        sqlx::query("INSERT INTO entities (id, tenant_id, name) VALUES ($1, $2, 'Main')")
            .bind(other_entity)
            .bind(other_tenant)
            .execute(&mut *tx)
            .await
            .expect("entity");
        sqlx::query(
            "INSERT INTO users (id, tenant_id, email, password_hash, is_active) VALUES ($1, $2, $3, 'x', true)",
        )
        .bind(other_user)
        .bind(other_tenant)
        .bind(format!("o-{}@example.com", &other_user.to_string()[..8]))
        .execute(&mut *tx)
        .await
        .expect("user");
        sqlx::query(
            "INSERT INTO employees (id, tenant_id, entity_id, user_id, employee_code, first_name_th, last_name_th, hire_date, position_title, base_salary, tax_filing_status, pf_member, status)
             VALUES ($1,$2,$3,$4,'EMPX','ทดสอบ','พนักงาน','2024-01-01','Staff',30000,1,false,'active')",
        )
        .bind(other_employee)
        .bind(other_tenant)
        .bind(other_entity)
        .bind(other_user)
        .execute(&mut *tx)
        .await
        .expect("employee");
        tx.commit().await.expect("commit");
    }

    upsert_rate(
        &pool,
        TenantId(tenant_id),
        employee_id,
        date(2026, 1, 1),
        Decimal::new(7, 2),
        None,
    )
    .await
    .expect("rate for first tenant");

    // The second tenant still falls back to 5% (no visible history).
    assert_eq!(
        rate_for(
            &pool,
            TenantId(other_tenant),
            other_employee,
            date(2026, 6, 1)
        )
        .await
        .expect("rate"),
        Decimal::new(5, 2)
    );

    // Upserting for an employee outside the tenant is refused.
    assert!(matches!(
        upsert_rate(
            &pool,
            TenantId(other_tenant),
            employee_id,
            date(2026, 1, 1),
            Decimal::new(7, 2),
            None
        )
        .await,
        Err(PfError::EmployeeNotFound)
    ));

    // Clean both tenants (employee deletion cascades to pf_rate_history).
    cleanup(&raw, tenant_id).await;
    cleanup(&raw, other_tenant).await;
}

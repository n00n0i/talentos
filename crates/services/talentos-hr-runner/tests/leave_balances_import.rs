//! Opening leave-balance import tests.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::leave::{import_balances, BalanceImportRow};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_entities};

fn row(
    code: &str,
    leave_type: &str,
    year: i32,
    total: Decimal,
    used: Option<Decimal>,
) -> BalanceImportRow {
    BalanceImportRow {
        employee_code: code.into(),
        leave_type: leave_type.into(),
        year,
        total_days: total,
        used_days: used,
    }
}

async fn balance_row_count(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid, year: i32) -> i64 {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*)::int8 FROM leave_balances WHERE tenant_id = $1 AND employee_id = $2 AND year = $3",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .bind(year)
    .fetch_one(&mut *tx)
    .await
    .expect("count balances");
    let _ = tx.rollback().await;
    count
}

async fn annual_balance(
    pool: &PgPool,
    tenant_id: Uuid,
    employee_id: Uuid,
    year: i32,
) -> Option<(Decimal, Decimal)> {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query(
        r#"SELECT lb.total_days, lb.used_days
           FROM leave_balances lb
           JOIN leave_types lt ON lt.id = lb.leave_type_id
           WHERE lb.tenant_id = $1 AND lb.employee_id = $2 AND lb.year = $3 AND lt.name = 'annual'"#,
    )
    .bind(tenant_id)
    .bind(employee_id)
    .bind(year)
    .fetch_optional(&mut *tx)
    .await
    .expect("fetch balance");
    let _ = tx.rollback().await;
    row.map(|r| {
        let total: Decimal = r.get("total_days");
        let used: Decimal = r.get("used_days");
        (total, used)
    })
}

#[tokio::test]
async fn dry_run_writes_nothing() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let report = import_balances(
        &pool,
        tenant,
        true,
        vec![row("EMP000", "annual", 2026, dec!(10), Some(dec!(0)))],
    )
    .await
    .expect("import should succeed");

    assert_eq!(report.total, 1);
    assert_eq!(report.valid, 1);
    assert_eq!(report.invalid, 0);
    assert_eq!(report.created, 0);
    assert_eq!(
        balance_row_count(&raw, tenant_id, entities[0].1, 2026).await,
        0
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn valid_row_upserts_balance() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let report = import_balances(
        &pool,
        tenant,
        false,
        vec![row("EMP000", "annual", 2026, dec!(10), Some(dec!(2)))],
    )
    .await
    .expect("import should succeed");

    assert_eq!(report.total, 1);
    assert_eq!(report.valid, 1);
    assert_eq!(report.invalid, 0);
    assert_eq!(report.created, 1);
    assert_eq!(
        balance_row_count(&raw, tenant_id, entities[0].1, 2026).await,
        1
    );
    assert_eq!(
        annual_balance(&raw, tenant_id, entities[0].1, 2026).await,
        Some((dec!(10), dec!(2)))
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn duplicate_in_file_marks_both_invalid() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let rows = vec![
        row("EMP000", "annual", 2026, dec!(10), Some(dec!(0))),
        row("EMP000", "annual", 2026, dec!(12), Some(dec!(0))),
    ];
    let report = import_balances(&pool, tenant, false, rows)
        .await
        .expect("import should succeed");

    assert_eq!(report.total, 2);
    assert_eq!(report.valid, 0);
    assert_eq!(report.invalid, 2);
    assert_eq!(report.created, 0);
    assert!(report.rows.iter().all(|r| r.status == "invalid"));
    assert!(report
        .rows
        .iter()
        .any(|r| r.errors.iter().any(|e| e.contains("duplicate"))));
    assert_eq!(
        balance_row_count(&raw, tenant_id, entities[0].1, 2026).await,
        0
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn used_greater_than_total_is_invalid() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let report = import_balances(
        &pool,
        tenant,
        false,
        vec![row("EMP000", "annual", 2026, dec!(5), Some(dec!(6)))],
    )
    .await
    .expect("import should succeed");

    assert_eq!(report.total, 1);
    assert_eq!(report.valid, 0);
    assert_eq!(report.invalid, 1);
    assert_eq!(report.created, 0);
    assert!(report.rows[0]
        .errors
        .iter()
        .any(|e| e.contains("used_days must not exceed total_days")));
    assert_eq!(
        balance_row_count(&raw, tenant_id, entities[0].1, 2026).await,
        0
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn unknown_employee_is_invalid() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let report = import_balances(
        &pool,
        tenant,
        false,
        vec![row("NOBODY", "annual", 2026, dec!(10), Some(dec!(0)))],
    )
    .await
    .expect("import should succeed");

    assert_eq!(report.total, 1);
    assert_eq!(report.valid, 0);
    assert_eq!(report.invalid, 1);
    assert_eq!(report.created, 0);
    assert!(report.rows[0]
        .errors
        .iter()
        .any(|e| e.contains("employee_code") && e.contains("not found")));
    assert_eq!(
        balance_row_count(&raw, tenant_id, entities[0].1, 2026).await,
        0
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn second_import_updates_same_row() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let first = import_balances(
        &pool,
        tenant,
        false,
        vec![row("EMP000", "annual", 2026, dec!(10), Some(dec!(0)))],
    )
    .await
    .expect("first import");
    assert_eq!(first.created, 1);

    let second = import_balances(
        &pool,
        tenant,
        false,
        vec![row("EMP000", "annual", 2026, dec!(12), Some(dec!(1)))],
    )
    .await
    .expect("second import");
    assert_eq!(second.created, 1);

    assert_eq!(
        balance_row_count(&raw, tenant_id, entities[0].1, 2026).await,
        1
    );
    assert_eq!(
        annual_balance(&raw, tenant_id, entities[0].1, 2026).await,
        Some((dec!(12), dec!(1)))
    );

    cleanup(&raw, tenant_id).await;
}

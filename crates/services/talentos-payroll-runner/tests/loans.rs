//! Loan / advance deduction integration tests.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::loans::reverse_installment;
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;

use common::{cleanup, database_url, recovery_url, seed};

async fn seed_loan(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    employee_id: Uuid,
    principal: Decimal,
    installment: Decimal,
) {
    let mut tx = pool.begin().await.expect("begin seed_loan");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        r#"INSERT INTO employee_loans
             (id, tenant_id, employee_id, principal, balance, installment_amount,
              start_date, status, created_by)
           VALUES ($1,$2,$3,$4,$4,$5,'2026-10-01','active',$6)"#,
    )
    .bind(Uuid::new_v4())
    .bind(tenant_id)
    .bind(employee_id)
    .bind(principal)
    .bind(installment)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .expect("insert loan");
    tx.commit().await.expect("commit seed_loan");
}

async fn loan_state(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid) -> (Decimal, String) {
    let mut tx = pool.begin().await.expect("begin loan_state");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query(
        "SELECT balance, status FROM employee_loans WHERE employee_id = $1 AND tenant_id = current_tenant_id()",
    )
    .bind(employee_id)
    .fetch_one(&mut *tx)
    .await
    .expect("loan row");
    let balance: Decimal = row.try_get("balance").expect("balance");
    let status: String = row.try_get("status").expect("status");
    let _ = tx.rollback().await;
    (balance, status)
}

async fn installment_count(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid) -> i64 {
    let mut tx = pool.begin().await.expect("begin installment_count");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint
             FROM loan_installments li
             JOIN employee_loans l ON l.id = li.loan_id
            WHERE l.employee_id = $1 AND l.tenant_id = current_tenant_id()"#,
    )
    .bind(employee_id)
    .fetch_one(&mut *tx)
    .await
    .expect("count installments");
    let _ = tx.rollback().await;
    count
}

async fn loan_id_of(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid) -> Uuid {
    let mut tx = pool.begin().await.expect("begin loan_id_of");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let id: Uuid = sqlx::query_scalar(
        "SELECT id FROM employee_loans WHERE employee_id = $1 AND tenant_id = current_tenant_id()",
    )
    .bind(employee_id)
    .fetch_one(&mut *tx)
    .await
    .expect("loan id");
    let _ = tx.rollback().await;
    id
}

async fn payslip_loan_deduction(
    pool: &PgPool,
    tenant_id: Uuid,
    run_id: Uuid,
    employee_id: Uuid,
) -> Option<Decimal> {
    let mut tx = pool.begin().await.expect("begin payslip_loan_deduction");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row: Option<Decimal> = sqlx::query_scalar(
        "SELECT loan_deduction FROM payslips WHERE payroll_run_id = $1 AND employee_id = $2 AND tenant_id = current_tenant_id()",
    )
    .bind(run_id)
    .bind(employee_id)
    .fetch_optional(&mut *tx)
    .await
    .expect("payslip loan");
    let _ = tx.rollback().await;
    row
}

#[tokio::test]
async fn execute_run_deducts_loan_installment_and_reduces_balance() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, _entity_id, run_id, employee_ids) =
        seed(&raw, &[("EMP100", Decimal::new(30_000, 0))]).await;
    let employee_id = employee_ids[0];

    // Need a user id for created_by; reuse the one seed created via employee lookup is not
    // possible here, so we use a subquery for the first user in the tenant.
    let user_id: Uuid = {
        let mut tx = raw.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let id: Uuid = sqlx::query_scalar(
            "SELECT id FROM users WHERE tenant_id = current_tenant_id() LIMIT 1",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("user");
        let _ = tx.rollback().await;
        id
    };

    seed_loan(
        &raw,
        tenant_id,
        user_id,
        employee_id,
        dec!(10_000),
        dec!(1_000),
    )
    .await;

    let summary = talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run should succeed");

    assert_eq!(summary.total_loan, dec!(1_000));

    let deduction = payslip_loan_deduction(&raw, tenant_id, run_id, employee_id)
        .await
        .expect("payslip should exist");
    assert_eq!(deduction, dec!(1_000));

    let (balance, status) = loan_state(&raw, tenant_id, employee_id).await;
    assert_eq!(balance, dec!(9_000));
    assert_eq!(status, "active");

    let count = installment_count(&raw, tenant_id, employee_id).await;
    assert_eq!(count, 1);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn rerun_does_not_double_deduct() {
    let (Some(url), Some(auth_url)) = (database_url(), recovery_url()) else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&auth_url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, _entity_id, run_id, employee_ids) =
        seed(&raw, &[("EMP101", Decimal::new(30_000, 0))]).await;
    let employee_id = employee_ids[0];

    let user_id: Uuid = {
        let mut tx = raw.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let id: Uuid = sqlx::query_scalar(
            "SELECT id FROM users WHERE tenant_id = current_tenant_id() LIMIT 1",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("user");
        let _ = tx.rollback().await;
        id
    };

    seed_loan(
        &raw,
        tenant_id,
        user_id,
        employee_id,
        dec!(10_000),
        dec!(1_000),
    )
    .await;

    talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("first run");

    let (balance_after_first, _) = loan_state(&raw, tenant_id, employee_id).await;
    assert_eq!(balance_after_first, dec!(9_000));

    // Simulate crash recovery: re-execute the same run from the calculating state.
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "UPDATE payroll_runs SET status = 'calculating', updated_at = now() - interval '1 hour' WHERE id = $1",
    )
    .bind(run_id)
    .execute(&mut *tx)
    .await
    .expect("mark calculating");
    let _ = tx.commit().await;

    talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("recovery run");

    let (balance_after_second, _) = loan_state(&raw, tenant_id, employee_id).await;
    assert_eq!(balance_after_second, dec!(9_000), "balance must not change");

    let count = installment_count(&raw, tenant_id, employee_id).await;
    assert_eq!(count, 1, "only one installment row");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn failed_payslip_skips_loan_deduction() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, _entity_id, run_id, employee_ids) =
        seed(&raw, &[("EMP102", Decimal::new(3_000, 0))]).await;
    let employee_id = employee_ids[0];

    let user_id: Uuid = {
        let mut tx = raw.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let id: Uuid = sqlx::query_scalar(
            "SELECT id FROM users WHERE tenant_id = current_tenant_id() LIMIT 1",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("user");
        let _ = tx.rollback().await;
        id
    };

    seed_loan(
        &raw,
        tenant_id,
        user_id,
        employee_id,
        dec!(10_000),
        dec!(5_000),
    )
    .await;

    let summary = talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run completes even when one employee fails");

    assert_eq!(summary.employees, 0);
    assert_eq!(summary.failed, 1);

    let (balance, status) = loan_state(&raw, tenant_id, employee_id).await;
    assert_eq!(balance, dec!(10_000), "balance unchanged on failed payslip");
    assert_eq!(status, "active");

    let count = installment_count(&raw, tenant_id, employee_id).await;
    assert_eq!(count, 0, "no installment row for failed payslip");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn final_installment_caps_at_balance_and_closes() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, _entity_id, run_id, employee_ids) =
        seed(&raw, &[("EMP103", Decimal::new(30_000, 0))]).await;
    let employee_id = employee_ids[0];

    let user_id: Uuid = {
        let mut tx = raw.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let id: Uuid = sqlx::query_scalar(
            "SELECT id FROM users WHERE tenant_id = current_tenant_id() LIMIT 1",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("user");
        let _ = tx.rollback().await;
        id
    };

    seed_loan(
        &raw,
        tenant_id,
        user_id,
        employee_id,
        dec!(1_500),
        dec!(2_000),
    )
    .await;

    let summary = talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run should succeed");

    assert_eq!(summary.total_loan, dec!(1_500));

    let deduction = payslip_loan_deduction(&raw, tenant_id, run_id, employee_id)
        .await
        .expect("payslip should exist");
    assert_eq!(deduction, dec!(1_500));

    let (balance, status) = loan_state(&raw, tenant_id, employee_id).await;
    assert_eq!(balance, dec!(0));
    assert_eq!(status, "closed");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn recalc_after_reversal_creates_new_installment_and_deducts_once() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, _entity_id, run_id, employee_ids) =
        seed(&raw, &[("EMP104", Decimal::new(30_000, 0))]).await;
    let employee_id = employee_ids[0];

    let user_id: Uuid = {
        let mut tx = raw.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let id: Uuid = sqlx::query_scalar(
            "SELECT id FROM users WHERE tenant_id = current_tenant_id() LIMIT 1",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("user");
        let _ = tx.rollback().await;
        id
    };

    seed_loan(
        &raw,
        tenant_id,
        user_id,
        employee_id,
        dec!(10_000),
        dec!(1_000),
    )
    .await;
    let loan_id = loan_id_of(&raw, tenant_id, employee_id).await;

    talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("first run");

    let (balance, _) = loan_state(&raw, tenant_id, employee_id).await;
    assert_eq!(balance, dec!(9_000));
    assert_eq!(installment_count(&raw, tenant_id, employee_id).await, 1);

    reverse_installment(&tenant_pool, TenantId(tenant_id), user_id, loan_id, run_id)
        .await
        .expect("reverse");

    let (balance, status) = loan_state(&raw, tenant_id, employee_id).await;
    assert_eq!(balance, dec!(10_000));
    assert_eq!(status, "active");
    assert_eq!(installment_count(&raw, tenant_id, employee_id).await, 1);

    // Recalculate: reset the run and execute again.
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("UPDATE payroll_runs SET status = 'calculating' WHERE id = $1")
        .bind(run_id)
        .execute(&mut *tx)
        .await
        .expect("mark calculating");
    let _ = tx.commit().await;

    talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("recalculated run");

    let (balance, _) = loan_state(&raw, tenant_id, employee_id).await;
    assert_eq!(balance, dec!(9_000), "deducted exactly once after reversal");

    // Two rows total: the reversed one plus a fresh one for the recalculated run.
    assert_eq!(
        installment_count(&raw, tenant_id, employee_id).await,
        2,
        "one reversed + one fresh installment"
    );

    cleanup(&raw, tenant_id).await;
}

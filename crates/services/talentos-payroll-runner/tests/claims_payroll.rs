//! Expense-claim reimbursement through payroll integration tests.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;

use common::{cleanup, database_url, recovery_url, seed};

/// Insert an expense claim and return its id.
async fn seed_claim(
    pool: &PgPool,
    tenant_id: Uuid,
    entity_id: Uuid,
    employee_id: Uuid,
    amount: Decimal,
    expense_date: &str,
    status: &str,
) -> Uuid {
    let claim_id = Uuid::new_v4();
    let mut tx = pool.begin().await.expect("begin seed_claim");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        r#"INSERT INTO expense_claims
             (id, tenant_id, entity_id, employee_id, category, amount, expense_date, status)
           VALUES ($1,$2,$3,$4,'travel',$5,$6::DATE,$7)"#,
    )
    .bind(claim_id)
    .bind(tenant_id)
    .bind(entity_id)
    .bind(employee_id)
    .bind(amount)
    .bind(expense_date)
    .bind(status)
    .execute(&mut *tx)
    .await
    .expect("insert claim");
    tx.commit().await.expect("commit seed_claim");
    claim_id
}

async fn claim_state(
    pool: &PgPool,
    tenant_id: Uuid,
    claim_id: Uuid,
) -> (String, Option<Uuid>, Option<chrono::DateTime<chrono::Utc>>) {
    let mut tx = pool.begin().await.expect("begin claim_state");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query(
        "SELECT status, payroll_run_id, paid_at FROM expense_claims WHERE id = $1 AND tenant_id = current_tenant_id()",
    )
    .bind(claim_id)
    .fetch_one(&mut *tx)
    .await
    .expect("claim row");
    let status: String = row.try_get("status").expect("status");
    let run_id: Option<Uuid> = row.try_get("payroll_run_id").expect("run id");
    let paid_at: Option<chrono::DateTime<chrono::Utc>> = row.try_get("paid_at").expect("paid at");
    let _ = tx.rollback().await;
    (status, run_id, paid_at)
}

async fn payslip_reimbursement(
    pool: &PgPool,
    tenant_id: Uuid,
    run_id: Uuid,
    employee_id: Uuid,
) -> Option<Decimal> {
    let mut tx = pool.begin().await.expect("begin payslip_reimbursement");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row: Option<Decimal> = sqlx::query_scalar(
        "SELECT reimbursement FROM payslips WHERE payroll_run_id = $1 AND employee_id = $2 AND tenant_id = current_tenant_id()",
    )
    .bind(run_id)
    .bind(employee_id)
    .fetch_optional(&mut *tx)
    .await
    .expect("payslip reimbursement");
    let _ = tx.rollback().await;
    row
}

async fn payslip_net(
    pool: &PgPool,
    tenant_id: Uuid,
    run_id: Uuid,
    employee_id: Uuid,
) -> Option<Decimal> {
    let mut tx = pool.begin().await.expect("begin payslip_net");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row: Option<Decimal> = sqlx::query_scalar(
        "SELECT net_pay FROM payslips WHERE payroll_run_id = $1 AND employee_id = $2 AND tenant_id = current_tenant_id()",
    )
    .bind(run_id)
    .bind(employee_id)
    .fetch_optional(&mut *tx)
    .await
    .expect("payslip net");
    let _ = tx.rollback().await;
    row
}

async fn payslip_totals(
    pool: &PgPool,
    tenant_id: Uuid,
    run_id: Uuid,
    employee_id: Uuid,
) -> (Decimal, Decimal) {
    let mut tx = pool.begin().await.expect("begin payslip_totals");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query(
        "SELECT gross_earnings, total_deductions FROM payslips WHERE payroll_run_id = $1 AND employee_id = $2 AND tenant_id = current_tenant_id()",
    )
    .bind(run_id)
    .bind(employee_id)
    .fetch_one(&mut *tx)
    .await
    .expect("payslip totals");
    let gross: Decimal = row.try_get("gross_earnings").expect("gross");
    let deductions: Decimal = row.try_get("total_deductions").expect("deductions");
    let _ = tx.rollback().await;
    (gross, deductions)
}

/// Seed a loan with an installment large enough to make net pay negative.
async fn seed_loan(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    employee_id: Uuid,
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
    .bind(installment)
    .bind(installment)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .expect("insert loan");
    tx.commit().await.expect("commit seed_loan");
}

async fn tenant_user_id(pool: &PgPool, tenant_id: Uuid) -> Uuid {
    let mut tx = pool.begin().await.expect("begin tenant_user_id");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE tenant_id = current_tenant_id() LIMIT 1")
            .fetch_one(&mut *tx)
            .await
            .expect("user");
    let _ = tx.rollback().await;
    id
}

#[tokio::test]
async fn approved_claim_in_period_is_reimbursed_and_marked_paid() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, entity_id, run_id, employee_ids) =
        seed(&raw, &[("EMP200", Decimal::new(30_000, 0))]).await;
    let employee_id = employee_ids[0];

    let claim_id = seed_claim(
        &raw,
        tenant_id,
        entity_id,
        employee_id,
        dec!(1500),
        "2026-10-15",
        "approved",
    )
    .await;

    let summary = talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run should succeed");
    assert_eq!(summary.employees, 1);
    assert_eq!(summary.failed, 0);

    let reimb = payslip_reimbursement(&raw, tenant_id, run_id, employee_id)
        .await
        .expect("payslip exists");
    assert_eq!(reimb, dec!(1500));

    let net = payslip_net(&raw, tenant_id, run_id, employee_id)
        .await
        .unwrap();
    let (gross, deductions) = payslip_totals(&raw, tenant_id, run_id, employee_id).await;
    assert_eq!(net, gross - deductions + dec!(1500));

    let (status, run, paid_at) = claim_state(&raw, tenant_id, claim_id).await;
    assert_eq!(status, "paid");
    assert_eq!(run, Some(run_id));
    assert!(paid_at.is_some());

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn rerun_does_not_double_reimburse() {
    let (Some(url), Some(auth_url)) = (database_url(), recovery_url()) else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&auth_url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, entity_id, run_id, employee_ids) =
        seed(&raw, &[("EMP201", Decimal::new(30_000, 0))]).await;
    let employee_id = employee_ids[0];

    let claim_id = seed_claim(
        &raw,
        tenant_id,
        entity_id,
        employee_id,
        dec!(1200),
        "2026-10-20",
        "approved",
    )
    .await;

    talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("first run");

    // Simulate crash recovery: re-open the run and execute again.
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

    let reimb = payslip_reimbursement(&raw, tenant_id, run_id, employee_id)
        .await
        .unwrap();
    assert_eq!(reimb, dec!(1200), "reimbursement must not double on rerun");

    let (status, run, _) = claim_state(&raw, tenant_id, claim_id).await;
    assert_eq!(status, "paid");
    assert_eq!(run, Some(run_id));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn claim_outside_period_is_ignored() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, entity_id, run_id, employee_ids) =
        seed(&raw, &[("EMP202", Decimal::new(30_000, 0))]).await;
    let employee_id = employee_ids[0];

    let claim_id = seed_claim(
        &raw,
        tenant_id,
        entity_id,
        employee_id,
        dec!(900),
        "2026-09-15",
        "approved",
    )
    .await;

    talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run should succeed");

    let reimb = payslip_reimbursement(&raw, tenant_id, run_id, employee_id)
        .await
        .unwrap_or(dec!(0));
    assert_eq!(reimb, dec!(0));

    let (status, run, paid_at) = claim_state(&raw, tenant_id, claim_id).await;
    assert_eq!(status, "approved");
    assert_eq!(run, None);
    assert!(paid_at.is_none());

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn rejected_claim_is_ignored() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    let (tenant_id, entity_id, run_id, employee_ids) =
        seed(&raw, &[("EMP203", Decimal::new(30_000, 0))]).await;
    let employee_id = employee_ids[0];

    let claim_id = seed_claim(
        &raw,
        tenant_id,
        entity_id,
        employee_id,
        dec!(900),
        "2026-10-15",
        "rejected",
    )
    .await;

    talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run should succeed");

    let reimb = payslip_reimbursement(&raw, tenant_id, run_id, employee_id)
        .await
        .unwrap_or(dec!(0));
    assert_eq!(reimb, dec!(0));

    let (status, _, _) = claim_state(&raw, tenant_id, claim_id).await;
    assert_eq!(status, "rejected");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn failed_payslip_leaves_claim_approved() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    let raw = PgPool::connect(&url).await.expect("connect");
    let tenant_pool = TenantPool::connect(&url, 4).await.expect("tenant pool");
    // Low salary + oversized loan installment forces a negative-net failure.
    let (tenant_id, entity_id, run_id, employee_ids) =
        seed(&raw, &[("EMP204", Decimal::new(3_000, 0))]).await;
    let employee_id = employee_ids[0];

    let user_id = tenant_user_id(&raw, tenant_id).await;
    seed_loan(&raw, tenant_id, user_id, employee_id, dec!(5_000)).await;

    let claim_id = seed_claim(
        &raw,
        tenant_id,
        entity_id,
        employee_id,
        dec!(1500),
        "2026-10-15",
        "approved",
    )
    .await;

    let summary = talentos_payroll_runner::execute_run(&tenant_pool, TenantId(tenant_id), run_id)
        .await
        .expect("run completes even with a failed payslip");

    assert_eq!(summary.employees, 0);
    assert_eq!(summary.failed, 1);

    let (status, run, paid_at) = claim_state(&raw, tenant_id, claim_id).await;
    assert_eq!(
        status, "approved",
        "failed payslip must not mark the claim paid"
    );
    assert_eq!(run, None);
    assert!(paid_at.is_none());

    cleanup(&raw, tenant_id).await;
}

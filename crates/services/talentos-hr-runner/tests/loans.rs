//! Manual loan payoff service tests.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::loans::{
    list_installments, list_payments, payoff, reverse_installment, PayoffError, ReverseError,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

async fn seed_loan(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    employee_id: Uuid,
    principal: Decimal,
) -> Uuid {
    let loan_id = Uuid::new_v4();
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
    .bind(loan_id)
    .bind(tenant_id)
    .bind(employee_id)
    .bind(principal)
    .bind(Decimal::new(1_000, 0))
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .expect("insert loan");
    tx.commit().await.expect("commit seed_loan");
    loan_id
}

async fn loan_state(pool: &PgPool, tenant_id: Uuid, loan_id: Uuid) -> (Decimal, String) {
    let mut tx = pool.begin().await.expect("begin loan_state");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query("SELECT balance, status FROM employee_loans WHERE id = $1")
        .bind(loan_id)
        .fetch_one(&mut *tx)
        .await
        .expect("loan row");
    let balance: Decimal = row.try_get("balance").expect("balance");
    let status: String = row.try_get("status").expect("status");
    let _ = tx.rollback().await;
    (balance, status)
}

/// Seed a payroll run (with the given status) plus one loan installment, and set
/// the loan balance/status to a post-deduction state. Returns the run id.
#[allow(clippy::too_many_arguments)]
async fn seed_run_and_installment(
    pool: &PgPool,
    tenant_id: Uuid,
    entity_id: Uuid,
    user_id: Uuid,
    loan_id: Uuid,
    run_status: &str,
    amount: Decimal,
    balance_after: Decimal,
    loan_status: &str,
) -> Uuid {
    let run_id = Uuid::new_v4();
    let mut tx = pool.begin().await.expect("begin seed_run_and_installment");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        r#"INSERT INTO payroll_runs
             (id, tenant_id, entity_id, period_start, period_end, payment_date, status, created_by)
           VALUES ($1,$2,$3,'2026-10-01','2026-10-31','2026-11-01',$4,$5)"#,
    )
    .bind(run_id)
    .bind(tenant_id)
    .bind(entity_id)
    .bind(run_status)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .expect("run");
    sqlx::query(
        "INSERT INTO loan_installments (tenant_id, loan_id, payroll_run_id, amount,
                                        principal_amount, interest_amount)
         VALUES ($1,$2,$3,$4,$4,0)",
    )
    .bind(tenant_id)
    .bind(loan_id)
    .bind(run_id)
    .bind(amount)
    .execute(&mut *tx)
    .await
    .expect("installment");
    sqlx::query("UPDATE employee_loans SET balance = $1, status = $2 WHERE id = $3")
        .bind(balance_after)
        .bind(loan_status)
        .bind(loan_id)
        .execute(&mut *tx)
        .await
        .expect("loan state");
    tx.commit().await.expect("commit seed_run_and_installment");
    run_id
}

#[tokio::test]
async fn partial_payoff_reduces_balance_and_records_payment() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let loan_id = seed_loan(&raw, tenant_id, user_id, entities[0].1, dec!(10_000)).await;

    payoff(
        &pool,
        TenantId(tenant_id),
        user_id,
        loan_id,
        dec!(4_000),
        None,
    )
    .await
    .expect("partial payoff");

    let (balance, status) = loan_state(&raw, tenant_id, loan_id).await;
    assert_eq!(balance, dec!(6_000));
    assert_eq!(status, "active");

    let payments = list_payments(&pool, TenantId(tenant_id), loan_id)
        .await
        .expect("list payments");
    assert_eq!(payments.len(), 1);
    assert_eq!(payments[0].amount, dec!(4_000));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn full_payoff_closes_loan() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let loan_id = seed_loan(&raw, tenant_id, user_id, entities[0].1, dec!(10_000)).await;

    payoff(
        &pool,
        TenantId(tenant_id),
        user_id,
        loan_id,
        dec!(10_000),
        None,
    )
    .await
    .expect("full payoff");

    let (balance, status) = loan_state(&raw, tenant_id, loan_id).await;
    assert_eq!(balance, dec!(0));
    assert_eq!(status, "closed");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn over_balance_is_rejected_and_balance_unchanged() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let loan_id = seed_loan(&raw, tenant_id, user_id, entities[0].1, dec!(10_000)).await;

    let err = payoff(
        &pool,
        TenantId(tenant_id),
        user_id,
        loan_id,
        dec!(10_001),
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, PayoffError::InvalidAmount));

    // Balance must never go negative.
    let (balance, status) = loan_state(&raw, tenant_id, loan_id).await;
    assert_eq!(balance, dec!(10_000));
    assert_eq!(status, "active");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn zero_amount_is_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let loan_id = seed_loan(&raw, tenant_id, user_id, entities[0].1, dec!(10_000)).await;

    let err = payoff(&pool, TenantId(tenant_id), user_id, loan_id, dec!(0), None)
        .await
        .unwrap_err();
    assert!(matches!(err, PayoffError::InvalidAmount));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn closed_loan_rejects_payoff() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let loan_id = seed_loan(&raw, tenant_id, user_id, entities[0].1, dec!(10_000)).await;

    payoff(
        &pool,
        TenantId(tenant_id),
        user_id,
        loan_id,
        dec!(10_000),
        None,
    )
    .await
    .expect("full payoff closes loan");

    let err = payoff(&pool, TenantId(tenant_id), user_id, loan_id, dec!(1), None)
        .await
        .unwrap_err();
    assert!(matches!(err, PayoffError::NotActive));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn tenant_scope_blocks_cross_tenant_payoff() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let _pool_a = TenantPool::connect(&url, 2).await.expect("tenant pool a");
    let pool_b = TenantPool::connect(&url, 2).await.expect("tenant pool b");
    let (tenant_a, entities_a) = seed_tenant_entities(&raw, 1).await;
    let (tenant_b, entities_b) = seed_tenant_entities(&raw, 1).await;
    let user_a = user_id_of(&raw, tenant_a, entities_a[0].1).await;
    let user_b = user_id_of(&raw, tenant_b, entities_b[0].1).await;
    let loan_id = seed_loan(&raw, tenant_a, user_a, entities_a[0].1, dec!(10_000)).await;

    let err = payoff(
        &pool_b,
        TenantId(tenant_b),
        user_b,
        loan_id,
        dec!(1_000),
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, PayoffError::NotFound));

    // Tenant B sees no payments; tenant A's balance is untouched.
    let list_b = list_payments(&pool_b, TenantId(tenant_b), loan_id)
        .await
        .expect("list tenant b");
    assert!(list_b.is_empty());

    let (balance, _) = loan_state(&raw, tenant_a, loan_id).await;
    assert_eq!(balance, dec!(10_000));

    cleanup(&raw, tenant_a).await;
    cleanup(&raw, tenant_b).await;
}

#[tokio::test]
async fn reverse_pending_restores_balance_and_reopens() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let (entity_id, employee_id) = entities[0];
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let loan_id = seed_loan(&raw, tenant_id, user_id, employee_id, dec!(1_000)).await;
    let run_id = seed_run_and_installment(
        &raw,
        tenant_id,
        entity_id,
        user_id,
        loan_id,
        "draft",
        dec!(1_000),
        dec!(0),
        "closed",
    )
    .await;

    reverse_installment(&pool, TenantId(tenant_id), user_id, loan_id, run_id)
        .await
        .expect("reverse pending installment");

    let (balance, status) = loan_state(&raw, tenant_id, loan_id).await;
    assert_eq!(balance, dec!(1_000));
    assert_eq!(status, "active");

    let installments = list_installments(&pool, TenantId(tenant_id), loan_id)
        .await
        .expect("list installments");
    assert_eq!(installments.len(), 1);
    assert!(installments[0].reversed_at.is_some());

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn reverse_approved_run_is_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let (entity_id, employee_id) = entities[0];
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let loan_id = seed_loan(&raw, tenant_id, user_id, employee_id, dec!(10_000)).await;
    let run_id = seed_run_and_installment(
        &raw,
        tenant_id,
        entity_id,
        user_id,
        loan_id,
        "approved",
        dec!(1_000),
        dec!(9_000),
        "active",
    )
    .await;

    let err = reverse_installment(&pool, TenantId(tenant_id), user_id, loan_id, run_id)
        .await
        .unwrap_err();
    assert!(matches!(err, ReverseError::InvalidRunState(_)));

    let (balance, _) = loan_state(&raw, tenant_id, loan_id).await;
    assert_eq!(balance, dec!(9_000));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn duplicate_reversal_is_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let (entity_id, employee_id) = entities[0];
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let loan_id = seed_loan(&raw, tenant_id, user_id, employee_id, dec!(10_000)).await;
    let run_id = seed_run_and_installment(
        &raw,
        tenant_id,
        entity_id,
        user_id,
        loan_id,
        "draft",
        dec!(1_000),
        dec!(9_000),
        "active",
    )
    .await;

    reverse_installment(&pool, TenantId(tenant_id), user_id, loan_id, run_id)
        .await
        .expect("first reversal");

    let err = reverse_installment(&pool, TenantId(tenant_id), user_id, loan_id, run_id)
        .await
        .unwrap_err();
    assert!(matches!(err, ReverseError::AlreadyReversed));

    let (balance, _) = loan_state(&raw, tenant_id, loan_id).await;
    assert_eq!(balance, dec!(10_000));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn reversal_caps_at_principal() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let (entity_id, employee_id) = entities[0];
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let loan_id = seed_loan(&raw, tenant_id, user_id, employee_id, dec!(1_000)).await;
    // Balance 500 + installment 1_000 would exceed principal 1_000; cap at principal.
    let run_id = seed_run_and_installment(
        &raw,
        tenant_id,
        entity_id,
        user_id,
        loan_id,
        "draft",
        dec!(1_000),
        dec!(500),
        "active",
    )
    .await;

    reverse_installment(&pool, TenantId(tenant_id), user_id, loan_id, run_id)
        .await
        .expect("reverse");

    let (balance, status) = loan_state(&raw, tenant_id, loan_id).await;
    assert_eq!(balance, dec!(1_000));
    assert_eq!(status, "active");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn reversal_restores_principal_only_and_writes_off_interest() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let (entity_id, employee_id) = entities[0];
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let loan_id = seed_loan(&raw, tenant_id, user_id, employee_id, dec!(1_000)).await;

    // Installment 1_040 = principal 1_000 + interest 40; loan fully paid off.
    let run_id = Uuid::new_v4();
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        r#"INSERT INTO payroll_runs
             (id, tenant_id, entity_id, period_start, period_end, payment_date, status, created_by)
           VALUES ($1,$2,$3,'2026-10-01','2026-10-31','2026-11-01','draft',$4)"#,
    )
    .bind(run_id)
    .bind(tenant_id)
    .bind(entity_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .expect("run");
    sqlx::query(
        "INSERT INTO loan_installments (tenant_id, loan_id, payroll_run_id, amount,
                                        principal_amount, interest_amount)
         VALUES ($1,$2,$3,1040,1000,40)",
    )
    .bind(tenant_id)
    .bind(loan_id)
    .bind(run_id)
    .execute(&mut *tx)
    .await
    .expect("installment");
    sqlx::query("UPDATE employee_loans SET balance = 0, status = 'closed' WHERE id = $1")
        .bind(loan_id)
        .execute(&mut *tx)
        .await
        .expect("loan state");
    tx.commit().await.expect("commit");

    reverse_installment(&pool, TenantId(tenant_id), user_id, loan_id, run_id)
        .await
        .expect("reverse");

    // Only the principal is restored; the interest is written off.
    let (balance, status) = loan_state(&raw, tenant_id, loan_id).await;
    assert_eq!(balance, dec!(1_000));
    assert_eq!(status, "active");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn tenant_scope_blocks_cross_tenant_reversal() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let _pool_a = TenantPool::connect(&url, 2).await.expect("tenant pool a");
    let pool_b = TenantPool::connect(&url, 2).await.expect("tenant pool b");
    let (tenant_a, entities_a) = seed_tenant_entities(&raw, 1).await;
    let (tenant_b, entities_b) = seed_tenant_entities(&raw, 1).await;
    let user_a = user_id_of(&raw, tenant_a, entities_a[0].1).await;
    let user_b = user_id_of(&raw, tenant_b, entities_b[0].1).await;
    let loan_id = seed_loan(&raw, tenant_a, user_a, entities_a[0].1, dec!(10_000)).await;
    let run_id = seed_run_and_installment(
        &raw,
        tenant_a,
        entities_a[0].0,
        user_a,
        loan_id,
        "draft",
        dec!(1_000),
        dec!(9_000),
        "active",
    )
    .await;

    let err = reverse_installment(&pool_b, TenantId(tenant_b), user_b, loan_id, run_id)
        .await
        .unwrap_err();
    assert!(matches!(err, ReverseError::NotFound));

    let (balance, _) = loan_state(&raw, tenant_a, loan_id).await;
    assert_eq!(balance, dec!(9_000));

    cleanup(&raw, tenant_a).await;
    cleanup(&raw, tenant_b).await;
}

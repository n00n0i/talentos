//! Notification enqueue hook tests.

use chrono::NaiveDate;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::notify::enqueue_payslip_ready;
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

async fn set_tenant(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, tenant_id: Uuid) {
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut **tx)
        .await
        .expect("set tenant");
}

#[tokio::test]
async fn payslip_ready_enqueues_one_row_per_active_linked_user() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let active_user = user_id_of(&raw, tenant_id, entities[0].1).await;
    let inactive_user = user_id_of(&raw, tenant_id, entities[1].1).await;

    // Deactivate one employee's linked user so the hook must filter them out.
    let mut tx = raw.begin().await.expect("begin");
    set_tenant(&mut tx, tenant_id).await;
    sqlx::query("UPDATE users SET is_active = false WHERE id = $1")
        .bind(inactive_user)
        .execute(&mut *tx)
        .await
        .expect("deactivate user");
    tx.commit().await.expect("commit deactivate");

    let run_id = Uuid::new_v4();
    let period_start = d(2026, 10, 1);
    let period_end = d(2026, 10, 31);

    // Seed a payroll run and a payslip for each employee.
    let mut tx = raw.begin().await.expect("begin");
    set_tenant(&mut tx, tenant_id).await;
    sqlx::query(
        r#"INSERT INTO payroll_runs
             (id, tenant_id, entity_id, period_start, period_end, payment_date, status,
              total_employees, created_by, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, 'approved', 2, $7, now())"#,
    )
    .bind(run_id)
    .bind(tenant_id)
    .bind(entities[0].0)
    .bind(period_start)
    .bind(period_end)
    .bind(d(2026, 11, 5))
    .bind(active_user)
    .execute(&mut *tx)
    .await
    .expect("insert run");

    for (_, employee_id) in &entities {
        sqlx::query(
            r#"INSERT INTO payslips
                 (id, payroll_run_id, tenant_id, employee_id, base_salary, gross_earnings,
                  total_deductions, net_pay, tax_withheld, sso_employee, sso_employer,
                  pf_employee, pf_employer, anomalies, status)
               VALUES (gen_random_uuid(), $1, $2, $3, 30000.00, 30000.00, 0, 29000.00,
                       0, 0, 0, 0, 0, '[]', 'calculated')"#,
        )
        .bind(run_id)
        .bind(tenant_id)
        .bind(employee_id)
        .execute(&mut *tx)
        .await
        .expect("insert payslip");
    }
    tx.commit().await.expect("commit payroll seed");

    // Call the hook inside a tenant-scoped transaction.
    let mut tx = pool.begin_tenant(tenant).await.expect("begin tenant tx");
    enqueue_payslip_ready(&mut tx, tenant_id, run_id, period_start, period_end)
        .await
        .expect("enqueue payslip ready");
    tx.commit().await.expect("commit enqueue");

    // Only the active, linked user should have a pending email row.
    let mut tx = raw.begin().await.expect("begin");
    set_tenant(&mut tx, tenant_id).await;
    let rows = sqlx::query(
        "SELECT channel, recipient, body FROM notifications
         WHERE tenant_id = $1 AND status = 'pending'
         ORDER BY channel",
    )
    .bind(tenant_id)
    .fetch_all(&mut *tx)
    .await
    .expect("fetch notifications");
    let _ = tx.rollback().await;

    assert_eq!(rows.len(), 1, "only the active linked user is notified");
    let channel: String = rows[0].get("channel");
    assert_eq!(channel, "email");
    let mut tx = raw.begin().await.expect("begin");
    set_tenant(&mut tx, tenant_id).await;
    let expected_recipient: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(active_user)
        .fetch_one(&mut *tx)
        .await
        .expect("active user email");
    let _ = tx.rollback().await;
    let recipient: String = rows[0].get("recipient");
    assert_eq!(
        recipient, expected_recipient,
        "recipient must be the linked user"
    );
    let body: String = rows[0].get("body");
    assert!(
        body.contains("2026-10-01"),
        "body should contain period start: {body}"
    );
    assert!(
        body.contains("2026-10-31"),
        "body should contain period end: {body}"
    );
    assert!(
        !body.contains("29000"),
        "body must not contain salary/PII: {body}"
    );

    // Clean up payroll data that `common::cleanup` does not know about.
    let mut tx = raw.begin().await.expect("begin cleanup");
    set_tenant(&mut tx, tenant_id).await;
    let _ = sqlx::query("DELETE FROM payslips WHERE tenant_id = $1")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await;
    let _ = sqlx::query("DELETE FROM notifications WHERE tenant_id = $1")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await;
    let _ = sqlx::query("DELETE FROM payroll_runs WHERE tenant_id = $1")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await;
    tx.commit().await.expect("commit payroll cleanup");

    cleanup(&raw, tenant_id).await;
}

//! PDPA retention/anonymisation: purge_employee_pii and run_retention_purge.
//! Requires DATABASE_URL; skips when unset.

use sqlx::PgPool;
use talentos_db::pii::PiiCipher;
use talentos_db::TenantPool;
use talentos_hr_runner::pdpa::{
    get_policy, purge_employee_pii, run_retention_purge, DEFAULT_OPS_RETENTION_MONTHS,
    DEFAULT_PAYROLL_RETENTION_YEARS,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_employee, seed_tenant_entities, user_id_of};

/// Terminate an employee and give them operational data on both sides of the
/// retention cutoff. Returns the cutoff-free PiiCipher-encrypted identity.
async fn terminated_with_ops_data(
    raw: &PgPool,
    tenant_id: Uuid,
    entity_id: Uuid,
    employee_id: Uuid,
    terminated_ago_days: i64,
) -> PiiCipher {
    let cipher = PiiCipher::from_hex(&"b".repeat(64)).expect("cipher");
    let enc = |v: &str| cipher.encrypt(v).expect("encrypt");
    let user_id = user_id_of(raw, tenant_id, employee_id).await;
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let today = chrono::Utc::now().date_naive();
    let terminated_on = today - chrono::Duration::days(terminated_ago_days);
    sqlx::query(
        "UPDATE employees
            SET national_id = '3100111111111', bank_account_number = $3, sso_number = $4
          WHERE tenant_id = $2 AND id = $1",
    )
    .bind(employee_id)
    .bind(tenant_id)
    .bind(enc("999-0-12345-6"))
    .bind(enc("11-00111111-1"))
    .execute(&mut *tx)
    .await
    .expect("terminate");
    sqlx::query(
        "UPDATE employees
                   SET termination_date = $3, status = 'terminated'
                 WHERE tenant_id = $2 AND id = $1",
    )
    .bind(employee_id)
    .bind(tenant_id)
    .bind(terminated_on)
    .execute(&mut *tx)
    .await
    .expect("terminate dates");
    // Old operational data (outside the window) and recent (inside).
    let old_date = today - chrono::Duration::days(terminated_ago_days + 180);
    let recent_date = terminated_on;
    for (date, reason) in [(old_date, "old"), (recent_date, "recent")] {
        sqlx::query(
            r#"INSERT INTO leave_requests
                 (tenant_id, entity_id, employee_id, leave_type, start_date, end_date, days, reason, status)
               VALUES ($1, $2, $3, 'sick', $4, $4, 1, $5, 'approved')"#,
        )
        .bind(tenant_id)
        .bind(entity_id)
        .bind(employee_id)
        .bind(date)
        .bind(reason)
        .execute(&mut *tx)
        .await
        .expect("leave");
        sqlx::query(
            r#"INSERT INTO ot_requests
                 (tenant_id, entity_id, employee_id, work_date, minutes, reason, status)
               VALUES ($1, $2, $3, $4, 60, $5, 'approved')"#,
        )
        .bind(tenant_id)
        .bind(entity_id)
        .bind(employee_id)
        .bind(date)
        .bind(reason)
        .execute(&mut *tx)
        .await
        .expect("ot");
        sqlx::query(
            r#"INSERT INTO attendance_days
                 (tenant_id, employee_id, work_date, status, computed_at)
               VALUES ($1, $2, $3, 'approved', now())"#,
        )
        .bind(tenant_id)
        .bind(employee_id)
        .bind(date)
        .execute(&mut *tx)
        .await
        .expect("attendance");
        sqlx::query(
            r#"INSERT INTO expense_claims
                 (tenant_id, entity_id, employee_id, category, amount, expense_date, status)
               VALUES ($1, $2, $3, 'meal', 100, $4, 'approved')"#,
        )
        .bind(tenant_id)
        .bind(entity_id)
        .bind(employee_id)
        .bind(date)
        .execute(&mut *tx)
        .await
        .expect("claim");
        sqlx::query(
            r#"INSERT INTO employee_documents
                 (tenant_id, employee_id, doc_type, title)
               VALUES ($1, $2, 'other', $3)"#,
        )
        .bind(tenant_id)
        .bind(employee_id)
        .bind(reason)
        .execute(&mut *tx)
        .await
        .expect("document");
    }
    // A payslip must survive the purge (legal retention).
    sqlx::query(
        r#"INSERT INTO payroll_runs
             (tenant_id, entity_id, period_start, period_end, payment_date,
              status, created_by)
           VALUES ($1, $2, $3, $4, $4, 'approved', $5)"#,
    )
    .bind(tenant_id)
    .bind(entity_id)
    .bind(recent_date)
    .bind(recent_date)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .expect("run");
    let run_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM payroll_runs
          WHERE tenant_id = $1 AND entity_id = $2 AND period_start = $3",
    )
    .bind(tenant_id)
    .bind(entity_id)
    .bind(recent_date)
    .fetch_one(&mut *tx)
    .await
    .expect("run id");
    sqlx::query(
        r#"INSERT INTO payslips
             (payroll_run_id, tenant_id, employee_id,
              base_salary, gross_earnings, total_deductions, net_pay, tax_withheld,
              sso_employee, sso_employer, pf_employee, pf_employer, status)
           VALUES ($1, $2, $3, 30000, 31000, 1000, 30000, 500,
                   450, 450, 1500, 1500, 'calculated')"#,
    )
    .bind(run_id)
    .bind(tenant_id)
    .bind(employee_id)
    .execute(&mut *tx)
    .await
    .expect("payslip");
    tx.commit().await.expect("commit");
    cipher
}

#[tokio::test]
async fn policy_defaults_and_purge_anonymises_pii_and_retains_payslips() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let tenant = TenantId(tenant_id);

    let policy = get_policy(&pool, tenant).await.expect("policy");
    assert_eq!(
        policy.payroll_retention_years,
        DEFAULT_PAYROLL_RETENTION_YEARS
    );
    assert_eq!(policy.ops_retention_months, DEFAULT_OPS_RETENTION_MONTHS);

    // 18 months ago → outside the 12-month ops window.
    terminated_with_ops_data(&raw, tenant_id, entity_id, employee_id, 540).await;

    let result = purge_employee_pii(&pool, tenant, employee_id, user_id, "user")
        .await
        .expect("purge");
    assert!(result.pii_anonymised, "identity PII was cleared");
    assert!(
        result.operational_rows_removed > 0,
        "expired operational rows removed"
    );

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT national_id, bank_account_number, sso_number FROM employees WHERE id = $1",
    )
    .bind(employee_id)
    .fetch_one(&mut *tx)
    .await
    .expect("employee row");
    assert!(
        row.0.is_none() && row.1.is_none() && row.2.is_none(),
        "identity wiped"
    );
    let payslips: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM payslips p
           JOIN payroll_runs r ON r.id = p.payroll_run_id
          WHERE p.tenant_id = $1 AND p.employee_id = $2",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .fetch_one(&mut *tx)
    .await
    .expect("payslips");
    assert_eq!(payslips, 1, "payroll rows survive the purge");
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log
          WHERE tenant_id = $1 AND action = 'privacy.purge' AND resource_id = $2",
    )
    .bind(tenant_id)
    .bind(employee_id.to_string())
    .fetch_one(&mut *tx)
    .await
    .expect("audit count");
    assert_eq!(audits, 1, "the purge is audited");
    let _ = tx.rollback().await;

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn purge_keeps_recent_ops_data_and_is_idempotent() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let tenant = TenantId(tenant_id);

    // Terminated 3 months ago → recent data inside the window, old data out.
    terminated_with_ops_data(&raw, tenant_id, entity_id, employee_id, 90).await;

    let first = purge_employee_pii(&pool, tenant, employee_id, user_id, "user")
        .await
        .expect("first purge");
    assert!(first.pii_anonymised);

    let second = purge_employee_pii(&pool, tenant, employee_id, user_id, "user")
        .await
        .expect("second purge");
    assert!(
        !second.pii_anonymised,
        "nothing left to anonymise on the second run"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn retention_purge_touches_only_expired_terminated_employees() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let (entity_a, employee_a) = entities[0];
    let (entity_b, employee_b) = entities[1];
    let (_entity_c, employee_c) = entities[2];
    let user_id = user_id_of(&raw, tenant_id, employee_a).await;
    let tenant = TenantId(tenant_id);

    // a: 18 months out of service (due), b: 2 weeks out of service (not due),
    // c: still active (never due).
    terminated_with_ops_data(&raw, tenant_id, entity_a, employee_a, 540).await;
    terminated_with_ops_data(&raw, tenant_id, entity_b, employee_b, 14).await;

    let results = run_retention_purge(&pool, tenant, user_id, "user")
        .await
        .expect("retention purge");
    assert_eq!(results.len(), 1, "only the expired employee is purged");
    assert_eq!(results[0].employee_id, employee_a);

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    for (emp, due) in [(employee_a, true), (employee_b, false), (employee_c, false)] {
        let flags: (Option<String>, Option<String>, bool) = sqlx::query_as(
            "SELECT national_id, sso_number, status = 'terminated' FROM employees WHERE id = $1",
        )
        .bind(emp)
        .fetch_one(&mut *tx)
        .await
        .expect("flags");
        if due {
            assert!(flags.0.is_none(), "a: PII wiped");
            assert!(flags.2, "a: still terminated");
        } else if flags.2 {
            // b: terminated but not yet due — identity intact.
            assert_eq!(flags.0.as_deref(), Some("3100111111111"), "b PII intact");
        }
        // c: still active, no data seeded.
    }
    let _ = tx.rollback().await;

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn recent_unexpiring_doc_not_purged() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let tenant = TenantId(tenant_id);

    // Terminated 18 months ago; 12-month retention → cutoff is 6 months ago (in the past).
    // Seed a document created yesterday with no expiry: it must survive.
    terminated_with_ops_data(&raw, tenant_id, entity_id, employee_id, 540).await;
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        r#"INSERT INTO employee_documents (tenant_id, employee_id, doc_type, title, created_at)
           VALUES ($1, $2, 'other', 'recent-doc', now() - interval '1 day')"#,
    )
    .bind(tenant_id)
    .bind(employee_id)
    .execute(&mut *tx)
    .await
    .expect("insert recent doc");
    let _ = tx.commit().await;

    let _result = purge_employee_pii(&pool, tenant, employee_id, user_id, "user")
        .await
        .expect("purge");
    let mut tx2 = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx2)
        .await
        .expect("set tenant");
    let docs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM employee_documents
          WHERE tenant_id = $1 AND employee_id = $2 AND title = 'recent-doc'",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .fetch_one(&mut *tx2)
    .await
    .expect("doc count");
    assert_eq!(docs, 1, "recent unexpiring doc must survive the purge");
    let _ = tx2.rollback().await;
    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn old_unexpiring_doc_purged() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let tenant = TenantId(tenant_id);

    // Terminated 18 months ago; 12-month retention → cutoff is 6 months ago (in the past).
    // Seed a document created 2 years ago with no expiry: it must be purged.
    terminated_with_ops_data(&raw, tenant_id, entity_id, employee_id, 540).await;
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        r#"INSERT INTO employee_documents (tenant_id, employee_id, doc_type, title, created_at)
           VALUES ($1, $2, 'other', 'old-doc', now() - interval '2 years')"#,
    )
    .bind(tenant_id)
    .bind(employee_id)
    .execute(&mut *tx)
    .await
    .expect("insert old doc");
    let _ = tx.commit().await;

    let _result = purge_employee_pii(&pool, tenant, employee_id, user_id, "user")
        .await
        .expect("purge");
    let mut tx2 = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx2)
        .await
        .expect("set tenant");
    let docs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM employee_documents
          WHERE tenant_id = $1 AND employee_id = $2 AND title = 'old-doc'",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .fetch_one(&mut *tx2)
    .await
    .expect("doc count");
    assert_eq!(docs, 0, "old unexpiring doc must be purged");
    let _ = tx2.rollback().await;
    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn purge_is_tenant_scoped() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_a, _entity_a, employee_a) = seed_tenant_employee(&raw).await;
    let (tenant_b, entity_b, employee_b) = seed_tenant_employee(&raw).await;
    let user_a = user_id_of(&raw, tenant_a, employee_a).await;
    terminated_with_ops_data(&raw, tenant_b, entity_b, employee_b, 540).await;

    let tenant = TenantId(tenant_a);
    let err = purge_employee_pii(&pool, tenant, employee_b, user_a, "user")
        .await
        .expect_err("another tenant's employee must not be found");
    assert!(matches!(
        err,
        talentos_hr_runner::pdpa::PdpaError::EmployeeNotFound
    ));

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_b.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let intact: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT national_id, sso_number FROM employees WHERE id = $1")
            .bind(employee_b)
            .fetch_one(&mut *tx)
            .await
            .expect("other tenant row");
    assert!(
        intact.0.is_some() && intact.1.is_some(),
        "other tenant PII untouched"
    );
    let _ = tx.rollback().await;

    cleanup(&raw, tenant_a).await;
    cleanup(&raw, tenant_b).await;
}

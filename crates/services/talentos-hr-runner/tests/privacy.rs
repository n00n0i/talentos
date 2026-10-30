//! PDPA consent and personal-data export. Requires DATABASE_URL; skips when unset.

use serde_json::json;
use sqlx::PgPool;
use talentos_db::TenantPool;
use talentos_hr_runner::privacy::{
    accept_privacy, export_personal_data, privacy_status, PrivacyError, NOTICE_VERSION,
};
use talentos_kernel::TenantId;

mod common;
use common::{cleanup, database_url, seed_tenant_employee, seed_tenant_entities, user_id_of};

#[tokio::test]
async fn consent_is_idempotent_and_rejects_unknown_versions() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, _entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let tenant = TenantId(tenant_id);

    let first = accept_privacy(&pool, tenant, user_id, NOTICE_VERSION)
        .await
        .expect("first accept");
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let again = accept_privacy(&pool, tenant, user_id, NOTICE_VERSION)
        .await
        .expect("repeat accept");
    assert_eq!(first, again, "repeat accept must keep the first timestamp");

    let status = privacy_status(&pool, tenant, user_id)
        .await
        .expect("status");
    assert_eq!(status.notice_version, NOTICE_VERSION);
    assert_eq!(status.accepted_at, Some(first));

    let err = accept_privacy(&pool, tenant, user_id, "1999-01-01")
        .await
        .expect_err("unknown version must be rejected");
    assert!(matches!(err, PrivacyError::InvalidVersion));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn export_contains_own_data_only() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let (entity_a, employee_a) = entities[0];
    let (entity_b, employee_b) = entities[1];
    let user_a = user_id_of(&raw, tenant_id, employee_a).await;

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "UPDATE employees SET national_id = '1234567890123', bank_name = 'KBANK',
                bank_account_number = '123-4-56789-0' WHERE id = $1",
    )
    .bind(employee_a)
    .execute(&mut *tx)
    .await
    .expect("profile");
    for (entity, employee, reason) in [
        (entity_a, employee_a, "OWN"),
        (entity_b, employee_b, "COLLEAGUE"),
    ] {
        sqlx::query(
            r#"INSERT INTO leave_requests
                 (tenant_id, entity_id, employee_id, leave_type, start_date, end_date, days, reason, status)
               VALUES ($1, $2, $3, 'annual', '2026-11-02', '2026-11-02', 1, $4, 'pending')"#,
        )
        .bind(tenant_id)
        .bind(entity)
        .bind(employee)
        .bind(reason)
        .execute(&mut *tx)
        .await
        .expect("leave");
    }
    tx.commit().await.expect("commit");

    // Another tenant's data must never appear in the export.
    let (other_tenant, other_entities) = seed_tenant_entities(&raw, 1).await;
    let (other_entity, other_employee) = other_entities[0];
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(other_tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        r#"INSERT INTO leave_requests
             (tenant_id, entity_id, employee_id, leave_type, start_date, end_date, days, reason, status)
           VALUES ($1, $2, $3, 'annual', '2026-11-03', '2026-11-03', 1, 'OTHER-TENANT', 'approved')"#,
    )
    .bind(other_tenant)
    .bind(other_entity)
    .bind(other_employee)
    .execute(&mut *tx)
    .await
    .expect("other leave");
    tx.commit().await.expect("commit");

    let data = export_personal_data(&pool, TenantId(tenant_id), user_a, &None)
        .await
        .expect("export");

    assert_eq!(data["employee"]["employee_code"], "EMP000");
    assert_eq!(data["employee"]["national_id"], "1234567890123");
    assert_eq!(data["employee"]["bank_account_number"], "123-4-56789-0");
    let leave = data["leave_requests"].as_array().expect("leave array");
    assert_eq!(leave.len(), 1, "only the caller's leave is exported");
    assert_eq!(leave[0]["reason"], "OWN");
    assert!(data["payslips"].is_array());
    assert!(
        !data.to_string().contains("OTHER-TENANT"),
        "another tenant's rows must not leak"
    );
    assert!(
        !data.to_string().contains("COLLEAGUE"),
        "a colleague's rows must not leak"
    );

    cleanup(&raw, tenant_id).await;
    cleanup(&raw, other_tenant).await;
}

#[tokio::test]
async fn export_without_employee_returns_account_only() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, _entity_id, employee_id) = seed_tenant_employee(&raw).await;
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;

    // Turn the linked employee into an admin-only account (no employee row).
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("DELETE FROM employees WHERE id = $1")
        .bind(employee_id)
        .execute(&mut *tx)
        .await
        .expect("delete employee");
    tx.commit().await.expect("commit");

    let data = export_personal_data(&pool, TenantId(tenant_id), user_id, &None)
        .await
        .expect("export");
    assert!(data["employee"].is_null());
    assert!(data["account"]["email"]
        .as_str()
        .is_some_and(|e| e.contains("example.com")));
    assert_eq!(data["attendance_days"], json!([]));

    cleanup(&raw, tenant_id).await;
}

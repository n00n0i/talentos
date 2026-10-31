//! PDPA consent and personal-data export. Requires DATABASE_URL; skips when unset.

use std::sync::Arc;

use serde_json::json;
use sqlx::PgPool;
use talentos_db::pii::PiiCipher;
use talentos_db::TenantPool;
use talentos_hr_runner::privacy::{
    accept_privacy, export_personal_data, privacy_status, PrivacyError, NOTICE_VERSION,
};
use talentos_kernel::TenantId;

mod common;
use common::{cleanup, database_url, seed_tenant_employee, seed_tenant_entities, user_id_of};

#[tokio::test]
async fn consent_is_idempotent_versioned_and_per_user() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let user_a = user_id_of(&raw, tenant_id, entities[0].1).await;
    let user_b = user_id_of(&raw, tenant_id, entities[1].1).await;
    let tenant = TenantId(tenant_id);

    let first = accept_privacy(&pool, tenant, user_a, NOTICE_VERSION)
        .await
        .expect("first accept");
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let again = accept_privacy(&pool, tenant, user_a, NOTICE_VERSION)
        .await
        .expect("repeat accept");
    assert_eq!(first, again, "repeat accept must keep the first timestamp");

    let status = privacy_status(&pool, tenant, user_a).await.expect("status");
    assert_eq!(status.notice_version, NOTICE_VERSION);
    assert_eq!(status.accepted_at, Some(first));

    let other = privacy_status(&pool, tenant, user_b)
        .await
        .expect("other status");
    assert_eq!(
        other.accepted_at, None,
        "another user's consent is separate"
    );

    let err = accept_privacy(&pool, tenant, user_a, "1999-01-01")
        .await
        .expect_err("unknown version must be rejected");
    assert!(matches!(err, PrivacyError::InvalidVersion));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn export_contains_all_linked_employees_and_no_one_else() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let (entity_a, employee_a) = entities[0];
    let (entity_b, employee_b) = entities[1];
    let (entity_c, employee_c) = entities[2];
    let user_a = user_id_of(&raw, tenant_id, employee_a).await;

    let cipher = PiiCipher::from_hex(&"a".repeat(64)).expect("cipher");
    let encrypted_bank = cipher.encrypt("123-4-56789-0").expect("encrypt");

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    // national_id stays legacy plaintext; the bank account is encrypted.
    // A terminated employee still has the right to their data.
    sqlx::query(
        "UPDATE employees SET national_id = '1234567890123', bank_name = 'KBANK',
                bank_account_number = $2, status = 'terminated' WHERE id = $1",
    )
    .bind(employee_a)
    .bind(&encrypted_bank)
    .execute(&mut *tx)
    .await
    .expect("profile");
    // The same user is also linked to a second entity's employee row.
    sqlx::query("UPDATE employees SET user_id = $1 WHERE id = $2")
        .bind(user_a)
        .bind(employee_c)
        .execute(&mut *tx)
        .await
        .expect("link second employee");
    for (entity, employee, reason) in [
        (entity_a, employee_a, "OWN"),
        (entity_b, employee_b, "COLLEAGUE"),
        (entity_c, employee_c, "SECOND-ENTITY"),
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
    sqlx::query(
        r#"INSERT INTO employee_training_records
             (tenant_id, employee_id, title, provider, completed_on)
           VALUES ($1, $2, 'ปฐมพยาบาล', 'Academy', '2026-11-01')"#,
    )
    .bind(tenant_id)
    .bind(employee_a)
    .execute(&mut *tx)
    .await
    .expect("training");
    sqlx::query(
        r#"INSERT INTO attendance_events
             (tenant_id, employee_id, kind, source, occurred_at, work_date, distance_m, accuracy_m)
           VALUES ($1, $2, 'in', 'mobile', '2026-11-02T02:00:00Z', '2026-11-02', 5, 10)"#,
    )
    .bind(tenant_id)
    .bind(employee_a)
    .execute(&mut *tx)
    .await
    .expect("punch event");
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

    let data = export_personal_data(&pool, TenantId(tenant_id), user_a, &Some(Arc::new(cipher)))
        .await
        .expect("export");

    let employees = data["employees"].as_array().expect("employees array");
    assert_eq!(employees.len(), 2, "both linked employee rows are exported");
    let codes: Vec<&str> = employees
        .iter()
        .filter_map(|e| e["employee_code"].as_str())
        .collect();
    assert!(codes.contains(&"EMP000"));
    assert!(codes.contains(&"EMP002"));
    let first = employees
        .iter()
        .find(|e| e["employee_code"] == "EMP000")
        .expect("employee A is exported");
    assert_eq!(
        first["national_id"], "1234567890123",
        "legacy plaintext passes"
    );
    assert_eq!(
        first["bank_account_number"], "123-4-56789-0",
        "encrypted PII is decrypted"
    );
    let events = first["attendance_events"].as_array().expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["source"], "mobile");
    assert_eq!(first["training"][0]["title"], "ปฐมพยาบาล");

    let text = data.to_string();
    assert!(
        !text.contains("OTHER-TENANT"),
        "another tenant must not leak"
    );
    assert!(!text.contains("COLLEAGUE"), "a colleague must not leak");

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log
          WHERE tenant_id = $1 AND action = 'privacy.export' AND actor_id = $2",
    )
    .bind(tenant_id)
    .bind(user_a.to_string())
    .fetch_one(&mut *tx)
    .await
    .expect("audit count");
    let _ = tx.rollback().await;
    assert_eq!(audits, 1, "each export is audited");

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
    assert_eq!(data["employees"], json!([]));
    assert!(data["account"]["email"]
        .as_str()
        .is_some_and(|e| e.contains("example.com")));
    assert_eq!(data["notifications"], json!([]));

    cleanup(&raw, tenant_id).await;
}

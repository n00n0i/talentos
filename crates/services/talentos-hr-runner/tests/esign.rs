//! E-signature acknowledgement tests. Requires DATABASE_URL; skips when unset.

use sqlx::PgPool;
use talentos_db::TenantPool;
use talentos_hr_runner::esign::{
    claim_employee_of_payslip, sign_document, DocumentType, EsignError, SignatureView,
};
use talentos_kernel::TenantId;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

async fn raw_pool(url: &str) -> (PgPool, TenantPool) {
    let raw = PgPool::connect(url).await.expect("connect raw");
    let pool = TenantPool::connect(url, 2).await.expect("tenant pool");
    (raw, pool)
}

#[tokio::test]
async fn sign_payslip_stores_hash_and_audit() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let (raw, pool) = raw_pool(&url).await;
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let tenant = TenantId(tenant_id);

    // Create a minimal payroll run + payslip so a payslip id exists.
    let payslip_id = seed_payslip(&raw, tenant_id, entities[0].0, entities[0].1, user_id).await;

    let sig = sign_document(
        &pool,
        tenant,
        DocumentType::Payslip,
        payslip_id,
        user_id,
        "Alice A",
    )
    .await
    .expect("sign");

    assert_eq!(sig.document_type, "payslip");
    assert_eq!(sig.document_id, payslip_id);
    assert_eq!(sig.typed_name, "Alice A");
    assert_eq!(sig.provider, "internal");
    // 64 hex chars.
    assert_eq!(sig.doc_hash_sha256.len(), 64);

    // Audit row was written.
    let audit_count: i64 = {
        let mut tx = raw.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let c: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_log WHERE tenant_id = $1 AND resource_type = 'payslip' AND action LIKE 'document.payslip%'",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await
        .expect("count audit");
        let _ = tx.rollback().await;
        c
    };
    assert!(audit_count >= 1, "audit_log row must be written");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn sign_is_idempotent_per_signer() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let (raw, pool) = raw_pool(&url).await;
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let tenant = TenantId(tenant_id);
    let payslip_id = seed_payslip(&raw, tenant_id, entities[0].0, entities[0].1, user_id).await;

    let first = sign_document(
        &pool,
        tenant,
        DocumentType::Payslip,
        payslip_id,
        user_id,
        "Alice A",
    )
    .await
    .expect("first sign");
    let second = sign_document(
        &pool,
        tenant,
        DocumentType::Payslip,
        payslip_id,
        user_id,
        "Alice A Updated",
    )
    .await
    .expect("second sign");

    // Re-signing returns the same signature row id (the upsert's key).
    assert_eq!(first.id, second.id, "re-sign returns existing signature");
    assert_eq!(second.typed_name, "Alice A Updated");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn sign_payslip_not_owned_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let (raw, pool) = raw_pool(&url).await;
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let tenant = TenantId(tenant_id);
    let payslip_id = seed_payslip(&raw, tenant_id, entities[0].0, entities[0].1, user_id).await;

    // A different user who is not linked to this payslip's employee.
    let other_user: uuid::Uuid = uuid::Uuid::new_v4();
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "INSERT INTO users (id, tenant_id, email, password_hash, is_active)
         VALUES ($1, $2, $3, 'x', true)",
    )
    .bind(other_user)
    .bind(tenant_id)
    .bind(format!(
        "other-{}@example.com",
        &other_user.to_string()[..8]
    ))
    .execute(&mut *tx)
    .await
    .expect("other user");
    tx.commit().await.expect("commit");

    let err = claim_employee_of_payslip(&pool, tenant, payslip_id, other_user)
        .await
        .unwrap_err();
    assert!(matches!(err, EsignError::NotOwnPayslip));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn empty_name_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let (raw, pool) = raw_pool(&url).await;
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let tenant = TenantId(tenant_id);
    let payslip_id = seed_payslip(&raw, tenant_id, entities[0].0, entities[0].1, user_id).await;

    let err = sign_document(
        &pool,
        tenant,
        DocumentType::Payslip,
        payslip_id,
        user_id,
        "  ",
    )
    .await
    .unwrap_err();
    assert!(matches!(err, EsignError::InvalidName));

    cleanup(&raw, tenant_id).await;
}

/// Seed a payroll run and one payslip for the employee; returns the payslip id.
async fn seed_payslip(
    raw: &PgPool,
    tenant_id: uuid::Uuid,
    entity_id: uuid::Uuid,
    employee_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> uuid::Uuid {
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let run_id = uuid::Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO payroll_runs
             (id, tenant_id, entity_id, period_start, period_end, payment_date, status, created_by)
           VALUES ($1, $2, $3, '2026-01-01', '2026-01-31', '2026-02-01', 'approved', $4)"#,
    )
    .bind(run_id)
    .bind(tenant_id)
    .bind(entity_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .expect("run");
    let payslip_id = uuid::Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO payslips
             (id, payroll_run_id, tenant_id, employee_id, base_salary, gross_earnings,
              total_deductions, net_pay, status)
           VALUES ($1, $2, $3, $4, 30000, 30000, 0, 30000, 'approved')"#,
    )
    .bind(payslip_id)
    .bind(run_id)
    .bind(tenant_id)
    .bind(employee_id)
    .execute(&mut *tx)
    .await
    .expect("payslip");
    tx.commit().await.expect("commit seed payslip");
    payslip_id
}

#[allow(dead_code)]
fn _sig_view(_v: &SignatureView) {}

//! Approval SLA escalation tests.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::escalate_overdue;
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{
    cleanup, database_url, ensure_role, seed_tenant_entities, set_line_user_id, user_id_of,
};

async fn set_tenant_guc(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, tenant_id: Uuid) {
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut **tx)
        .await
        .expect("set tenant");
}

async fn set_sla_hours(pool: &PgPool, tenant_id: Uuid, hours: i32) {
    let mut tx = pool.begin().await.expect("begin");
    set_tenant_guc(&mut tx, tenant_id).await;
    sqlx::query(
        r#"INSERT INTO tenant_payroll_settings
             (tenant_id, unpaid_leave_divisor, ot_weekday_rate, ot_weekend_rate,
              ot_holiday_rate, leave_approval_chain, approval_sla_hours)
           VALUES ($1, 30, 1.50, 2.00, 3.00, '{hr_admin}', $2)
           ON CONFLICT (tenant_id) DO UPDATE SET approval_sla_hours = EXCLUDED.approval_sla_hours"#,
    )
    .bind(tenant_id)
    .bind(hours)
    .execute(&mut *tx)
    .await
    .expect("set sla");
    tx.commit().await.expect("commit sla");
}

/// Insert a leave request + one approval step with `created_at` backdated by
/// `hours_ago`. Returns (request_id, step_id).
async fn seed_step(
    pool: &PgPool,
    tenant_id: Uuid,
    entity_id: Uuid,
    employee_id: Uuid,
    approver_role: &str,
    hours_ago: i32,
) -> (Uuid, Uuid) {
    let request_id = Uuid::new_v4();
    let step_id = Uuid::new_v4();
    let mut tx = pool.begin().await.expect("begin");
    set_tenant_guc(&mut tx, tenant_id).await;
    sqlx::query(
        r#"INSERT INTO leave_requests
             (id, tenant_id, entity_id, employee_id, leave_type, start_date, end_date, days, status)
           VALUES ($1,$2,$3,$4,'annual','2026-12-01','2026-12-02',2,'pending')"#,
    )
    .bind(request_id)
    .bind(tenant_id)
    .bind(entity_id)
    .bind(employee_id)
    .execute(&mut *tx)
    .await
    .expect("insert request");
    sqlx::query(
        r#"INSERT INTO leave_approval_steps
             (id, tenant_id, leave_request_id, step_no, approver_role, status, created_at)
           VALUES ($1,$2,$3,1,$4,'pending', now() - make_interval(hours => $5))"#,
    )
    .bind(step_id)
    .bind(tenant_id)
    .bind(request_id)
    .bind(approver_role)
    .bind(hours_ago)
    .execute(&mut *tx)
    .await
    .expect("insert step");
    tx.commit().await.expect("commit seed");
    (request_id, step_id)
}

async fn escalated_at(pool: &PgPool, tenant_id: Uuid, step_id: Uuid) -> Option<String> {
    let mut tx = pool.begin().await.expect("begin");
    set_tenant_guc(&mut tx, tenant_id).await;
    let v: Option<String> =
        sqlx::query_scalar("SELECT escalated_at::text FROM leave_approval_steps WHERE id = $1")
            .bind(step_id)
            .fetch_one(&mut *tx)
            .await
            .expect("fetch escalated_at");
    let _ = tx.rollback().await;
    v
}

async fn notification_rows(pool: &PgPool, tenant_id: Uuid) -> Vec<(String, String)> {
    let mut tx = pool.begin().await.expect("begin");
    set_tenant_guc(&mut tx, tenant_id).await;
    let rows = sqlx::query(
        "SELECT channel, recipient FROM notifications WHERE tenant_id = $1 ORDER BY channel, recipient",
    )
    .bind(tenant_id)
    .fetch_all(&mut *tx)
    .await
    .expect("fetch notifications");
    let _ = tx.rollback().await;
    rows.into_iter()
        .map(|r| (r.get("channel"), r.get("recipient")))
        .collect()
}

#[tokio::test]
async fn overdue_step_notifies_each_active_hr_admin_and_stamps_escalated_at() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let tenant = TenantId(tenant_id);

    // Two active HR admins: one with LINE, one without.
    let hr1 = user_id_of(&raw, tenant_id, entities[0].1).await;
    let hr2 = user_id_of(&raw, tenant_id, entities[1].1).await;
    ensure_role(&raw, tenant_id, "hr_admin", hr1).await;
    ensure_role(&raw, tenant_id, "hr_admin", hr2).await;
    set_line_user_id(&raw, tenant_id, hr1, "U_LINE_1").await;

    set_sla_hours(&raw, tenant_id, 48).await;
    let (_req, step_id) = seed_step(
        &raw,
        tenant_id,
        entities[2].0,
        entities[2].1,
        "hr_admin",
        72,
    )
    .await;

    let n = escalate_overdue(&pool, tenant, 100)
        .await
        .expect("escalate");
    assert_eq!(n, 1, "one overdue step escalated");
    assert!(escalated_at(&raw, tenant_id, step_id).await.is_some());

    let rows = notification_rows(&raw, tenant_id).await;
    // email for both admins + LINE for the one with a line id = 3 rows.
    assert_eq!(rows.len(), 3, "expected email x2 + line x1: {rows:?}");
    let emails = rows.iter().filter(|(c, _)| c == "email").count();
    let lines = rows.iter().filter(|(c, _)| c == "line").count();
    assert_eq!(emails, 2);
    assert_eq!(lines, 1);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn second_run_does_not_duplicate() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let hr = user_id_of(&raw, tenant_id, entities[0].1).await;
    ensure_role(&raw, tenant_id, "hr_admin", hr).await;
    set_sla_hours(&raw, tenant_id, 48).await;
    seed_step(
        &raw,
        tenant_id,
        entities[1].0,
        entities[1].1,
        "hr_admin",
        72,
    )
    .await;

    assert_eq!(escalate_overdue(&pool, tenant, 100).await.unwrap(), 1);
    assert_eq!(
        escalate_overdue(&pool, tenant, 100).await.unwrap(),
        0,
        "second sweep escalates nothing new"
    );
    assert_eq!(notification_rows(&raw, tenant_id).await.len(), 1);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn fresh_and_decided_steps_are_ignored() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let tenant = TenantId(tenant_id);

    let hr = user_id_of(&raw, tenant_id, entities[0].1).await;
    ensure_role(&raw, tenant_id, "hr_admin", hr).await;
    set_sla_hours(&raw, tenant_id, 48).await;

    // Fresh step (0 hours old) and a decided step (backdated but approved).
    seed_step(&raw, tenant_id, entities[1].0, entities[1].1, "hr_admin", 0).await;
    let (_req, decided_step) = seed_step(
        &raw,
        tenant_id,
        entities[2].0,
        entities[2].1,
        "hr_admin",
        72,
    )
    .await;
    let mut tx = raw.begin().await.expect("begin");
    set_tenant_guc(&mut tx, tenant_id).await;
    sqlx::query("UPDATE leave_approval_steps SET status = 'approved' WHERE id = $1")
        .bind(decided_step)
        .execute(&mut *tx)
        .await
        .expect("decide step");
    tx.commit().await.expect("commit decide");

    assert_eq!(escalate_overdue(&pool, tenant, 100).await.unwrap(), 0);
    assert!(notification_rows(&raw, tenant_id).await.is_empty());

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn sla_zero_disables_escalation() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let hr = user_id_of(&raw, tenant_id, entities[0].1).await;
    ensure_role(&raw, tenant_id, "hr_admin", hr).await;
    set_sla_hours(&raw, tenant_id, 0).await;
    seed_step(
        &raw,
        tenant_id,
        entities[1].0,
        entities[1].1,
        "hr_admin",
        72,
    )
    .await;

    assert_eq!(escalate_overdue(&pool, tenant, 100).await.unwrap(), 0);
    assert!(notification_rows(&raw, tenant_id).await.is_empty());

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn escalation_is_tenant_scoped() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");

    let (a_id, a_entities) = seed_tenant_entities(&raw, 2).await;
    let (b_id, b_entities) = seed_tenant_entities(&raw, 2).await;

    let a_hr = user_id_of(&raw, a_id, a_entities[0].1).await;
    let b_hr = user_id_of(&raw, b_id, b_entities[0].1).await;
    ensure_role(&raw, a_id, "hr_admin", a_hr).await;
    ensure_role(&raw, b_id, "hr_admin", b_hr).await;

    set_sla_hours(&raw, a_id, 48).await;
    set_sla_hours(&raw, b_id, 48).await;
    seed_step(&raw, a_id, a_entities[1].0, a_entities[1].1, "hr_admin", 72).await;
    seed_step(&raw, b_id, b_entities[1].0, b_entities[1].1, "hr_admin", 72).await;

    // Escalate only tenant A.
    let n = escalate_overdue(&pool, TenantId(a_id), 100).await.unwrap();
    assert_eq!(n, 1);
    assert_eq!(notification_rows(&raw, a_id).await.len(), 1);
    assert!(notification_rows(&raw, b_id).await.is_empty());

    cleanup(&raw, a_id).await;
    cleanup(&raw, b_id).await;
}

#[tokio::test]
async fn sla_out_of_range_is_rejected_by_db_check() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let (tenant_id, _) = seed_tenant_entities(&raw, 1).await;

    let mut tx = raw.begin().await.expect("begin");
    set_tenant_guc(&mut tx, tenant_id).await;
    let res = sqlx::query(
        r#"INSERT INTO tenant_payroll_settings
             (tenant_id, unpaid_leave_divisor, ot_weekday_rate, ot_weekend_rate,
              ot_holiday_rate, leave_approval_chain, approval_sla_hours)
           VALUES ($1, 30, 1.50, 2.00, 3.00, '{hr_admin}', 721)"#,
    )
    .bind(tenant_id)
    .execute(&mut *tx)
    .await;
    let _ = tx.rollback().await;
    assert!(
        res.is_err(),
        "approval_sla_hours 721 must violate the CHECK"
    );

    cleanup(&raw, tenant_id).await;
}

//! Multi-level OT approval chain tests.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::NaiveDate;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::ot::{
    cancel_request, create_request, decide_request, pending_approvals, OtDecideOutcome, OtError,
    OtInput,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

async fn set_ot_chain(pool: &PgPool, tenant_id: Uuid, chain: &[String]) {
    let mut tx = pool.begin().await.expect("begin chain");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        r#"INSERT INTO tenant_payroll_settings
             (tenant_id, unpaid_leave_divisor, ot_weekday_rate, ot_weekend_rate,
              ot_holiday_rate, ot_approval_chain)
           VALUES ($1, 30, 1.50, 2.00, 3.00, $2)
           ON CONFLICT (tenant_id) DO UPDATE SET ot_approval_chain = EXCLUDED.ot_approval_chain"#,
    )
    .bind(tenant_id)
    .bind(chain)
    .execute(&mut *tx)
    .await
    .expect("set ot chain");
    tx.commit().await.expect("commit chain");
}

async fn step_statuses(pool: &PgPool, tenant_id: Uuid, request_id: Uuid) -> Vec<(i32, String)> {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let rows = sqlx::query(
        "SELECT step_no, status FROM ot_approval_steps
         WHERE ot_request_id = $1 ORDER BY step_no",
    )
    .bind(request_id)
    .fetch_all(&mut *tx)
    .await
    .expect("fetch steps");
    let _ = tx.rollback().await;
    rows.into_iter()
        .map(|r| (r.get("step_no"), r.get("status")))
        .collect()
}

async fn request_status(pool: &PgPool, tenant_id: Uuid, request_id: Uuid) -> String {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let status: String = sqlx::query_scalar("SELECT status FROM ot_requests WHERE id = $1")
        .bind(request_id)
        .fetch_one(&mut *tx)
        .await
        .expect("fetch status");
    let _ = tx.rollback().await;
    status
}

#[tokio::test]
async fn default_chain_is_single_hr_step() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let approver = user_id_of(&raw, tenant_id, entities[0].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[1].1).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        OtInput {
            work_date: d(2026, 12, 1),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create request");

    let steps = step_statuses(&raw, tenant_id, req.id).await;
    assert_eq!(steps, vec![(1, "pending".into())]);

    let outcome = decide_request(&pool, tenant, approver, &["hr_admin".into()], req.id, true)
        .await
        .expect("approve");
    assert_eq!(
        outcome,
        OtDecideOutcome {
            status: "approved".into(),
            step: 1,
            total_steps: 1,
        }
    );
    assert_eq!(request_status(&raw, tenant_id, req.id).await, "approved");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn two_step_chain_advances_then_approves() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let tenant = TenantId(tenant_id);

    let manager = user_id_of(&raw, tenant_id, entities[0].1).await;
    let hr = user_id_of(&raw, tenant_id, entities[1].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[2].1).await;

    set_ot_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        OtInput {
            work_date: d(2026, 12, 6),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create request");

    let steps = step_statuses(&raw, tenant_id, req.id).await;
    assert_eq!(steps, vec![(1, "pending".into()), (2, "pending".into())]);

    let first = decide_request(&pool, tenant, manager, &["manager".into()], req.id, true)
        .await
        .expect("manager approves");
    assert_eq!(
        first,
        OtDecideOutcome {
            status: "pending".into(),
            step: 2,
            total_steps: 2,
        }
    );
    assert_eq!(request_status(&raw, tenant_id, req.id).await, "pending");

    let second = decide_request(&pool, tenant, hr, &["hr_admin".into()], req.id, true)
        .await
        .expect("hr approves");
    assert_eq!(
        second,
        OtDecideOutcome {
            status: "approved".into(),
            step: 2,
            total_steps: 2,
        }
    );
    assert_eq!(request_status(&raw, tenant_id, req.id).await, "approved");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn manager_cannot_decide_hr_step() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let tenant = TenantId(tenant_id);

    let manager = user_id_of(&raw, tenant_id, entities[0].1).await;
    let other_manager = user_id_of(&raw, tenant_id, entities[1].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[2].1).await;

    set_ot_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        OtInput {
            work_date: d(2026, 12, 8),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create request");

    decide_request(&pool, tenant, manager, &["manager".into()], req.id, true)
        .await
        .expect("manager approves step 1");

    let err = decide_request(
        &pool,
        tenant,
        other_manager,
        &["manager".into()],
        req.id,
        true,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, OtError::Forbidden));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn hr_can_override_manager_step() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let hr = user_id_of(&raw, tenant_id, entities[0].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[1].1).await;

    set_ot_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        OtInput {
            work_date: d(2026, 12, 9),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create request");

    let first = decide_request(&pool, tenant, hr, &["hr_admin".into()], req.id, true)
        .await
        .expect("hr overrides step 1");
    assert_eq!(
        first,
        OtDecideOutcome {
            status: "pending".into(),
            step: 2,
            total_steps: 2,
        }
    );

    let second = decide_request(&pool, tenant, hr, &["hr_admin".into()], req.id, true)
        .await
        .expect("hr approves step 2");
    assert_eq!(second.status, "approved");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn reject_at_step_one_skips_remaining_steps() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let manager = user_id_of(&raw, tenant_id, entities[0].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[1].1).await;

    set_ot_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        OtInput {
            work_date: d(2026, 12, 13),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create request");

    let outcome = decide_request(&pool, tenant, manager, &["manager".into()], req.id, false)
        .await
        .expect("manager rejects");
    assert_eq!(outcome.status, "rejected");

    assert_eq!(request_status(&raw, tenant_id, req.id).await, "rejected");
    let steps = step_statuses(&raw, tenant_id, req.id).await;
    assert_eq!(steps, vec![(1, "rejected".into()), (2, "skipped".into())]);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn cancel_skips_pending_steps() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let requester = user_id_of(&raw, tenant_id, entities[0].1).await;

    set_ot_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        OtInput {
            work_date: d(2026, 12, 15),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create request");

    cancel_request(&pool, tenant, requester, req.id)
        .await
        .expect("cancel");

    let steps = step_statuses(&raw, tenant_id, req.id).await;
    assert_eq!(steps, vec![(1, "skipped".into()), (2, "skipped".into())]);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn request_uses_chain_snapshot_from_creation() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let hr = user_id_of(&raw, tenant_id, entities[0].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[1].1).await;

    set_ot_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        OtInput {
            work_date: d(2026, 12, 16),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create request");

    // Change tenant chain after the request was created.
    set_ot_chain(&raw, tenant_id, &["hr_admin".into()]).await;

    // The request still has two steps and HR can override/approve both.
    let first = decide_request(&pool, tenant, hr, &["hr_admin".into()], req.id, true)
        .await
        .expect("hr approves snapshot step 1");
    assert_eq!(first.status, "pending");
    assert_eq!(first.total_steps, 2);

    let second = decide_request(&pool, tenant, hr, &["hr_admin".into()], req.id, true)
        .await
        .expect("hr approves snapshot step 2");
    assert_eq!(second.status, "approved");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn legacy_pending_request_approves_via_single_hr_step() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let hr = user_id_of(&raw, tenant_id, entities[0].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[1].1).await;

    let request_id = Uuid::new_v4();
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        r#"INSERT INTO ot_requests
             (id, tenant_id, entity_id, employee_id, work_date, minutes, reason, status, requested_by)
           VALUES ($1,$2,$3,$4,$5,60,'legacy','pending',$6)"#,
    )
    .bind(request_id)
    .bind(tenant_id)
    .bind(entities[0].0)
    .bind(entities[1].1)
    .bind(d(2026, 12, 20))
    .bind(requester)
    .execute(&mut *tx)
    .await
    .expect("insert legacy request");
    tx.commit().await.expect("commit");

    let outcome = decide_request(&pool, tenant, hr, &["hr_admin".into()], request_id, true)
        .await
        .expect("approve legacy");
    assert_eq!(outcome.status, "approved");
    assert_eq!(outcome.total_steps, 1);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn pending_approvals_shows_current_step() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let requester = user_id_of(&raw, tenant_id, entities[1].1).await;

    set_ot_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        OtInput {
            work_date: d(2026, 12, 22),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create request");

    let inbox = pending_approvals(&pool, tenant).await.expect("inbox");
    let item = inbox.iter().find(|i| i.id == req.id).expect("in item");
    assert_eq!(item.step_no, 1);
    assert_eq!(item.total_steps, 2);
    assert_eq!(item.approver_role, "manager");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn legacy_pending_request_shows_default_step() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let request_id = Uuid::new_v4();
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        r#"INSERT INTO ot_requests
             (id, tenant_id, entity_id, employee_id, work_date, minutes, reason, status, requested_by)
           VALUES ($1,$2,$3,$4,$5,60,'legacy','pending',NULL)"#,
    )
    .bind(request_id)
    .bind(tenant_id)
    .bind(entities[0].0)
    .bind(entities[0].1)
    .bind(d(2026, 12, 23))
    .execute(&mut *tx)
    .await
    .expect("insert legacy request");
    tx.commit().await.expect("commit");

    let inbox = pending_approvals(&pool, tenant).await.expect("inbox");
    let item = inbox.iter().find(|i| i.id == request_id).expect("in item");
    assert_eq!(item.step_no, 1);
    assert_eq!(item.total_steps, 1);
    assert_eq!(item.approver_role, "hr_admin");

    cleanup(&raw, tenant_id).await;
}

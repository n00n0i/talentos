//! Multi-level leave approval chain tests.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::leave::{
    cancel_request, create_request, decide_request, list_requests, pending_approvals,
    DecideOutcome, LeaveError, LeaveInput,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{
    cleanup, database_url, ensure_role, seed_tenant_entities, set_payroll_chain, user_id_of,
};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

async fn leave_type_id(pool: &PgPool, tenant_id: Uuid, name: &str) -> i32 {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let id: i32 = sqlx::query_scalar("SELECT id FROM leave_types WHERE name = $1")
        .bind(name)
        .fetch_one(&mut *tx)
        .await
        .expect("leave type");
    let _ = tx.rollback().await;
    id
}

async fn seed_balance(
    pool: &PgPool,
    tenant_id: Uuid,
    employee_id: Uuid,
    leave_type_id: i32,
    year: i32,
    total_days: Decimal,
) {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "INSERT INTO leave_balances (tenant_id, employee_id, leave_type_id, year, total_days, used_days)
         VALUES ($1, $2, $3, $4, $5, 0)",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .bind(leave_type_id)
    .bind(year)
    .bind(total_days)
    .execute(&mut *tx)
    .await
    .expect("seed balance");
    tx.commit().await.expect("commit seed");
}

async fn step_statuses(pool: &PgPool, tenant_id: Uuid, request_id: Uuid) -> Vec<(i32, String)> {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let rows = sqlx::query(
        "SELECT step_no, status FROM leave_approval_steps
         WHERE leave_request_id = $1 ORDER BY step_no",
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
    let status: String = sqlx::query_scalar("SELECT status FROM leave_requests WHERE id = $1")
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
    ensure_role(&raw, tenant_id, "hr_admin", approver).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 12, 1),
            end_date: d(2026, 12, 2),
            reason: None,
            employee_id: None,
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
        DecideOutcome {
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
    ensure_role(&raw, tenant_id, "manager", manager).await;
    ensure_role(&raw, tenant_id, "hr_admin", hr).await;

    set_payroll_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 12, 6),
            end_date: d(2026, 12, 7),
            reason: None,
            employee_id: None,
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
        DecideOutcome {
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
        DecideOutcome {
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
    ensure_role(&raw, tenant_id, "manager", manager).await;
    ensure_role(&raw, tenant_id, "manager", other_manager).await;

    set_payroll_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "personal".into(),
            start_date: d(2026, 12, 8),
            end_date: d(2026, 12, 8),
            reason: None,
            employee_id: None,
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
    assert!(matches!(err, LeaveError::Forbidden));

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
    ensure_role(&raw, tenant_id, "hr_admin", hr).await;

    set_payroll_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "sick".into(),
            start_date: d(2026, 12, 9),
            end_date: d(2026, 12, 10),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    let first = decide_request(&pool, tenant, hr, &["hr_admin".into()], req.id, true)
        .await
        .expect("hr overrides step 1");
    assert_eq!(
        first,
        DecideOutcome {
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
    let year = 2026;
    let annual_id = leave_type_id(&raw, tenant_id, "annual").await;

    let manager = user_id_of(&raw, tenant_id, entities[0].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[1].1).await;
    ensure_role(&raw, tenant_id, "manager", manager).await;
    seed_balance(&raw, tenant_id, entities[1].1, annual_id, year, dec!(6)).await;

    set_payroll_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 12, 13),
            end_date: d(2026, 12, 14),
            reason: None,
            employee_id: None,
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

    set_payroll_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "personal".into(),
            start_date: d(2026, 12, 15),
            end_date: d(2026, 12, 15),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    let status = cancel_request(&pool, tenant, requester, req.id)
        .await
        .expect("cancel");
    assert_eq!(status, "cancelled");

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
    ensure_role(&raw, tenant_id, "hr_admin", hr).await;

    set_payroll_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "sick".into(),
            start_date: d(2026, 12, 16),
            end_date: d(2026, 12, 17),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    // Change tenant chain after the request was created.
    set_payroll_chain(&raw, tenant_id, &["hr_admin".into()]).await;

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
    ensure_role(&raw, tenant_id, "hr_admin", hr).await;

    let request_id = Uuid::new_v4();
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        r#"INSERT INTO leave_requests
             (id, tenant_id, entity_id, employee_id, leave_type, start_date, end_date, days,
              reason, status, requested_by, balance_debited)
           VALUES ($1,$2,$3,$4,'annual',$5,$6,2,'legacy','pending',$7,FALSE)"#,
    )
    .bind(request_id)
    .bind(tenant_id)
    .bind(entities[0].0)
    .bind(entities[1].1)
    .bind(d(2026, 12, 20))
    .bind(d(2026, 12, 21))
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

    set_payroll_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 12, 22),
            end_date: d(2026, 12, 23),
            reason: None,
            employee_id: None,
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
async fn employee_without_role_cannot_approve() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let employee = user_id_of(&raw, tenant_id, entities[0].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[1].1).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "personal".into(),
            start_date: d(2026, 12, 24),
            end_date: d(2026, 12, 24),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    let err = decide_request(&pool, tenant, employee, &["employee".into()], req.id, true)
        .await
        .unwrap_err();
    assert!(matches!(err, LeaveError::Forbidden));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn final_reject_restores_balance_once() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);
    let year = 2026;
    let annual_id = leave_type_id(&raw, tenant_id, "annual").await;

    let hr = user_id_of(&raw, tenant_id, entities[0].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[1].1).await;
    ensure_role(&raw, tenant_id, "hr_admin", hr).await;
    seed_balance(&raw, tenant_id, entities[1].1, annual_id, year, dec!(6)).await;

    set_payroll_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 12, 27),
            end_date: d(2026, 12, 28),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    // Balance was debited on create.
    let used_before = {
        let mut tx = raw.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let used: Decimal = sqlx::query_scalar(
            "SELECT used_days FROM leave_balances WHERE employee_id = $1 AND leave_type_id = $2 AND year = $3",
        )
        .bind(entities[1].1)
        .bind(annual_id)
        .bind(year)
        .fetch_one(&mut *tx)
        .await
        .expect("used days");
        let _ = tx.rollback().await;
        used
    };
    assert_eq!(used_before, dec!(2));

    // HR overrides and rejects at step 1 (final reject).
    let outcome = decide_request(&pool, tenant, hr, &["hr_admin".into()], req.id, false)
        .await
        .expect("hr rejects");
    assert_eq!(outcome.status, "rejected");

    let used_after = {
        let mut tx = raw.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let used: Decimal = sqlx::query_scalar(
            "SELECT used_days FROM leave_balances WHERE employee_id = $1 AND leave_type_id = $2 AND year = $3",
        )
        .bind(entities[1].1)
        .bind(annual_id)
        .bind(year)
        .fetch_one(&mut *tx)
        .await
        .expect("used days");
        let _ = tx.rollback().await;
        used
    };
    assert_eq!(used_after, dec!(0));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn intermediate_approval_does_not_change_balance() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let tenant = TenantId(tenant_id);
    let year = 2026;
    let annual_id = leave_type_id(&raw, tenant_id, "annual").await;

    let manager = user_id_of(&raw, tenant_id, entities[0].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[2].1).await;
    ensure_role(&raw, tenant_id, "manager", manager).await;
    seed_balance(&raw, tenant_id, entities[2].1, annual_id, year, dec!(6)).await;

    set_payroll_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 12, 29),
            end_date: d(2026, 12, 30),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    decide_request(&pool, tenant, manager, &["manager".into()], req.id, true)
        .await
        .expect("manager approves");

    let used = {
        let mut tx = raw.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let used: Decimal = sqlx::query_scalar(
            "SELECT used_days FROM leave_balances WHERE employee_id = $1 AND leave_type_id = $2 AND year = $3",
        )
        .bind(entities[2].1)
        .bind(annual_id)
        .bind(year)
        .fetch_one(&mut *tx)
        .await
        .expect("used days");
        let _ = tx.rollback().await;
        used
    };
    assert_eq!(used, dec!(2));

    let requests = list_requests(&pool, tenant, entities[2].1, None)
        .await
        .expect("list requests");
    assert_eq!(requests[0].status, "pending");

    cleanup(&raw, tenant_id).await;
}

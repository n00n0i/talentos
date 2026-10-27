//! Self-service leave + HR pending inbox.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::leave::{
    cancel_request, create_request, decide_request, employee_of_user, list_balances, list_requests,
    pending_approvals, LeaveInput,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{
    cleanup, database_url, ensure_role, seed_tenant_entities, set_line_user_id, user_id_of,
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

async fn pending_notifications(
    pool: &PgPool,
    tenant_id: Uuid,
    channel: &str,
) -> Vec<(String, String, String)> {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let rows = sqlx::query(
        "SELECT recipient, subject, body FROM notifications
         WHERE tenant_id = $1 AND status = 'pending' AND channel = $2
         ORDER BY created_at",
    )
    .bind(tenant_id)
    .bind(channel)
    .fetch_all(&mut *tx)
    .await
    .expect("fetch notifications");
    let _ = tx.rollback().await;
    rows.into_iter()
        .map(|r| (r.get("recipient"), r.get("subject"), r.get("body")))
        .collect()
}

async fn used_days(
    pool: &PgPool,
    tenant_id: Uuid,
    employee_id: Uuid,
    leave_type_id: i32,
    year: i32,
) -> Decimal {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let used: Decimal = sqlx::query_scalar(
        "SELECT used_days FROM leave_balances WHERE employee_id = $1 AND leave_type_id = $2 AND year = $3",
    )
    .bind(employee_id)
    .bind(leave_type_id)
    .bind(year)
    .fetch_one(&mut *tx)
    .await
    .expect("used days");
    let _ = tx.rollback().await;
    used
}

#[tokio::test]
async fn linked_user_sees_only_own_balances_and_requests() {
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

    for (_entity_id, employee_id) in entities.iter() {
        seed_balance(&raw, tenant_id, *employee_id, annual_id, year, dec!(6)).await;
    }

    let mut users = Vec::new();
    for (_entity_id, employee_id) in entities.iter() {
        let user_id = user_id_of(&raw, tenant_id, *employee_id).await;
        users.push((user_id, *employee_id));
    }

    let req = create_request(
        &pool,
        tenant,
        users[0].0,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 10, 5),
            end_date: d(2026, 10, 6),
            reason: Some("trip".into()),
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    let my_emp = employee_of_user(&pool, tenant, users[0].0)
        .await
        .expect("resolve employee")
        .expect("employee exists");
    let balances = list_balances(&pool, tenant, my_emp, year)
        .await
        .expect("balances");
    let annual = balances.iter().find(|b| b.leave_type == "annual").unwrap();
    assert!(annual.provisioned);
    assert_eq!(annual.used_days, dec!(2));
    assert_eq!(annual.remaining_days, dec!(4));

    let requests = list_requests(&pool, tenant, my_emp, None)
        .await
        .expect("requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].id, req.id);

    let other_emp = employee_of_user(&pool, tenant, users[1].0)
        .await
        .expect("resolve employee")
        .expect("employee exists");
    let other_balances = list_balances(&pool, tenant, other_emp, year)
        .await
        .expect("balances");
    let other_annual = other_balances
        .iter()
        .find(|b| b.leave_type == "annual")
        .unwrap();
    assert!(other_annual.provisioned);
    assert_eq!(other_annual.used_days, dec!(0));
    assert_eq!(other_annual.remaining_days, dec!(6));

    let other_requests = list_requests(&pool, tenant, other_emp, None)
        .await
        .expect("requests");
    assert!(other_requests.is_empty());

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn unprovisioned_balance_shows_default_total_and_not_provisioned() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let year = 2026;

    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    // No leave_balances row seeded: the view should report the default total
    // from leave_types and provisioned=false.
    let balances = list_balances(&pool, tenant, entities[0].1, year)
        .await
        .expect("balances");
    let annual = balances.iter().find(|b| b.leave_type == "annual").unwrap();
    assert!(!annual.provisioned);
    assert_eq!(annual.total_days, dec!(6));
    assert_eq!(annual.used_days, dec!(0));
    assert_eq!(annual.remaining_days, dec!(6));

    // Creating a request is still allowed and does not debit (no balance row).
    create_request(
        &pool,
        tenant,
        user_id,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 10, 5),
            end_date: d(2026, 10, 6),
            reason: Some("unprovisioned".into()),
            employee_id: None,
        },
    )
    .await
    .expect("create request without balance row");

    let balances_after = list_balances(&pool, tenant, entities[0].1, year)
        .await
        .expect("balances after");
    let annual_after = balances_after
        .iter()
        .find(|b| b.leave_type == "annual")
        .unwrap();
    assert!(!annual_after.provisioned);
    assert_eq!(annual_after.used_days, dec!(0));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn unlinked_user_gets_empty_balances_and_requests() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, _entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let linked = employee_of_user(&pool, tenant, Uuid::new_v4())
        .await
        .unwrap();
    assert!(linked.is_none());

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn concurrent_create_prevents_overspending() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    // Two separate connections so the requests truly race.
    let pool_a = TenantPool::connect(&url, 2).await.expect("tenant pool a");
    let pool_b = TenantPool::connect(&url, 2).await.expect("tenant pool b");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let year = 2026;
    let annual_id = leave_type_id(&raw, tenant_id, "annual").await;
    seed_balance(&raw, tenant_id, entities[0].1, annual_id, year, dec!(2)).await;

    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let input = LeaveInput {
        leave_type: "annual".into(),
        start_date: d(2026, 10, 10),
        end_date: d(2026, 10, 11),
        reason: None,
        employee_id: None,
    };

    let (a, b) = tokio::join!(
        create_request(&pool_a, tenant, user_id, input.clone()),
        create_request(&pool_b, tenant, user_id, input.clone())
    );

    let successes = [a.is_ok(), b.is_ok()];
    assert_eq!(
        successes.iter().filter(|&&x| x).count(),
        1,
        "exactly one concurrent request should succeed"
    );

    assert_eq!(
        used_days(&raw, tenant_id, entities[0].1, annual_id, year).await,
        dec!(2)
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn cancel_pending_succeeds_and_restores_balance() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let year = 2026;
    let annual_id = leave_type_id(&raw, tenant_id, "annual").await;
    seed_balance(&raw, tenant_id, entities[0].1, annual_id, year, dec!(6)).await;

    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let req = create_request(
        &pool,
        tenant,
        user_id,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 10, 10),
            end_date: d(2026, 10, 11),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    assert_eq!(
        used_days(&raw, tenant_id, entities[0].1, annual_id, year).await,
        dec!(2)
    );

    let status = cancel_request(&pool, tenant, user_id, req.id)
        .await
        .expect("cancel");
    assert_eq!(status, "cancelled");

    let requests = list_requests(&pool, tenant, entities[0].1, Some("cancelled"))
        .await
        .expect("list cancelled");
    assert_eq!(requests.len(), 1);

    assert_eq!(
        used_days(&raw, tenant_id, entities[0].1, annual_id, year).await,
        dec!(0)
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn cancel_another_employees_request_returns_not_found() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let user0 = user_id_of(&raw, tenant_id, entities[0].1).await;
    let user1 = user_id_of(&raw, tenant_id, entities[1].1).await;

    let req = create_request(
        &pool,
        tenant,
        user0,
        LeaveInput {
            leave_type: "personal".into(),
            start_date: d(2026, 11, 1),
            end_date: d(2026, 11, 1),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    let err = cancel_request(&pool, tenant, user1, req.id)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        talentos_hr_runner::leave::LeaveError::NotFound
    ));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn cancel_approved_request_returns_conflict() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let req = create_request(
        &pool,
        tenant,
        user_id,
        LeaveInput {
            leave_type: "bereavement".into(),
            start_date: d(2026, 12, 1),
            end_date: d(2026, 12, 2),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("UPDATE leave_requests SET status = 'approved' WHERE id = $1")
        .bind(req.id)
        .execute(&mut *tx)
        .await
        .expect("approve");
    tx.commit().await.expect("commit");

    let err = cancel_request(&pool, tenant, user_id, req.id)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        talentos_hr_runner::leave::LeaveError::NotCancellable
    ));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn approve_keeps_balance_debited_and_reject_restores() {
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

    let approver = user_id_of(&raw, tenant_id, entities[0].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[1].1).await;
    seed_balance(&raw, tenant_id, entities[1].1, annual_id, year, dec!(6)).await;

    let to_approve = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 10, 5),
            end_date: d(2026, 10, 6),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create approve request");

    let to_reject = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 10, 12),
            end_date: d(2026, 10, 13),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create reject request");

    // Both debited: 4 used days.
    assert_eq!(
        used_days(&raw, tenant_id, entities[1].1, annual_id, year).await,
        dec!(4)
    );

    let approved = decide_request(&pool, tenant, approver, to_approve.id, true)
        .await
        .expect("approve");
    assert_eq!(approved, "approved");

    let rejected = decide_request(&pool, tenant, approver, to_reject.id, false)
        .await
        .expect("reject");
    assert_eq!(rejected, "rejected");

    // Approved kept the debit, rejected restored it: net 2 used days.
    assert_eq!(
        used_days(&raw, tenant_id, entities[1].1, annual_id, year).await,
        dec!(2)
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn legacy_pending_request_cancels_without_touching_balance() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let year = 2026;
    let annual_id = leave_type_id(&raw, tenant_id, "annual").await;
    seed_balance(&raw, tenant_id, entities[0].1, annual_id, year, dec!(6)).await;

    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    // Insert a legacy-style pending row with balance_debited = false.
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
    .bind(entities[0].1)
    .bind(d(2026, 10, 20))
    .bind(d(2026, 10, 21))
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .expect("insert legacy request");
    tx.commit().await.expect("commit");

    assert_eq!(
        used_days(&raw, tenant_id, entities[0].1, annual_id, year).await,
        dec!(0)
    );

    let status = cancel_request(&pool, tenant, user_id, request_id)
        .await
        .expect("cancel legacy");
    assert_eq!(status, "cancelled");

    // Balance must stay at 0 because the legacy row was never debited.
    assert_eq!(
        used_days(&raw, tenant_id, entities[0].1, annual_id, year).await,
        dec!(0)
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn hr_inbox_lists_pending_with_code_and_name() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let user0 = user_id_of(&raw, tenant_id, entities[0].1).await;
    let user1 = user_id_of(&raw, tenant_id, entities[1].1).await;

    let pending = create_request(
        &pool,
        tenant,
        user0,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 10, 5),
            end_date: d(2026, 10, 6),
            reason: Some("vacation".into()),
            employee_id: None,
        },
    )
    .await
    .expect("create pending");

    let approved = create_request(
        &pool,
        tenant,
        user1,
        LeaveInput {
            leave_type: "sick".into(),
            start_date: d(2026, 10, 7),
            end_date: d(2026, 10, 8),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create approved");

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("UPDATE leave_requests SET status = 'approved' WHERE id = $1")
        .bind(approved.id)
        .execute(&mut *tx)
        .await
        .expect("approve");
    tx.commit().await.expect("commit");

    let inbox = pending_approvals(&pool, tenant).await.expect("inbox");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].id, pending.id);
    assert_eq!(inbox[0].employee_code, "EMP000");
    assert_eq!(inbox[0].employee_name, "ทดสอบ พนักงาน");
    assert_eq!(inbox[0].leave_type, "annual");
    assert_eq!(inbox[0].label_th, "ลาพักร้อน");
    assert_eq!(inbox[0].days, dec!(2));
    assert_eq!(inbox[0].reason.as_deref(), Some("vacation"));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn submitted_request_creates_email_rows_for_hr_and_line_only_when_line_id_exists() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let tenant = TenantId(tenant_id);

    let hr_user = user_id_of(&raw, tenant_id, entities[0].1).await;
    let manager_user = user_id_of(&raw, tenant_id, entities[1].1).await;
    let employee_user = user_id_of(&raw, tenant_id, entities[2].1).await;

    ensure_role(&raw, tenant_id, "hr_admin", hr_user).await;
    ensure_role(&raw, tenant_id, "manager", manager_user).await;
    set_line_user_id(&raw, tenant_id, hr_user, "U_HR").await;
    // manager has no line_user_id

    create_request(
        &pool,
        tenant,
        employee_user,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 10, 5),
            end_date: d(2026, 10, 6),
            reason: Some("vacation".into()),
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    let emails = pending_notifications(&raw, tenant_id, "email").await;
    assert_eq!(emails.len(), 2, "HR admin and manager should receive email");
    let recipients: Vec<_> = emails.iter().map(|(r, _, _)| r.clone()).collect();
    let hr_email = format!("t-0-{}@example.com", &hr_user.to_string()[..8]);
    let mgr_email = format!("t-1-{}@example.com", &manager_user.to_string()[..8]);
    assert!(recipients.contains(&hr_email));
    assert!(recipients.contains(&mgr_email));
    for (_, subject, body) in &emails {
        assert!(
            subject.contains("ลาพักร้อน"),
            "subject contains label_th: {subject}"
        );
        assert!(
            body.contains("ทดสอบ พนักงาน"),
            "body contains employee name: {body}"
        );
        assert!(
            body.contains("2026-10-05"),
            "body contains start date: {body}"
        );
        assert!(
            body.contains("2026-10-06"),
            "body contains end date: {body}"
        );
    }

    let lines = pending_notifications(&raw, tenant_id, "line").await;
    assert_eq!(lines.len(), 1, "only HR admin with line_user_id gets LINE");
    assert_eq!(lines[0].0, "U_HR");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn decided_request_creates_notification_for_employee() {
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
    set_line_user_id(&raw, tenant_id, requester, "U_EMP").await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "sick".into(),
            start_date: d(2026, 11, 1),
            end_date: d(2026, 11, 2),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    // clear submit notifications so only the decision notification remains
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("DELETE FROM notifications WHERE tenant_id = $1")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .expect("clear notifications");
    tx.commit().await.expect("commit clear");

    let status = decide_request(&pool, tenant, approver, req.id, true)
        .await
        .expect("approve");
    assert_eq!(status, "approved");

    let emails = pending_notifications(&raw, tenant_id, "email").await;
    assert_eq!(emails.len(), 1, "employee should receive decision email");
    assert!(emails[0].1.contains("ลาป่วย"));
    assert!(emails[0].1.contains("อนุมัติ"));

    let lines = pending_notifications(&raw, tenant_id, "line").await;
    assert_eq!(lines.len(), 1, "employee with line id should receive LINE");
    assert_eq!(lines[0].0, "U_EMP");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn cancelled_request_creates_no_decision_notification() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let req = create_request(
        &pool,
        tenant,
        user_id,
        LeaveInput {
            leave_type: "personal".into(),
            start_date: d(2026, 12, 1),
            end_date: d(2026, 12, 1),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    // clear submit notifications
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("DELETE FROM notifications WHERE tenant_id = $1")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .expect("clear notifications");
    tx.commit().await.expect("commit clear");

    let status = cancel_request(&pool, tenant, user_id, req.id)
        .await
        .expect("cancel");
    assert_eq!(status, "cancelled");

    let emails = pending_notifications(&raw, tenant_id, "email").await;
    assert!(emails.is_empty(), "cancelled request should not notify");

    cleanup(&raw, tenant_id).await;
}

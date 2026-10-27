//! Self-service leave + HR pending inbox.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::NaiveDate;
use rust_decimal_macros::dec;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::leave::{
    cancel_request, create_request, employee_of_user, list_balances, list_requests,
    pending_approvals, LeaveInput,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_entities};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

async fn user_id_of(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid) -> Uuid {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query("SELECT user_id FROM employees WHERE id = $1")
        .bind(employee_id)
        .fetch_one(&mut *tx)
        .await
        .expect("fetch user_id");
    let _ = tx.rollback().await;
    row.get("user_id")
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

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let annual_type_id: i32 = sqlx::query_scalar("SELECT id FROM leave_types WHERE name = 'annual'")
        .fetch_one(&mut *tx)
        .await
        .expect("annual type");
    for (_entity_id, employee_id) in entities.iter() {
        sqlx::query(
            "INSERT INTO leave_balances (tenant_id, employee_id, leave_type_id, year, total_days, used_days)
             VALUES ($1, $2, $3, $4, 6, 0)",
        )
        .bind(tenant_id)
        .bind(*employee_id)
        .bind(annual_type_id)
        .bind(year)
        .execute(&mut *tx)
        .await
        .expect("seed balance");
    }
    tx.commit().await.expect("commit seed");

    let mut users = Vec::new();
    for (_entity_id, employee_id) in entities.iter() {
        let user_id = user_id_of(&raw, tenant_id, *employee_id).await;
        users.push((user_id, *employee_id));
    }

    // Create a request for employee 0 only.
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

    // User 0 sees one request and a balance row for annual.
    let my_emp = employee_of_user(&pool, tenant, users[0].0)
        .await
        .expect("resolve employee")
        .expect("employee exists");
    let balances = list_balances(&pool, tenant, my_emp, year).await.expect("balances");
    let annual = balances.iter().find(|b| b.leave_type == "annual").unwrap();
    assert_eq!(annual.used_days, dec!(2));
    assert_eq!(annual.remaining_days, dec!(4));

    let requests = list_requests(&pool, tenant, my_emp, None).await.expect("requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].id, req.id);

    // User 1 sees zero requests and zero used annual days.
    let other_emp = employee_of_user(&pool, tenant, users[1].0)
        .await
        .expect("resolve employee")
        .expect("employee exists");
    let other_balances = list_balances(&pool, tenant, other_emp, year).await.expect("balances");
    let other_annual = other_balances.iter().find(|b| b.leave_type == "annual").unwrap();
    assert_eq!(other_annual.used_days, dec!(0));
    assert_eq!(other_annual.remaining_days, dec!(6));

    let other_requests = list_requests(&pool, tenant, other_emp, None)
        .await
        .expect("requests");
    assert!(other_requests.is_empty());

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

    let linked = employee_of_user(&pool, tenant, Uuid::new_v4()).await.unwrap();
    assert!(linked.is_none());

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn cancel_pending_succeeds() {
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
            leave_type: "sick".into(),
            start_date: d(2026, 10, 10),
            end_date: d(2026, 10, 11),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    let status = cancel_request(&pool, tenant, user_id, req.id).await.expect("cancel");
    assert_eq!(status, "cancelled");

    let requests = list_requests(&pool, tenant, entities[0].1, Some("cancelled"))
        .await
        .expect("list cancelled");
    assert_eq!(requests.len(), 1);

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

    let err = cancel_request(&pool, tenant, user1, req.id).await.unwrap_err();
    assert!(matches!(err, talentos_hr_runner::leave::LeaveError::NotFound));

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

    let err = cancel_request(&pool, tenant, user_id, req.id).await.unwrap_err();
    assert!(matches!(
        err,
        talentos_hr_runner::leave::LeaveError::NotCancellable
    ));

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

    // One pending, one approved.
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
    assert_eq!(inbox[0].days, dec!(2));
    assert_eq!(inbox[0].reason.as_deref(), Some("vacation"));

    cleanup(&raw, tenant_id).await;
}

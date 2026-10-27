//! Self-service OT + HR pending inbox.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::NaiveDate;
use sqlx::PgPool;
use talentos_db::TenantPool;
use talentos_hr_runner::ot::{
    cancel_request, create_request, decide_request, employee_of_user, list_requests,
    pending_approvals, tenant_requests, OtInput,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

#[tokio::test]
async fn linked_user_creates_and_lists_own_requests() {
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
        OtInput {
            work_date: d(2026, 10, 5),
            minutes: 120,
            reason: Some("deploy".into()),
        },
    )
    .await
    .expect("create request");
    assert_eq!(req.employee_id, entities[0].1);
    assert_eq!(req.work_date, d(2026, 10, 5));
    assert_eq!(req.minutes, 120);
    assert_eq!(req.status, "pending");

    let my_emp = employee_of_user(&pool, tenant, user0)
        .await
        .expect("resolve employee")
        .expect("employee exists");
    let requests = list_requests(&pool, tenant, my_emp, None)
        .await
        .expect("list requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].id, req.id);

    let other_emp = employee_of_user(&pool, tenant, user1)
        .await
        .expect("resolve employee")
        .expect("employee exists");
    let other_requests = list_requests(&pool, tenant, other_emp, None)
        .await
        .expect("list other requests");
    assert!(other_requests.is_empty());

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn unlinked_user_gets_empty_requests() {
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
        .expect("resolve employee");
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
        OtInput {
            work_date: d(2026, 10, 10),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create request");

    cancel_request(&pool, tenant, user_id, req.id)
        .await
        .expect("cancel");

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
        OtInput {
            work_date: d(2026, 11, 1),
            minutes: 90,
            reason: None,
        },
    )
    .await
    .expect("create request");

    let err = cancel_request(&pool, tenant, user1, req.id)
        .await
        .unwrap_err();
    assert!(matches!(err, talentos_hr_runner::ot::OtError::NotFound));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn cancel_approved_request_returns_not_pending() {
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
        OtInput {
            work_date: d(2026, 12, 1),
            minutes: 60,
            reason: None,
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
    sqlx::query("UPDATE ot_requests SET status = 'approved' WHERE id = $1")
        .bind(req.id)
        .execute(&mut *tx)
        .await
        .expect("approve");
    tx.commit().await.expect("commit");

    let err = cancel_request(&pool, tenant, user_id, req.id)
        .await
        .unwrap_err();
    assert!(matches!(err, talentos_hr_runner::ot::OtError::NotPending));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn duplicate_live_date_returns_conflict() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    create_request(
        &pool,
        tenant,
        user_id,
        OtInput {
            work_date: d(2026, 10, 5),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create first request");

    let err = create_request(
        &pool,
        tenant,
        user_id,
        OtInput {
            work_date: d(2026, 10, 5),
            minutes: 90,
            reason: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(
        err,
        talentos_hr_runner::ot::OtError::DuplicateDate
    ));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn rejected_request_frees_the_date() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let requester = user_id_of(&raw, tenant_id, entities[0].1).await;
    let approver = user_id_of(&raw, tenant_id, entities[1].1).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        OtInput {
            work_date: d(2026, 10, 5),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create request");

    let status = decide_request(&pool, tenant, approver, req.id, false)
        .await
        .expect("reject");
    assert_eq!(status, "rejected");

    create_request(
        &pool,
        tenant,
        requester,
        OtInput {
            work_date: d(2026, 10, 5),
            minutes: 90,
            reason: None,
        },
    )
    .await
    .expect("create after rejection");

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
        OtInput {
            work_date: d(2026, 10, 5),
            minutes: 120,
            reason: Some("deploy".into()),
        },
    )
    .await
    .expect("create pending");

    let approved = create_request(
        &pool,
        tenant,
        user1,
        OtInput {
            work_date: d(2026, 10, 6),
            minutes: 60,
            reason: None,
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
    sqlx::query("UPDATE ot_requests SET status = 'approved' WHERE id = $1")
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
    assert_eq!(inbox[0].work_date, d(2026, 10, 5));
    assert_eq!(inbox[0].minutes, 120);
    assert_eq!(inbox[0].reason.as_deref(), Some("deploy"));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn decide_twice_returns_not_pending() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let requester = user_id_of(&raw, tenant_id, entities[0].1).await;
    let approver = user_id_of(&raw, tenant_id, entities[1].1).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        OtInput {
            work_date: d(2026, 10, 5),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create request");

    let status = decide_request(&pool, tenant, approver, req.id, true)
        .await
        .expect("approve");
    assert_eq!(status, "approved");

    let err = decide_request(&pool, tenant, approver, req.id, false)
        .await
        .unwrap_err();
    assert!(matches!(err, talentos_hr_runner::ot::OtError::NotPending));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn minutes_out_of_bounds_are_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let err = create_request(
        &pool,
        tenant,
        user_id,
        OtInput {
            work_date: d(2026, 10, 5),
            minutes: 0,
            reason: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(
        err,
        talentos_hr_runner::ot::OtError::InvalidMinutes
    ));

    let err = create_request(
        &pool,
        tenant,
        user_id,
        OtInput {
            work_date: d(2026, 10, 6),
            minutes: 721,
            reason: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(
        err,
        talentos_hr_runner::ot::OtError::InvalidMinutes
    ));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn reason_length_is_validated() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let too_long = "x".repeat(501);

    let err = create_request(
        &pool,
        tenant,
        user_id,
        OtInput {
            work_date: d(2026, 10, 5),
            minutes: 60,
            reason: Some(too_long),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(
        err,
        talentos_hr_runner::ot::OtError::InvalidReason
    ));

    let exactly_500 = "x".repeat(500);
    create_request(
        &pool,
        tenant,
        user_id,
        OtInput {
            work_date: d(2026, 10, 6),
            minutes: 60,
            reason: Some(exactly_500),
        },
    )
    .await
    .expect("500 char reason accepted");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn tenant_requests_can_filter_by_status() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let requester = user_id_of(&raw, tenant_id, entities[0].1).await;
    let approver = user_id_of(&raw, tenant_id, entities[1].1).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        OtInput {
            work_date: d(2026, 10, 5),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create request");

    let pending = tenant_requests(&pool, tenant, Some("pending"), None, None, 100)
        .await
        .expect("list pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, req.id);
    assert_eq!(pending[0].status, "pending");
    assert!(pending[0].decided_at.is_none());

    let approved_status = decide_request(&pool, tenant, approver, req.id, true)
        .await
        .expect("approve");
    assert_eq!(approved_status, "approved");

    let approved = tenant_requests(&pool, tenant, Some("approved"), None, None, 100)
        .await
        .expect("list approved");
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].id, req.id);
    assert_eq!(approved[0].employee_code, "EMP000");
    assert_eq!(approved[0].status, "approved");
    assert!(approved[0].decided_at.is_some());

    let pending_after = tenant_requests(&pool, tenant, Some("pending"), None, None, 100)
        .await
        .expect("list pending after");
    assert!(pending_after.is_empty());

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn tenant_requests_clamps_limit_and_rejects_bad_status() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    create_request(
        &pool,
        tenant,
        user_id,
        OtInput {
            work_date: d(2026, 10, 5),
            minutes: 60,
            reason: None,
        },
    )
    .await
    .expect("create request");

    // Limit clamped to 1..=500; 0 should still return at most one row.
    let one = tenant_requests(&pool, tenant, None, None, None, 0)
        .await
        .expect("clamped low");
    assert_eq!(one.len(), 1);

    let many = tenant_requests(&pool, tenant, None, None, None, 1000)
        .await
        .expect("clamped high");
    assert_eq!(many.len(), 1);

    let err = tenant_requests(&pool, tenant, Some("invalid"), None, None, 100)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        talentos_hr_runner::ot::OtError::InvalidStatus
    ));

    cleanup(&raw, tenant_id).await;
}

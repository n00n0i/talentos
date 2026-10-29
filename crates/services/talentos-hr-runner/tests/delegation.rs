//! Approval delegation tests: a manager/HR delegates leave-approval authority
//! to an active user for a date range, and `decide_request` honours it.
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::{Duration, NaiveDate, Utc};
use sqlx::PgPool;
use talentos_db::TenantPool;
use talentos_hr_runner::{
    create_delegation, create_request, decide_request, delete_delegation, list_delegations,
    DelegationError, DelegationInput, LeaveError, LeaveInput,
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

fn today() -> NaiveDate {
    Utc::now().date_naive()
}

/// Create a pending leave request whose first (manager) step is ready.
async fn seed_pending_request(pool: &TenantPool, tenant: TenantId, requester: Uuid) -> Uuid {
    create_request(
        pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "personal".into(),
            start_date: d(2026, 12, 3),
            end_date: d(2026, 12, 3),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request")
    .id
}

#[tokio::test]
async fn delegate_approves_manager_step() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let tenant = TenantId(tenant_id);

    let manager = user_id_of(&raw, tenant_id, entities[0].1).await;
    let delegate = user_id_of(&raw, tenant_id, entities[1].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[2].1).await;
    ensure_role(&raw, tenant_id, "manager", manager).await;

    set_payroll_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;

    let request_id = seed_pending_request(&pool, tenant, requester).await;

    // The delegate (a plain employee) cannot approve the manager step yet.
    let denied = decide_request(
        &pool,
        tenant,
        delegate,
        &["employee".into()],
        request_id,
        true,
    )
    .await
    .unwrap_err();
    assert!(matches!(denied, LeaveError::Forbidden));

    // Manager delegates manager approval to the employee for the current window.
    create_delegation(
        &pool,
        tenant,
        manager,
        false,
        DelegationInput {
            delegate_user_id: delegate,
            starts_on: today() - Duration::days(1),
            ends_on: today() + Duration::days(1),
            delegator_user_id: None,
        },
    )
    .await
    .expect("create delegation");

    let outcome = decide_request(
        &pool,
        tenant,
        delegate,
        &["employee".into()],
        request_id,
        true,
    )
    .await
    .expect("delegate approves");
    assert_eq!(outcome.status, "pending"); // advanced to the hr_admin step
    assert_eq!(outcome.step, 2);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn expired_delegation_is_ignored() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let tenant = TenantId(tenant_id);

    let manager = user_id_of(&raw, tenant_id, entities[0].1).await;
    let delegate = user_id_of(&raw, tenant_id, entities[1].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[2].1).await;
    ensure_role(&raw, tenant_id, "manager", manager).await;

    set_payroll_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;
    let request_id = seed_pending_request(&pool, tenant, requester).await;

    // Delegation ended yesterday.
    create_delegation(
        &pool,
        tenant,
        manager,
        false,
        DelegationInput {
            delegate_user_id: delegate,
            starts_on: today() - Duration::days(2),
            ends_on: today() - Duration::days(1),
            delegator_user_id: None,
        },
    )
    .await
    .expect("create expired delegation");

    let err = decide_request(
        &pool,
        tenant,
        delegate,
        &["employee".into()],
        request_id,
        true,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, LeaveError::Forbidden));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn self_delegation_is_rejected() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let manager = user_id_of(&raw, tenant_id, entities[0].1).await;
    ensure_role(&raw, tenant_id, "manager", manager).await;

    let err = create_delegation(
        &pool,
        tenant,
        manager,
        false,
        DelegationInput {
            delegate_user_id: manager, // delegate == delegator
            starts_on: today(),
            ends_on: today() + Duration::days(1),
            delegator_user_id: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DelegationError::SelfDelegation));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn hr_creates_delegation_for_another_manager() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let tenant = TenantId(tenant_id);

    let hr = user_id_of(&raw, tenant_id, entities[0].1).await;
    let manager = user_id_of(&raw, tenant_id, entities[1].1).await;
    let delegate = user_id_of(&raw, tenant_id, entities[2].1).await;
    ensure_role(&raw, tenant_id, "hr_admin", hr).await;
    ensure_role(&raw, tenant_id, "manager", manager).await;

    let view = create_delegation(
        &pool,
        tenant,
        hr,
        true,
        DelegationInput {
            delegate_user_id: delegate,
            starts_on: today(),
            ends_on: today() + Duration::days(1),
            delegator_user_id: Some(manager), // HR names the delegator
        },
    )
    .await
    .expect("hr creates delegation");

    assert_eq!(view.delegator_user_id, manager);
    assert_eq!(view.delegate_user_id, delegate);

    // HR sees every delegation in the tenant.
    let all = list_delegations(&pool, tenant, hr, true)
        .await
        .expect("hr list");
    assert!(all.iter().any(|d| d.id == view.id));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn delete_stops_delegated_rights() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let tenant = TenantId(tenant_id);

    let manager = user_id_of(&raw, tenant_id, entities[0].1).await;
    let delegate = user_id_of(&raw, tenant_id, entities[1].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[2].1).await;
    ensure_role(&raw, tenant_id, "manager", manager).await;

    set_payroll_chain(&raw, tenant_id, &["manager".into(), "hr_admin".into()]).await;
    let request_id = seed_pending_request(&pool, tenant, requester).await;

    let view = create_delegation(
        &pool,
        tenant,
        manager,
        false,
        DelegationInput {
            delegate_user_id: delegate,
            starts_on: today() - Duration::days(1),
            ends_on: today() + Duration::days(1),
            delegator_user_id: None,
        },
    )
    .await
    .expect("create delegation");

    delete_delegation(&pool, tenant, manager, false, view.id)
        .await
        .expect("delete delegation");

    let err = decide_request(
        &pool,
        tenant,
        delegate,
        &["employee".into()],
        request_id,
        true,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, LeaveError::Forbidden));

    cleanup(&raw, tenant_id).await;
}

//! Conditional leave approval rule tests.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::NaiveDate;
use rust_decimal_macros::dec;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::{
    create_rule, delete_rule,
    leave::{create_request, LeaveInput},
    list_rules, ApprovalRuleError, ApprovalRuleInput,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, set_payroll_chain, user_id_of};

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

async fn seed_balance(pool: &PgPool, tenant_id: Uuid, employee_id: Uuid, leave_type_id: i32) {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "INSERT INTO leave_balances (tenant_id, employee_id, leave_type_id, year, total_days, used_days)
         VALUES ($1, $2, $3, 2026, 30, 0)",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .bind(leave_type_id)
    .execute(&mut *tx)
    .await
    .expect("seed balance");
    tx.commit().await.expect("commit seed");
}

async fn insert_rule(
    pool: &PgPool,
    tenant_id: Uuid,
    leave_type: Option<&str>,
    min_days: Option<&str>,
    chain: &[&str],
    priority: i32,
) {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "INSERT INTO leave_approval_rules (tenant_id, leave_type, min_days, chain, priority)
         VALUES ($1, $2, $3::numeric, $4, $5)",
    )
    .bind(tenant_id)
    .bind(leave_type)
    .bind(min_days)
    .bind(chain)
    .bind(priority)
    .execute(&mut *tx)
    .await
    .expect("insert rule");
    tx.commit().await.expect("commit rule");
}

async fn request_chain(pool: &PgPool, tenant_id: Uuid, request_id: Uuid) -> Vec<(i32, String)> {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let rows = sqlx::query(
        "SELECT step_no, approver_role FROM leave_approval_steps
         WHERE leave_request_id = $1 ORDER BY step_no",
    )
    .bind(request_id)
    .fetch_all(&mut *tx)
    .await
    .expect("fetch steps");
    let _ = tx.rollback().await;
    rows.into_iter()
        .map(|r| (r.get("step_no"), r.get("approver_role")))
        .collect()
}

async fn setup(raw: &PgPool, n: usize) -> (TenantId, Uuid, Vec<(Uuid, Uuid)>) {
    let (tenant_id, entities) = seed_tenant_entities(raw, n).await;
    let tenant = TenantId(tenant_id);
    // Default chain is manager -> hr_admin for these tests so a rule match is
    // distinguishable from the fallback.
    set_payroll_chain(
        raw,
        tenant_id,
        &["manager".to_string(), "hr_admin".to_string()],
    )
    .await;
    let requester = user_id_of(raw, tenant_id, entities[n - 1].1).await;
    (tenant, requester, entities)
}

#[tokio::test]
async fn type_specific_rule_beats_generic() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant, requester, entities) = setup(&raw, 2).await;
    let tenant_id = tenant.0;
    let annual_id = leave_type_id(&raw, tenant_id, "annual").await;
    seed_balance(&raw, tenant_id, entities[1].1, annual_id).await;

    insert_rule(&raw, tenant_id, None, None, &["hr_admin"], 0).await; // generic
    insert_rule(
        &raw,
        tenant_id,
        Some("annual"),
        None,
        &["manager", "hr_admin"],
        0,
    )
    .await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 11, 2),
            end_date: d(2026, 11, 2),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    let steps = request_chain(&raw, tenant_id, req.id).await;
    assert_eq!(
        steps,
        vec![(1, "manager".into()), (2, "hr_admin".into())],
        "type-specific rule should win over generic"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn min_days_threshold_and_below_threshold_uses_default() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant, requester, entities) = setup(&raw, 2).await;
    let tenant_id = tenant.0;
    let annual_id = leave_type_id(&raw, tenant_id, "annual").await;
    seed_balance(&raw, tenant_id, entities[1].1, annual_id).await;

    insert_rule(&raw, tenant_id, Some("annual"), Some("5"), &["hr_admin"], 0).await;

    // Below threshold (2 days): falls back to default manager -> hr_admin.
    let short = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 11, 9),
            end_date: d(2026, 11, 10),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create short request");
    assert_eq!(
        request_chain(&raw, tenant_id, short.id).await,
        vec![(1, "manager".into()), (2, "hr_admin".into())]
    );

    // At/above threshold (5 days): rule matches, single hr_admin step.
    let long = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 11, 16),
            end_date: d(2026, 11, 20),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create long request");
    assert_eq!(
        request_chain(&raw, tenant_id, long.id).await,
        vec![(1, "hr_admin".into())]
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn priority_breaks_tie_between_equivalent_rules() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant, requester, entities) = setup(&raw, 2).await;
    let tenant_id = tenant.0;
    let annual_id = leave_type_id(&raw, tenant_id, "annual").await;
    seed_balance(&raw, tenant_id, entities[1].1, annual_id).await;

    // Two identical type-specific rules, differing only in priority.
    insert_rule(&raw, tenant_id, Some("annual"), None, &["hr_admin"], 0).await;
    insert_rule(
        &raw,
        tenant_id,
        Some("annual"),
        None,
        &["manager", "hr_admin"],
        10,
    )
    .await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 11, 23),
            end_date: d(2026, 11, 23),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");

    assert_eq!(
        request_chain(&raw, tenant_id, req.id).await,
        vec![(1, "manager".into()), (2, "hr_admin".into())],
        "higher priority rule should win the tie"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn request_keeps_chain_after_rule_deleted() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant, requester, entities) = setup(&raw, 2).await;
    let tenant_id = tenant.0;
    let annual_id = leave_type_id(&raw, tenant_id, "annual").await;
    seed_balance(&raw, tenant_id, entities[1].1, annual_id).await;

    insert_rule(&raw, tenant_id, Some("annual"), None, &["hr_admin"], 0).await;

    let req = create_request(
        &pool,
        tenant,
        requester,
        LeaveInput {
            leave_type: "annual".into(),
            start_date: d(2026, 11, 24),
            end_date: d(2026, 11, 24),
            reason: None,
            employee_id: None,
        },
    )
    .await
    .expect("create request");
    assert_eq!(
        request_chain(&raw, tenant_id, req.id).await,
        vec![(1, "hr_admin".into())]
    );

    // Delete the rule afterwards: the request's steps are already snapshotted.
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("DELETE FROM leave_approval_rules WHERE tenant_id = $1")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .expect("delete rules");
    tx.commit().await.expect("commit");

    assert_eq!(
        request_chain(&raw, tenant_id, req.id).await,
        vec![(1, "hr_admin".into())],
        "request keeps the chain snapshotted at creation"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn crud_is_tenant_scoped_and_validated() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_a, created_by, _) = setup(&raw, 1).await;
    let (tenant_b, _, _) = setup(&raw, 1).await;

    // Validation: bad chain and bad type are rejected.
    let bad_chain = create_rule(
        &pool,
        tenant_a,
        created_by,
        ApprovalRuleInput {
            leave_type: None,
            min_days: None,
            chain: vec!["manager".to_string()],
            priority: None,
        },
    )
    .await;
    assert!(matches!(bad_chain, Err(ApprovalRuleError::InvalidChain)));

    let bad_type = create_rule(
        &pool,
        tenant_a,
        created_by,
        ApprovalRuleInput {
            leave_type: Some("holiday".to_string()),
            min_days: None,
            chain: vec!["hr_admin".to_string()],
            priority: None,
        },
    )
    .await;
    assert!(matches!(bad_type, Err(ApprovalRuleError::InvalidType)));

    // Create a valid rule in tenant A.
    let view = create_rule(
        &pool,
        tenant_a,
        created_by,
        ApprovalRuleInput {
            leave_type: Some("annual".to_string()),
            min_days: Some(dec!(5)),
            chain: vec!["manager".to_string(), "hr_admin".to_string()],
            priority: Some(3),
        },
    )
    .await
    .expect("create rule");
    assert_eq!(view.priority, 3);
    assert_eq!(view.leave_type.as_deref(), Some("annual"));

    // Tenant A sees it; tenant B does not.
    let a_rules = list_rules(&pool, tenant_a).await.expect("list a");
    assert_eq!(a_rules.len(), 1);
    assert_eq!(a_rules[0].id, view.id);
    let b_rules = list_rules(&pool, tenant_b).await.expect("list b");
    assert!(b_rules.is_empty(), "tenant B must not see tenant A's rule");

    // Delete from tenant B does not affect tenant A's rule.
    assert!(matches!(
        delete_rule(&pool, tenant_b, view.id).await,
        Err(ApprovalRuleError::NotFound)
    ));
    delete_rule(&pool, tenant_a, view.id)
        .await
        .expect("delete a");
    assert!(list_rules(&pool, tenant_a)
        .await
        .expect("list a empty")
        .is_empty());

    cleanup(&raw, tenant_a.0).await;
    cleanup(&raw, tenant_b.0).await;
}

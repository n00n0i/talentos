//! Performance goals: CRUD, cycle filter, validation, tenant/self scope.
//! Requires DATABASE_URL; skips when unset.

use chrono::NaiveDate;
use sqlx::PgPool;
use talentos_db::TenantPool;
use talentos_hr_runner::goals::{
    create_goal, delete_goal, list_goals, list_my_goals, update_goal, GoalError, GoalInput,
};
use talentos_hr_runner::performance::{create_cycle, CycleInput};
use talentos_kernel::TenantId;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn goal(title: &str) -> GoalInput {
    GoalInput {
        cycle_id: None,
        title: title.into(),
        description: Some("คำอธิบาย".into()),
        target: Some("100%".into()),
    }
}

#[tokio::test]
async fn goals_crud_cycle_filter_and_isolation() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let (_, employee_a) = entities[0];
    let (_, employee_b) = entities[1];
    let user_a = user_id_of(&raw, tenant_id, employee_a).await;
    let user_b = user_id_of(&raw, tenant_id, employee_b).await;
    let tenant = TenantId(tenant_id);

    let cycle = create_cycle(
        &pool,
        tenant,
        user_a,
        &CycleInput {
            name: "รอบ 2026".into(),
            period_start: d(2026, 1, 1),
            period_end: d(2026, 12, 31),
        },
    )
    .await
    .expect("cycle");

    let with_cycle = create_goal(
        &pool,
        tenant,
        user_a,
        employee_a,
        &GoalInput {
            cycle_id: Some(cycle.id),
            ..goal("เพิ่มยอดขาย 20%")
        },
    )
    .await
    .expect("goal with cycle");
    assert_eq!(with_cycle.cycle_name.as_deref(), Some("รอบ 2026"));
    assert_eq!(with_cycle.progress, 0);

    let without_cycle = create_goal(&pool, tenant, user_a, employee_a, &goal("ลดต้นทุน"))
        .await
        .expect("goal without cycle");
    create_goal(&pool, tenant, user_b, employee_b, &goal("ของ B"))
        .await
        .expect("B goal");

    assert_eq!(
        list_goals(&pool, tenant, employee_a, None)
            .await
            .expect("all A goals")
            .len(),
        2
    );
    let filtered = list_goals(&pool, tenant, employee_a, Some(cycle.id))
        .await
        .expect("filtered");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, with_cycle.id);

    let updated = update_goal(&pool, tenant, with_cycle.id, 40, "active")
        .await
        .expect("update progress");
    assert_eq!(updated.progress, 40);
    let done = update_goal(&pool, tenant, with_cycle.id, 100, "achieved")
        .await
        .expect("achieve");
    assert_eq!(done.status, "achieved");

    assert!(matches!(
        update_goal(&pool, tenant, with_cycle.id, 101, "active").await,
        Err(GoalError::InvalidProgress)
    ));
    assert!(matches!(
        update_goal(&pool, tenant, with_cycle.id, 50, "paused").await,
        Err(GoalError::InvalidStatus)
    ));

    let mine = list_my_goals(&pool, tenant, user_a)
        .await
        .expect("A self goals");
    assert_eq!(mine.len(), 2);
    let b_goals = list_my_goals(&pool, tenant, user_b)
        .await
        .expect("B self goals");
    assert_eq!(b_goals.len(), 1);

    let (other_tenant, other_entities) = seed_tenant_entities(&raw, 1).await;
    let other_user = user_id_of(&raw, other_tenant, other_entities[0].1).await;
    let other = TenantId(other_tenant);
    assert!(list_goals(&pool, other, employee_a, None)
        .await
        .expect("cross-tenant list")
        .is_empty());
    assert!(matches!(
        update_goal(&pool, other, with_cycle.id, 50, "active").await,
        Err(GoalError::GoalNotFound)
    ));
    assert!(matches!(
        delete_goal(&pool, other, with_cycle.id).await,
        Err(GoalError::GoalNotFound)
    ));
    let foreign_cycle = create_cycle(
        &pool,
        other,
        other_user,
        &CycleInput {
            name: "Foreign".into(),
            period_start: d(2026, 1, 1),
            period_end: d(2026, 12, 31),
        },
    )
    .await
    .expect("foreign cycle");
    assert!(matches!(
        create_goal(
            &pool,
            tenant,
            user_a,
            employee_a,
            &GoalInput {
                cycle_id: Some(foreign_cycle.id),
                ..goal("ข้ามรอบ")
            }
        )
        .await,
        Err(GoalError::CycleNotFound)
    ));
    assert!(matches!(
        create_goal(
            &pool,
            tenant,
            user_a,
            other_entities[0].1,
            &goal("ข้าม tenant")
        )
        .await,
        Err(GoalError::EmployeeNotFound)
    ));

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(other_tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM performance_goals")
        .fetch_one(&mut *tx)
        .await
        .expect("rls count");
    let _ = tx.rollback().await;
    assert_eq!(visible, 0, "RLS hides other tenants without a filter");

    delete_goal(&pool, tenant, with_cycle.id)
        .await
        .expect("delete");
    assert_eq!(
        list_goals(&pool, tenant, employee_a, None)
            .await
            .expect("after delete")
            .len(),
        1
    );
    let _ = without_cycle;

    cleanup(&raw, tenant_id).await;
    cleanup(&raw, other_tenant).await;
}

#[tokio::test]
async fn validation() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let (_, employee_id) = entities[0];
    let user = user_id_of(&raw, tenant_id, employee_id).await;
    let tenant = TenantId(tenant_id);

    let mut bad = goal("   ");
    bad.title = "   ".into();
    assert!(matches!(
        create_goal(&pool, tenant, user, employee_id, &bad).await,
        Err(GoalError::InvalidTitle)
    ));
    let mut bad = goal("goal");
    bad.description = Some("d".repeat(1001));
    assert!(matches!(
        create_goal(&pool, tenant, user, employee_id, &bad).await,
        Err(GoalError::InvalidDescription)
    ));
    let mut bad = goal("goal");
    bad.target = Some("t".repeat(201));
    assert!(matches!(
        create_goal(&pool, tenant, user, employee_id, &bad).await,
        Err(GoalError::InvalidTarget)
    ));

    cleanup(&raw, tenant_id).await;
}

use talentos_hr_runner::goals::{add_goal_check_in, list_goal_check_ins, list_goals_for_manager};

/// Assign `manager_id` on a seeded employee so the manager's user can manage them.
async fn link_manager(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
    report: uuid::Uuid,
    manager: uuid::Uuid,
) {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("UPDATE employees SET manager_id = $3 WHERE id = $1 AND tenant_id = $2")
        .bind(report)
        .bind(tenant_id)
        .bind(manager)
        .execute(&mut *tx)
        .await
        .expect("set manager");
    tx.commit().await.expect("commit");
}

#[tokio::test]
async fn goal_check_ins_manager_only_and_audited() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = sqlx::PgPool::connect(&url).await.expect("connect");
    let pool = talentos_db::TenantPool::connect(&url, 2)
        .await
        .expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let report = entities[0].1;
    let manager = entities[1].1;
    let tenant = TenantId(tenant_id);

    // The manager user (linked to `manager`) manages `report`.
    let manager_user = user_id_of(&raw, tenant_id, manager).await;
    link_manager(&raw, tenant_id, report, manager).await;

    // A goal owned by the report.
    let input = GoalInput {
        cycle_id: None,
        title: " เป้าหมาย E2E".into(),
        description: None,
        target: None,
    };
    let owner_user = user_id_of(&raw, tenant_id, report).await;
    let goal = create_goal(&pool, tenant, owner_user, report, &input)
        .await
        .expect("goal");

    // The manager can add a check-in.
    let check_in = add_goal_check_in(&pool, tenant, manager_user, goal.id, "คืบหน้าดี")
        .await
        .expect("check-in");
    assert_eq!(check_in.goal_id, goal.id);
    assert_eq!(check_in.note, "คืบหน้าดี");

    // A non-manager (the report's own user) cannot add a check-in.
    assert!(matches!(
        add_goal_check_in(&pool, tenant, owner_user, goal.id, "ไม่ได้").await,
        Err(talentos_hr_runner::goals::CheckInError::NotManager)
    ));

    // List returns the check-in.
    let check_ins = list_goal_check_ins(&pool, tenant, goal.id)
        .await
        .expect("check-ins");
    assert!(check_ins.iter().any(|c| c.id == check_in.id));

    // Validation: empty note rejected.
    assert!(matches!(
        add_goal_check_in(&pool, tenant, manager_user, goal.id, "  ").await,
        Err(talentos_hr_runner::goals::CheckInError::InvalidNote)
    ));

    let audits: i64 = {
        let mut tx = raw.begin().await.expect("begin");
        sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let n = sqlx::query_scalar(
            "SELECT count(*) FROM audit_log WHERE tenant_id = $1 AND action = 'performance.check_in'",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await
        .expect("check-in audit count");
        let _ = tx.rollback().await;
        n
    };
    assert!(audits >= 1, "check-in is audit-logged");

    // The manager's direct goals include this goal.
    let direct = list_goals_for_manager(&pool, tenant, manager_user)
        .await
        .expect("direct goals");
    assert!(direct.iter().any(|g| g.id == goal.id));

    cleanup(&raw, tenant_id).await;
}

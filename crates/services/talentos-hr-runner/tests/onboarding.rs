//! Onboarding checklist: standard tasks, completion, duplicates, tenant scope.
//! Requires DATABASE_URL; skips when unset.

use chrono::Duration;
use sqlx::PgPool;
use talentos_attendance::bangkok_offset;
use talentos_db::TenantPool;
use talentos_hr_runner::onboarding::{
    create_task, delete_task, list_my_tasks, list_tasks, set_done, set_my_done, start_standard,
    OnboardingError, TaskInput,
};
use talentos_kernel::TenantId;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

fn today() -> chrono::NaiveDate {
    chrono::Utc::now()
        .with_timezone(&bangkok_offset())
        .date_naive()
}

async fn completed_by(
    raw: &PgPool,
    tenant_id: uuid::Uuid,
    task_id: uuid::Uuid,
) -> Option<uuid::Uuid> {
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let by = sqlx::query_scalar("SELECT completed_by FROM employee_onboarding_tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await
        .expect("completed_by");
    let _ = tx.rollback().await;
    by
}

fn task(title: &str, due: Option<i64>) -> TaskInput {
    TaskInput {
        title: title.into(),
        notes: Some("note".into()),
        due_date: due.map(|d| today() + Duration::days(d)),
    }
}

#[tokio::test]
async fn standard_checklist_is_idempotent_and_dates_from_hire() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let (_, employee_id) = entities[0];
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let tenant = TenantId(tenant_id);

    let tasks = start_standard(&pool, tenant, user_id, employee_id)
        .await
        .expect("standard checklist");
    assert_eq!(tasks.len(), 6);
    let contract = tasks
        .iter()
        .find(|t| t.title == "ลงนามสัญญาจ้าง")
        .expect("contract task");
    assert_eq!(
        contract.due_date,
        Some(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
        "seeded hire date + 0 days"
    );
    let equipment = tasks
        .iter()
        .find(|t| t.title == "รับอุปกรณ์และตั้งค่าอีเมล")
        .expect("equipment task");
    assert_eq!(
        equipment.due_date,
        Some(chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap())
    );
    assert!(equipment.days_left.is_some_and(|d| d < 0), "past hire date");

    let again = start_standard(&pool, tenant, user_id, employee_id)
        .await
        .expect("repeat standard");
    assert_eq!(again.len(), 6, "standard checklist must not duplicate");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn completion_duplicates_and_validation() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let (_, employee_id) = entities[0];
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let tenant = TenantId(tenant_id);

    let created = create_task(
        &pool,
        tenant,
        user_id,
        employee_id,
        &task("ส่งคู่มือพนักงาน", Some(5)),
    )
    .await
    .expect("custom task");
    assert_eq!(created.status, "pending");

    let err = create_task(
        &pool,
        tenant,
        user_id,
        employee_id,
        &task("ส่งคู่มือพนักงาน", None),
    )
    .await
    .expect_err("duplicate title");
    assert!(matches!(err, OnboardingError::DuplicateTask));

    let done = set_done(&pool, tenant, user_id, created.id, true)
        .await
        .expect("complete");
    assert_eq!(done.status, "done");
    assert!(done.completed_at.is_some());
    assert_eq!(
        completed_by(&raw, tenant_id, created.id).await,
        Some(user_id)
    );

    let reopened = set_done(&pool, tenant, user_id, created.id, false)
        .await
        .expect("reopen");
    assert_eq!(reopened.status, "pending");
    assert!(reopened.completed_at.is_none());
    assert_eq!(completed_by(&raw, tenant_id, created.id).await, None);

    let mut long_notes = task("งานอื่น", None);
    long_notes.notes = Some("x".repeat(501));
    let err = create_task(&pool, tenant, user_id, employee_id, &long_notes)
        .await
        .expect_err("notes too long");
    assert!(matches!(err, OnboardingError::InvalidNotes));

    let err = create_task(&pool, tenant, user_id, employee_id, &task("   ", None))
        .await
        .expect_err("empty title");
    assert!(matches!(err, OnboardingError::InvalidTitle));

    let (other_tenant, other_entities) = seed_tenant_entities(&raw, 1).await;
    let err = create_task(
        &pool,
        tenant,
        user_id,
        other_entities[0].1,
        &task("ข้าม tenant", None),
    )
    .await
    .expect_err("cross-tenant employee");
    assert!(matches!(err, OnboardingError::EmployeeNotFound));

    cleanup(&raw, tenant_id).await;
    cleanup(&raw, other_tenant).await;
}

#[tokio::test]
async fn self_service_and_cross_tenant_are_scoped() {
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

    start_standard(&pool, tenant, user_a, employee_a)
        .await
        .expect("A checklist");
    let task_b = create_task(&pool, tenant, user_b, employee_b, &task("งานของ B", None))
        .await
        .expect("B task");

    let mine = list_my_tasks(&pool, tenant, user_a)
        .await
        .expect("A self list");
    assert_eq!(mine.len(), 6);
    assert!(mine.iter().all(|t| t.employee_id == employee_a));
    assert!(matches!(
        set_my_done(&pool, tenant, user_a, task_b.id, true).await,
        Err(OnboardingError::NotFound)
    ));

    let own = set_my_done(&pool, tenant, user_a, mine[0].id, true)
        .await
        .expect("A completes own task");
    assert_eq!(own.status, "done");
    assert_eq!(
        completed_by(&raw, tenant_id, mine[0].id).await,
        Some(user_a)
    );

    let (other_tenant, _) = seed_tenant_entities(&raw, 1).await;
    let other = TenantId(other_tenant);
    assert!(matches!(
        set_done(&pool, other, user_a, mine[0].id, false).await,
        Err(OnboardingError::NotFound)
    ));
    assert!(matches!(
        delete_task(&pool, other, mine[0].id).await,
        Err(OnboardingError::NotFound)
    ));
    assert!(matches!(
        list_tasks(&pool, other, employee_a).await,
        Err(OnboardingError::EmployeeNotFound)
    ));
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(other_tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM employee_onboarding_tasks")
        .fetch_one(&mut *tx)
        .await
        .expect("rls count");
    let _ = tx.rollback().await;
    assert_eq!(visible, 0, "RLS hides other tenants even without a filter");

    delete_task(&pool, tenant, mine[0].id)
        .await
        .expect("owner delete");
    delete_task(&pool, tenant, task_b.id)
        .await
        .expect("B delete");
    cleanup(&raw, tenant_id).await;
    cleanup(&raw, other_tenant).await;
}

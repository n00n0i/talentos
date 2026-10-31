//! Performance cycles: start population, submission, close, tenant/self scope.
//! Requires DATABASE_URL; skips when unset.

use chrono::NaiveDate;
use sqlx::PgPool;
use talentos_db::TenantPool;
use talentos_hr_runner::performance::{
    close_cycle, create_cycle, delete_cycle, list_cycles, list_my_reviews, list_reviews,
    start_cycle, submit_review, CycleInput, PerformanceError,
};
use talentos_kernel::TenantId;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn cycle() -> CycleInput {
    CycleInput {
        name: "รอบประเมิน 2026".into(),
        period_start: d(2026, 1, 1),
        period_end: d(2026, 12, 31),
    }
}

#[tokio::test]
async fn cycle_populates_active_employees_and_submits_reviews() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let (_, employee_a) = entities[0];
    let (_, employee_b) = entities[1];
    let (_, employee_c) = entities[2];
    let user_a = user_id_of(&raw, tenant_id, employee_a).await;
    let user_b = user_id_of(&raw, tenant_id, employee_b).await;
    let admin = user_a;
    let tenant = TenantId(tenant_id);

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("UPDATE employees SET status = 'terminated' WHERE id = $1")
        .bind(employee_c)
        .execute(&mut *tx)
        .await
        .expect("terminate");
    tx.commit().await.expect("commit");

    let created = create_cycle(&pool, tenant, admin, &cycle())
        .await
        .expect("cycle");
    assert_eq!(created.status, "draft");

    // A review that already exists for the draft cycle must not be duplicated.
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "INSERT INTO performance_reviews (tenant_id, cycle_id, employee_id)
         VALUES ($1, $2, $3)",
    )
    .bind(tenant_id)
    .bind(created.id)
    .bind(employee_a)
    .execute(&mut *tx)
    .await
    .expect("pre-existing review");
    tx.commit().await.expect("commit");

    start_cycle(&pool, tenant, created.id).await.expect("start");
    let cycles = list_cycles(&pool, tenant).await.expect("cycles");
    assert_eq!(cycles[0].status, "active");
    assert_eq!(cycles[0].review_count, 2, "terminated employee excluded");
    assert_eq!(cycles[0].submitted_count, 0);

    let reviews = list_reviews(&pool, tenant, created.id)
        .await
        .expect("reviews");
    assert_eq!(reviews.len(), 2);
    let review_a = reviews
        .iter()
        .find(|r| r.employee_id == employee_a)
        .expect("A review");
    assert_eq!(review_a.status, "pending");

    assert!(matches!(
        start_cycle(&pool, tenant, created.id).await,
        Err(PerformanceError::CycleNotDraft)
    ));

    let submitted = submit_review(
        &pool,
        tenant,
        admin,
        review_a.id,
        4,
        Some("ส่งงานตรงเวลา"),
        Some("ควรสื่อสารเร็วขึ้น"),
    )
    .await
    .expect("submit");
    assert_eq!(submitted.status, "submitted");
    assert_eq!(submitted.rating, Some(4));
    assert_eq!(submitted.strengths.as_deref(), Some("ส่งงานตรงเวลา"));
    assert!(submitted.submitted_at.is_some());

    assert!(matches!(
        submit_review(&pool, tenant, admin, review_a.id, 6, None, None).await,
        Err(PerformanceError::InvalidRating)
    ));
    let long = "x".repeat(2001);
    assert!(matches!(
        submit_review(&pool, tenant, admin, review_a.id, 4, Some(&long), None).await,
        Err(PerformanceError::InvalidText)
    ));

    let cycles = list_cycles(&pool, tenant).await.expect("cycles");
    assert_eq!(cycles[0].submitted_count, 1);

    let mine = list_my_reviews(&pool, tenant, user_a)
        .await
        .expect("A self reviews");
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].rating, Some(4));
    let b_reviews = list_my_reviews(&pool, tenant, user_b)
        .await
        .expect("B self reviews");
    assert_eq!(b_reviews.len(), 1);
    assert_eq!(b_reviews[0].status, "pending");

    close_cycle(&pool, tenant, created.id).await.expect("close");
    assert!(matches!(
        submit_review(&pool, tenant, admin, review_a.id, 3, None, None).await,
        Err(PerformanceError::CycleNotActive)
    ));
    assert_eq!(
        list_cycles(&pool, tenant).await.expect("cycles")[0].status,
        "closed"
    );

    assert!(
        matches!(
            delete_cycle(&pool, tenant, created.id).await,
            Err(PerformanceError::CycleNotDraft)
        ),
        "active/closed review records are retained"
    );
    assert_eq!(
        list_reviews(&pool, tenant, created.id)
            .await
            .expect("closed reviews")
            .len(),
        2
    );

    let draft = create_cycle(&pool, tenant, admin, &cycle())
        .await
        .expect("draft cycle");
    delete_cycle(&pool, tenant, draft.id)
        .await
        .expect("delete draft");
    assert!(matches!(
        list_reviews(&pool, tenant, draft.id).await,
        Err(PerformanceError::CycleNotFound)
    ));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn validation_and_tenant_isolation() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let admin = user_id_of(&raw, tenant_id, entities[0].1).await;
    let tenant = TenantId(tenant_id);

    let mut bad = cycle();
    bad.name = "  ".into();
    assert!(matches!(
        create_cycle(&pool, tenant, admin, &bad).await,
        Err(PerformanceError::InvalidName)
    ));
    let mut bad = cycle();
    bad.period_end = d(2025, 12, 31);
    assert!(matches!(
        create_cycle(&pool, tenant, admin, &bad).await,
        Err(PerformanceError::InvalidPeriod)
    ));

    let created = create_cycle(&pool, tenant, admin, &cycle())
        .await
        .expect("cycle");
    start_cycle(&pool, tenant, created.id).await.expect("start");
    let review = list_reviews(&pool, tenant, created.id)
        .await
        .expect("reviews")
        .into_iter()
        .next()
        .expect("one review");

    let (other_tenant, _) = seed_tenant_entities(&raw, 1).await;
    let other = TenantId(other_tenant);
    assert!(list_cycles(&pool, other)
        .await
        .expect("other cycles")
        .is_empty());
    assert!(matches!(
        list_reviews(&pool, other, created.id).await,
        Err(PerformanceError::CycleNotFound)
    ));
    assert!(matches!(
        submit_review(&pool, other, admin, review.id, 4, None, None).await,
        Err(PerformanceError::ReviewNotFound)
    ));
    assert!(matches!(
        delete_cycle(&pool, other, created.id).await,
        Err(PerformanceError::CycleNotFound)
    ));

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(other_tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM performance_reviews")
        .fetch_one(&mut *tx)
        .await
        .expect("rls count");
    let _ = tx.rollback().await;
    assert_eq!(visible, 0, "RLS hides other tenants without a filter");

    cleanup(&raw, tenant_id).await;
    cleanup(&raw, other_tenant).await;
}

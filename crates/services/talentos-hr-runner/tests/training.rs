//! Training records: creation, expiry views, validation, tenant/self scoping.
//! Requires DATABASE_URL; skips when unset.

use chrono::{Duration, Utc};
use sqlx::PgPool;
use talentos_attendance::bangkok_offset;
use talentos_db::TenantPool;
use talentos_hr_runner::training::{
    create_training, delete_training, expiring_training, list_my_training, list_training,
    TrainingError, TrainingInput,
};
use talentos_kernel::TenantId;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

fn today() -> chrono::NaiveDate {
    Utc::now().with_timezone(&bangkok_offset()).date_naive()
}

fn input(title: &str, in_days: Option<i64>) -> TrainingInput {
    TrainingInput {
        title: title.into(),
        provider: Some("TalentOS Academy".into()),
        completed_on: Some(today() - Duration::days(30)),
        expires_on: in_days.map(|d| today() + Duration::days(d)),
    }
}

#[tokio::test]
async fn create_list_and_expiry_alerts() {
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

    let soon = create_training(
        &pool,
        tenant,
        user_id,
        employee_id,
        &input("ความปลอดภัยในการทำงาน", Some(10)),
    )
    .await
    .expect("expiring training");
    assert_eq!(soon.days_left, Some(10));

    let expired = create_training(
        &pool,
        tenant,
        user_id,
        employee_id,
        &input("ปฐมพยาบาล", Some(-5)),
    )
    .await
    .expect("expired training");
    assert_eq!(expired.days_left, Some(-5));

    create_training(&pool, tenant, user_id, employee_id, &input("ปฐมนิเทศ", None))
        .await
        .expect("training without expiry");

    let listed = list_training(&pool, tenant, employee_id)
        .await
        .expect("list");
    assert_eq!(listed.len(), 3);

    let expiring = expiring_training(&pool, tenant, 30)
        .await
        .expect("expiring");
    assert_eq!(expiring.len(), 2);
    assert_eq!(expiring[0].training.title, "ปฐมพยาบาล");
    assert_eq!(expiring[0].employee_code, "EMP000");
    assert_eq!(expiring[1].training.title, "ความปลอดภัยในการทำงาน");

    let err = create_training(&pool, tenant, user_id, employee_id, &input("   ", None))
        .await
        .expect_err("empty title");
    assert!(matches!(err, TrainingError::InvalidTitle));

    let mut long_provider = input("อบรม", None);
    long_provider.provider = Some("p".repeat(201));
    let err = create_training(&pool, tenant, user_id, employee_id, &long_provider)
        .await
        .expect_err("provider too long");
    assert!(matches!(err, TrainingError::InvalidProvider));

    let mut bad_dates = input("อบรม", Some(1));
    bad_dates.completed_on = Some(today() + Duration::days(10));
    let err = create_training(&pool, tenant, user_id, employee_id, &bad_dates)
        .await
        .expect_err("expiry before completion");
    assert!(matches!(err, TrainingError::InvalidDates));

    let (other_tenant, other_entities) = seed_tenant_entities(&raw, 1).await;
    let err = create_training(
        &pool,
        tenant,
        user_id,
        other_entities[0].1,
        &input("ข้าม tenant", None),
    )
    .await
    .expect_err("cross-tenant employee");
    assert!(matches!(err, TrainingError::EmployeeNotFound));

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

    let record_a = create_training(&pool, tenant, user_a, employee_a, &input("ของ A", None))
        .await
        .expect("A training");
    let record_b = create_training(&pool, tenant, user_b, employee_b, &input("ของ B", None))
        .await
        .expect("B training");

    let mine = list_my_training(&pool, tenant, user_a)
        .await
        .expect("A self list");
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].id, record_a.id);

    let (other_tenant, _) = seed_tenant_entities(&raw, 1).await;
    let other = TenantId(other_tenant);
    assert!(list_training(&pool, other, employee_a)
        .await
        .expect("cross-tenant list")
        .is_empty());
    assert!(matches!(
        delete_training(&pool, other, record_a.id).await,
        Err(TrainingError::NotFound)
    ));
    assert!(matches!(
        delete_training(&pool, other, record_b.id).await,
        Err(TrainingError::NotFound)
    ));

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(other_tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM employee_training_records")
        .fetch_one(&mut *tx)
        .await
        .expect("rls count");
    let _ = tx.rollback().await;
    assert_eq!(visible, 0, "RLS hides other tenants without a filter");

    delete_training(&pool, tenant, record_a.id)
        .await
        .expect("owner delete A");
    delete_training(&pool, tenant, record_b.id)
        .await
        .expect("owner delete B");
    cleanup(&raw, tenant_id).await;
    cleanup(&raw, other_tenant).await;
}

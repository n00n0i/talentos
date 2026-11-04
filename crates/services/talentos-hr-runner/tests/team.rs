//! Manager team directory: reporting-line scope, tenant scope, safe fields.
//! Requires DATABASE_URL; skips when unset.

use sqlx::PgPool;
use talentos_db::TenantPool;
use talentos_hr_runner::team::list_my_team;
use talentos_kernel::TenantId;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

async fn set_manager(
    raw: &PgPool,
    tenant_id: uuid::Uuid,
    employee_id: uuid::Uuid,
    manager_id: uuid::Uuid,
) {
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("UPDATE employees SET manager_id = $2 WHERE id = $1")
        .bind(employee_id)
        .bind(manager_id)
        .execute(&mut *tx)
        .await
        .expect("set manager");
    tx.commit().await.expect("commit");
}

#[tokio::test]
async fn returns_only_direct_reports_with_safe_fields() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 3).await;
    let manager = entities[0].1;
    let report = entities[1].1;
    let unrelated = entities[2].1;
    let manager_user = user_id_of(&raw, tenant_id, manager).await;
    set_manager(&raw, tenant_id, report, manager).await;

    let team = list_my_team(&pool, TenantId(tenant_id), manager_user)
        .await
        .expect("team");
    assert_eq!(team.len(), 1);
    assert_eq!(team[0].id, report);
    assert_ne!(team[0].id, unrelated);

    let value = serde_json::to_value(&team[0]).expect("serialize");
    let keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    for forbidden in [
        "salary",
        "base_salary",
        "bank_account_number",
        "bank_name",
        "national_id",
        "sso_number",
        "email",
    ] {
        assert!(!keys.contains(&forbidden), "leaked {forbidden}");
    }
    assert_eq!(value["employee_code"], "EMP001");

    // An employee without reports sees an empty list, never the tenant.
    let unrelated_user = user_id_of(&raw, tenant_id, unrelated).await;
    assert!(list_my_team(&pool, TenantId(tenant_id), unrelated_user)
        .await
        .expect("unrelated team")
        .is_empty());

    // Another tenant's manager only sees its own report.
    let (other_tenant, other_entities) = seed_tenant_entities(&raw, 2).await;
    let other_manager = other_entities[0].1;
    let other_report = other_entities[1].1;
    let other_user = user_id_of(&raw, other_tenant, other_manager).await;
    set_manager(&raw, other_tenant, other_report, other_manager).await;
    let other_team = list_my_team(&pool, TenantId(other_tenant), other_user)
        .await
        .expect("other team");
    assert_eq!(other_team.len(), 1);
    assert_eq!(other_team[0].id, other_report);
    assert!(list_my_team(&pool, TenantId(tenant_id), manager_user)
        .await
        .expect("tenant A team")
        .iter()
        .all(|m| m.id != other_report));

    cleanup(&raw, tenant_id).await;
    cleanup(&raw, other_tenant).await;
}

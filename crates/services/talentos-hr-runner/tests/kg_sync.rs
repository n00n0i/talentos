//! KG sync: empty tenant, hierarchy edges, and rebuild idempotency.
//! Requires DATABASE_URL; skips when unset.

use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_hr_runner::kg::NodeClass;
use talentos_hr_runner::kg_sync::{rebuild, NODE_CAP};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url};

/// Seed a tenant + one entity, then `employees` as (code, first_name, department).
async fn seed(raw: &PgPool, employees: &[(&str, &str, &str)]) -> Uuid {
    let tenant_id = Uuid::new_v4();
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'KG Sync Co', $2)")
        .bind(tenant_id)
        .bind(format!("kg-sync-{}", &tenant_id.to_string()[..8]))
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let entity_id = Uuid::new_v4();
    sqlx::query("INSERT INTO entities (id, tenant_id, name) VALUES ($1, $2, 'KG Entity')")
        .bind(entity_id)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .expect("entity");
    for (code, first_name, department) in employees {
        sqlx::query(
            "INSERT INTO employees
               (id, tenant_id, entity_id, employee_code, first_name_th, last_name_th,
                hire_date, position_title, department, base_salary, tax_filing_status,
                pf_member, status)
             VALUES ($1,$2,$3,$4,$5,'ทดสอบ','2024-01-01','Engineer',$6,30000,1,false,'active')",
        )
        .bind(Uuid::new_v4())
        .bind(tenant_id)
        .bind(entity_id)
        .bind(code)
        .bind(first_name)
        .bind(department)
        .execute(&mut *tx)
        .await
        .expect("employee");
    }
    tx.commit().await.expect("commit");
    tenant_id
}

async fn edge_count(raw: &PgPool, tenant_id: Uuid, relation: &str) -> i64 {
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM kg_edges WHERE tenant_id = $1 AND relation = $2")
            .bind(tenant_id)
            .bind(relation)
            .fetch_one(&mut *tx)
            .await
            .expect("edge count");
    let _ = tx.rollback().await;
    n
}

async fn node_id(raw: &PgPool, tenant_id: Uuid, class: &str) -> Uuid {
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query("SELECT id FROM kg_nodes WHERE tenant_id = $1 AND class = $2 LIMIT 1")
        .bind(tenant_id)
        .bind(class)
        .fetch_one(&mut *tx)
        .await
        .expect("node row");
    let _ = tx.rollback().await;
    row.get("id")
}

#[tokio::test]
async fn kg_sync_empty_tenant_has_org_and_entity_only() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let tenant_id = seed(&raw, &[]).await;

    let report = rebuild(&pool, TenantId(tenant_id)).await.expect("rebuild");
    assert_eq!(report.nodes.get(&NodeClass::Organization), Some(&1));
    assert_eq!(report.nodes.get(&NodeClass::LegalEntity), Some(&1));
    for class in NodeClass::ALL {
        if !matches!(class, NodeClass::Organization | NodeClass::LegalEntity) {
            assert_eq!(
                report.nodes.get(&class),
                Some(&0),
                "{class:?} must be empty"
            );
        }
    }
    assert_eq!(
        report.edges, 1,
        "only Organization --has_entity--> LegalEntity"
    );
    assert!(!report.capped);
    assert_eq!(report.nodes.values().sum::<i64>(), 2);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn kg_sync_two_employees_one_department() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let tenant_id = seed(
        &raw,
        &[
            ("EMP001", "สมชาย", "Engineering"),
            ("EMP002", "สมหญิง", "Engineering"),
        ],
    )
    .await;

    rebuild(&pool, TenantId(tenant_id)).await.expect("rebuild");
    assert_eq!(edge_count(&raw, tenant_id, "member_of").await, 2);

    // Two employees, one shared department and position.
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let rows = sqlx::query(
        "SELECT class, count(*) AS n FROM kg_nodes WHERE tenant_id = $1 GROUP BY class",
    )
    .bind(tenant_id)
    .fetch_all(&mut *tx)
    .await
    .expect("counts");
    let _ = tx.rollback().await;
    let mut by_class = std::collections::HashMap::new();
    for row in rows {
        by_class.insert(row.get::<String, _>("class"), row.get::<i64, _>("n"));
    }
    assert_eq!(by_class.get("Employee"), Some(&2));
    assert_eq!(by_class.get("Department"), Some(&1));
    assert_eq!(by_class.get("Position"), Some(&1));

    // Both employees hang off the one department, and the position is held twice.
    assert_eq!(edge_count(&raw, tenant_id, "member_of").await, 2);
    assert_eq!(edge_count(&raw, tenant_id, "held_by").await, 2);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn kg_sync_is_idempotent() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let tenant_id = seed(
        &raw,
        &[
            ("EMP001", "สมชาย", "Engineering"),
            ("EMP002", "สมหญิง", "Engineering"),
        ],
    )
    .await;

    let first = rebuild(&pool, TenantId(tenant_id)).await.expect("first");
    let second = rebuild(&pool, TenantId(tenant_id)).await.expect("second");
    assert_eq!(first.nodes, second.nodes, "node counts must not grow");
    assert_eq!(first.edges, second.edges, "edge counts must not grow");
    assert!(!first.capped);

    // Traversal still reaches from the organization down to an employee.
    let org = node_id(&raw, tenant_id, "Organization").await;
    let (nodes, _) = talentos_hr_runner::kg::neighbors(&pool, TenantId(tenant_id), org, 4)
        .await
        .expect("neighbors");
    assert!(nodes.iter().any(|n| n.class == NodeClass::Employee));

    cleanup(&raw, tenant_id).await;
}

#[test]
fn node_cap_is_five_thousand() {
    assert_eq!(NODE_CAP, 5_000);
}

//! Knowledge graph primitives: upsert idempotency, depth-2 traversal, class counts.
//! Requires DATABASE_URL; skips when unset.

use serde_json::json;
use sqlx::PgPool;
use std::str::FromStr;
use talentos_db::TenantPool;
use talentos_hr_runner::kg::{
    counts, neighbors, upsert_edge, upsert_node, KgEdge, KgNode, NodeClass,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, seed_tenant_entities};

fn node(class: NodeClass, key: &str) -> KgNode {
    KgNode {
        id: Uuid::nil(),
        class,
        key: key.into(),
        label_th: key.into(),
        label_en: Some(key.into()),
        properties: json!({}),
    }
}

fn edge(from: Uuid, relation: &str, to: Uuid) -> KgEdge {
    KgEdge {
        id: Uuid::nil(),
        from_id: from,
        to_id: to,
        relation: relation.into(),
        properties: json!({}),
    }
}

#[tokio::test]
async fn kg_upsert_traversal_and_counts() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, _) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    // Same (class, key) twice updates in place: one row, same id.
    let org = upsert_node(&pool, tenant, &node(NodeClass::Organization, "org"))
        .await
        .expect("org");
    let mut renamed = node(NodeClass::Organization, "org");
    renamed.label_en = Some("Acme".into());
    let org_again = upsert_node(&pool, tenant, &renamed)
        .await
        .expect("org again");
    assert_eq!(org, org_again);

    let dept = upsert_node(&pool, tenant, &node(NodeClass::Department, "dept"))
        .await
        .expect("dept");
    let position = upsert_node(&pool, tenant, &node(NodeClass::Position, "pos"))
        .await
        .expect("position");
    let employee = upsert_node(&pool, tenant, &node(NodeClass::Employee, "emp"))
        .await
        .expect("employee");

    // Chain org -> dept -> position -> employee.
    upsert_edge(&pool, tenant, &edge(org, "has_department", dept))
        .await
        .expect("org->dept");
    upsert_edge(&pool, tenant, &edge(dept, "has_position", position))
        .await
        .expect("dept->pos");
    upsert_edge(&pool, tenant, &edge(position, "held_by", employee))
        .await
        .expect("pos->emp");
    // Upserting the same edge again is a no-op, not a duplicate.
    upsert_edge(&pool, tenant, &edge(position, "held_by", employee))
        .await
        .expect("pos->emp again");

    let (nodes, edges) = neighbors(&pool, tenant, org, 2).await.expect("neighbors");
    let ids: Vec<Uuid> = nodes.iter().map(|n| n.id).collect();
    assert_eq!(nodes.len(), 3, "org + 2 hops");
    assert!(ids.contains(&dept) && ids.contains(&position));
    assert!(!ids.contains(&employee), "3 hops away at depth 2");
    assert_eq!(edges.len(), 2, "edges inside the reached set");

    let (deep_nodes, _) = neighbors(&pool, tenant, org, 3).await.expect("depth 3");
    assert!(deep_nodes.iter().any(|n| n.id == employee));

    let by_class = counts(&pool, tenant).await.expect("counts");
    assert_eq!(by_class.get(&NodeClass::Organization), Some(&1));
    assert_eq!(by_class.get(&NodeClass::Department), Some(&1));
    assert_eq!(by_class.get(&NodeClass::Position), Some(&1));
    assert_eq!(by_class.get(&NodeClass::Employee), Some(&1));
    assert_eq!(by_class.get(&NodeClass::Policy), Some(&0));

    // Edge endpoints from another tenant are rejected.
    let (other_tenant, _) = seed_tenant_entities(&raw, 1).await;
    let other = TenantId(other_tenant);
    let foreign = upsert_node(&pool, other, &node(NodeClass::Organization, "foreign"))
        .await
        .expect("foreign org");
    assert!(
        upsert_edge(&pool, tenant, &edge(org, "has_department", foreign))
            .await
            .is_err()
    );
    let other_counts = counts(&pool, other).await.expect("other counts");
    assert_eq!(other_counts.get(&NodeClass::Organization), Some(&1));
    assert_eq!(other_counts.get(&NodeClass::Department), Some(&0));
    assert_eq!(
        other_counts.values().sum::<i64>(),
        1,
        "no leakage across tenants"
    );

    // Ontology round-trips through as_str/from_str.
    for class in NodeClass::ALL {
        assert_eq!(NodeClass::from_str(class.as_str()).ok(), Some(class));
    }

    cleanup(&raw, tenant_id).await;
    cleanup(&raw, other_tenant).await;
}

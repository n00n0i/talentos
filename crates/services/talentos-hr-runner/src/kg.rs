//! Organizational knowledge graph: ontology classes, nodes, edges, traversal.
//! Tenant isolation is `begin_tenant` + explicit `tenant_id` predicates.

use std::collections::BTreeMap;
use std::str::FromStr;

use serde::Serialize;
use serde_json::Value;
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

/// The 12 ontology classes from the design (spec §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum NodeClass {
    Organization,
    LegalEntity,
    Department,
    Position,
    Employee,
    Skill,
    Policy,
    Benefit,
    PayrollItem,
    Cycle,
    Document,
    Process,
}

impl NodeClass {
    pub const ALL: [NodeClass; 12] = [
        NodeClass::Organization,
        NodeClass::LegalEntity,
        NodeClass::Department,
        NodeClass::Position,
        NodeClass::Employee,
        NodeClass::Skill,
        NodeClass::Policy,
        NodeClass::Benefit,
        NodeClass::PayrollItem,
        NodeClass::Cycle,
        NodeClass::Document,
        NodeClass::Process,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            NodeClass::Organization => "Organization",
            NodeClass::LegalEntity => "LegalEntity",
            NodeClass::Department => "Department",
            NodeClass::Position => "Position",
            NodeClass::Employee => "Employee",
            NodeClass::Skill => "Skill",
            NodeClass::Policy => "Policy",
            NodeClass::Benefit => "Benefit",
            NodeClass::PayrollItem => "PayrollItem",
            NodeClass::Cycle => "Cycle",
            NodeClass::Document => "Document",
            NodeClass::Process => "Process",
        }
    }
}

impl FromStr for NodeClass {
    type Err = KgError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Organization" => Ok(NodeClass::Organization),
            "LegalEntity" => Ok(NodeClass::LegalEntity),
            "Department" => Ok(NodeClass::Department),
            "Position" => Ok(NodeClass::Position),
            "Employee" => Ok(NodeClass::Employee),
            "Skill" => Ok(NodeClass::Skill),
            "Policy" => Ok(NodeClass::Policy),
            "Benefit" => Ok(NodeClass::Benefit),
            "PayrollItem" => Ok(NodeClass::PayrollItem),
            "Cycle" => Ok(NodeClass::Cycle),
            "Document" => Ok(NodeClass::Document),
            "Process" => Ok(NodeClass::Process),
            other => Err(KgError::UnknownClass(other.to_owned())),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KgError {
    #[error("unknown node class: {0}")]
    UnknownClass(String),
    #[error("edge endpoints must both exist for this tenant")]
    EndpointNotFound,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// One real thing in the tenant. `id` is ignored by [`upsert_node`], which
/// returns the stored id (stable across re-syncs of the same class + key).
#[derive(Debug, Clone, Serialize)]
pub struct KgNode {
    pub id: Uuid,
    pub class: NodeClass,
    pub key: String,
    pub label_th: String,
    pub label_en: Option<String>,
    pub properties: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct KgEdge {
    pub id: Uuid,
    pub from_id: Uuid,
    pub to_id: Uuid,
    pub relation: String,
    pub properties: Value,
}

fn map_node(row: &sqlx::postgres::PgRow) -> Result<KgNode, KgError> {
    let class: String = row.try_get("class")?;
    Ok(KgNode {
        id: row.try_get("id")?,
        class: class.parse()?,
        key: row.try_get("key")?,
        label_th: row.try_get("label_th")?,
        label_en: row.try_get("label_en")?,
        properties: row.try_get("properties")?,
    })
}

fn map_edge(row: &sqlx::postgres::PgRow) -> Result<KgEdge, KgError> {
    Ok(KgEdge {
        id: row.try_get("id")?,
        from_id: row.try_get("from_id")?,
        to_id: row.try_get("to_id")?,
        relation: row.try_get("relation")?,
        properties: row.try_get("properties")?,
    })
}

pub async fn upsert_node(
    pool: &TenantPool,
    tenant_id: TenantId,
    node: &KgNode,
) -> Result<Uuid, KgError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO kg_nodes (tenant_id, class, key, label_th, label_en, properties)
           VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (tenant_id, class, key) DO UPDATE
              SET label_th = EXCLUDED.label_th,
                  label_en = EXCLUDED.label_en,
                  properties = EXCLUDED.properties,
                  updated_at = now()
           RETURNING id"#,
    )
    .bind(tenant_id.0)
    .bind(node.class.as_str())
    .bind(&node.key)
    .bind(&node.label_th)
    .bind(&node.label_en)
    .bind(&node.properties)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn upsert_edge(
    pool: &TenantPool,
    tenant_id: TenantId,
    edge: &KgEdge,
) -> Result<Uuid, KgError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let endpoints = [edge.from_id, edge.to_id];
    let found: i64 =
        sqlx::query_scalar("SELECT count(*) FROM kg_nodes WHERE tenant_id = $1 AND id = ANY($2)")
            .bind(tenant_id.0)
            .bind(endpoints.as_slice())
            .fetch_one(&mut *tx)
            .await?;
    if found != 2 {
        return Err(KgError::EndpointNotFound);
    }
    let id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO kg_edges (tenant_id, from_id, to_id, relation, properties)
           VALUES ($1, $2, $3, $4, $5)
           ON CONFLICT (tenant_id, from_id, relation, to_id) DO UPDATE
              SET properties = EXCLUDED.properties
           RETURNING id"#,
    )
    .bind(tenant_id.0)
    .bind(edge.from_id)
    .bind(edge.to_id)
    .bind(&edge.relation)
    .bind(&edge.properties)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// All nodes within `depth` hops of `node_id` (edges traversed both ways) and
/// the edges between them. Depth is clamped to 1..=5.
pub async fn neighbors(
    pool: &TenantPool,
    tenant_id: TenantId,
    node_id: Uuid,
    depth: i32,
) -> Result<(Vec<KgNode>, Vec<KgEdge>), KgError> {
    // shortcut: depth capped at 5, raise if the agent ever needs deeper walks.
    let depth = depth.clamp(1, 5);
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let ids: Vec<Uuid> = sqlx::query_scalar(
        r#"WITH RECURSIVE reach(id, depth) AS (
               SELECT $2::uuid, 0
               UNION
               SELECT CASE WHEN e.from_id = r.id THEN e.to_id ELSE e.from_id END,
                      r.depth + 1
                 FROM reach r
                 JOIN kg_edges e
                   ON e.tenant_id = $1 AND (e.from_id = r.id OR e.to_id = r.id)
                WHERE r.depth < $3
           )
           SELECT DISTINCT id FROM reach"#,
    )
    .bind(tenant_id.0)
    .bind(node_id)
    .bind(depth)
    .fetch_all(&mut *tx)
    .await?;

    let node_rows = sqlx::query(
        r#"SELECT id, class, key, label_th, label_en, properties
             FROM kg_nodes
            WHERE tenant_id = $1 AND id = ANY($2)
            ORDER BY class, key"#,
    )
    .bind(tenant_id.0)
    .bind(ids.as_slice())
    .fetch_all(&mut *tx)
    .await?;
    let edge_rows = sqlx::query(
        r#"SELECT id, from_id, to_id, relation, properties
             FROM kg_edges
            WHERE tenant_id = $1 AND from_id = ANY($2) AND to_id = ANY($2)
            ORDER BY relation, id"#,
    )
    .bind(tenant_id.0)
    .bind(ids.as_slice())
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    let nodes = node_rows.iter().map(map_node).collect::<Result<_, _>>()?;
    let edges = edge_rows.iter().map(map_edge).collect::<Result<_, _>>()?;
    Ok((nodes, edges))
}

/// Nodes matching a label/key substring and optional class, newest first.
pub async fn list_nodes(
    pool: &TenantPool,
    tenant_id: TenantId,
    q: Option<&str>,
    class: Option<&str>,
    limit: i64,
) -> Result<Vec<KgNode>, KgError> {
    // shortcut: fixed 200-row cap, add paging if the inspector ever needs more.
    let limit = limit.clamp(1, 200);
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT id, class, key, label_th, label_en, properties
             FROM kg_nodes
            WHERE tenant_id = $1
              AND ($2::text IS NULL
                   OR label_th ILIKE '%' || $2 || '%'
                   OR label_en ILIKE '%' || $2 || '%'
                   OR key ILIKE '%' || $2 || '%')
              AND ($3::text IS NULL OR class = $3)
            ORDER BY updated_at DESC
            LIMIT $4"#,
    )
    .bind(tenant_id.0)
    .bind(q)
    .bind(class)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.iter().map(map_node).collect()
}

/// Node count per class, with classes that have no rows reported as 0.
pub async fn counts(
    pool: &TenantPool,
    tenant_id: TenantId,
) -> Result<BTreeMap<NodeClass, i64>, KgError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        "SELECT class, count(*) AS n FROM kg_nodes WHERE tenant_id = $1 GROUP BY class",
    )
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    let mut counts: BTreeMap<NodeClass, i64> =
        NodeClass::ALL.into_iter().map(|class| (class, 0)).collect();
    for row in rows {
        let class: String = row.try_get("class")?;
        counts.insert(class.parse()?, row.try_get("n")?);
    }
    Ok(counts)
}

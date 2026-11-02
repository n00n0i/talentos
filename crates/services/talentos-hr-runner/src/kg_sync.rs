//! Idempotent rebuild of the organizational knowledge graph (design §7).
//!
//! One tenant transaction, capped at [`NODE_CAP`] nodes. Every source table is
//! best-effort: a missing/empty table contributes nothing and never fails the
//! rebuild. Embeddings stay NULL until a sync worker has a model credential.
//!
//! Edges come from the authored taxonomy in design §5, not from inference.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use serde_json::{json, Value};
use sqlx::{Postgres, Row, Transaction};
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

use crate::kg::{KgError, NodeClass};

/// Design §7: a synchronous rebuild is capped so one big tenant cannot stall it.
pub const NODE_CAP: i64 = 5_000;

#[derive(Debug, Clone, Serialize)]
pub struct RebuildReport {
    pub nodes: BTreeMap<NodeClass, i64>,
    pub edges: i64,
    /// True when the node cap was reached and some source rows were skipped.
    pub capped: bool,
}

#[allow(clippy::too_many_arguments)]
async fn put_node(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    class: NodeClass,
    key: &str,
    label_th: &str,
    label_en: Option<&str>,
    properties: Value,
    budget: &mut i64,
) -> Result<Option<Uuid>, KgError> {
    if *budget <= 0 {
        return Ok(None);
    }
    *budget -= 1;
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
    .bind(tenant_id)
    .bind(class.as_str())
    .bind(key)
    .bind(label_th)
    .bind(label_en)
    .bind(properties)
    .fetch_one(&mut **tx)
    .await?;
    Ok(Some(id))
}

async fn put_edge(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    from_id: Uuid,
    relation: &str,
    to_id: Uuid,
) -> Result<(), KgError> {
    sqlx::query(
        r#"INSERT INTO kg_edges (tenant_id, from_id, to_id, relation)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (tenant_id, from_id, relation, to_id) DO NOTHING"#,
    )
    .bind(tenant_id)
    .bind(from_id)
    .bind(to_id)
    .bind(relation)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Rebuild the tenant's graph from existing HR tables. Re-running it leaves the
/// node and edge counts unchanged. Stale `sync` rows are intentionally kept:
/// pruning is a follow-up, and deleting would also hit manually authored nodes.
pub async fn rebuild(pool: &TenantPool, tenant_id: TenantId) -> Result<RebuildReport, KgError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let tenant = tenant_id.0;
    let mut budget = NODE_CAP;

    // Organization: one per tenant.
    let org_name: Option<String> = sqlx::query_scalar("SELECT name FROM tenants WHERE id = $1")
        .bind(tenant)
        .fetch_optional(&mut *tx)
        .await?;
    let org_name = org_name.unwrap_or_else(|| "Organization".to_string());
    let org = put_node(
        &mut tx,
        tenant,
        NodeClass::Organization,
        &tenant.to_string(),
        &org_name,
        None,
        json!({ "name": org_name }),
        &mut budget,
    )
    .await?
    .expect("first node never hits the cap");

    // LegalEntity from active entities.
    let entities = sqlx::query(
        "SELECT id, name, tax_id FROM entities
          WHERE tenant_id = $1 AND is_active
          ORDER BY name, id",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    let mut entity_ids: HashMap<Uuid, Uuid> = HashMap::new();
    for row in &entities {
        let id: Uuid = row.try_get("id")?;
        let name: String = row.try_get("name")?;
        let tax_id: Option<String> = row.try_get("tax_id")?;
        let node = put_node(
            &mut tx,
            tenant,
            NodeClass::LegalEntity,
            &id.to_string(),
            &name,
            None,
            json!({ "tax_id": tax_id }),
            &mut budget,
        )
        .await?;
        if let Some(node) = node {
            entity_ids.insert(id, node);
            put_edge(&mut tx, tenant, org, "has_entity", node).await?;
        }
    }

    // Department + Position are distinct values on employees; no source table.
    let departments: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT department FROM employees
          WHERE tenant_id = $1 AND department IS NOT NULL AND department <> ''
          ORDER BY department",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    let mut department_ids: HashMap<String, Uuid> = HashMap::new();
    for name in &departments {
        if let Some(node) = put_node(
            &mut tx,
            tenant,
            NodeClass::Department,
            name,
            name,
            None,
            json!({}),
            &mut budget,
        )
        .await?
        {
            department_ids.insert(name.clone(), node);
        }
    }

    let positions: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT position_title FROM employees
          WHERE tenant_id = $1 AND position_title <> ''
          ORDER BY position_title",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    let mut position_ids: HashMap<String, Uuid> = HashMap::new();
    for title in &positions {
        if let Some(node) = put_node(
            &mut tx,
            tenant,
            NodeClass::Position,
            title,
            title,
            None,
            json!({}),
            &mut budget,
        )
        .await?
        {
            position_ids.insert(title.clone(), node);
        }
    }

    let entity_departments = sqlx::query(
        "SELECT DISTINCT entity_id, department FROM employees
          WHERE tenant_id = $1 AND department IS NOT NULL AND department <> ''",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    for row in &entity_departments {
        let entity_id: Uuid = row.try_get("entity_id")?;
        let department: String = row.try_get("department")?;
        if let (Some(from), Some(to)) =
            (entity_ids.get(&entity_id), department_ids.get(&department))
        {
            put_edge(&mut tx, tenant, *from, "has_department", *to).await?;
        }
    }

    let department_positions = sqlx::query(
        "SELECT DISTINCT department, position_title FROM employees
          WHERE tenant_id = $1 AND department IS NOT NULL AND department <> ''
            AND position_title <> ''",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    for row in &department_positions {
        let department: String = row.try_get("department")?;
        let title: String = row.try_get("position_title")?;
        if let (Some(from), Some(to)) = (department_ids.get(&department), position_ids.get(&title))
        {
            put_edge(&mut tx, tenant, *from, "has_position", *to).await?;
        }
    }

    // Employee + hierarchy edges.
    let employees = sqlx::query(
        "SELECT id, employee_code, first_name_th, last_name_th, first_name_en,
                last_name_en, department, position_title, status, hire_date::text AS hire_date
           FROM employees
          WHERE tenant_id = $1
          ORDER BY employee_code, id",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    let mut employee_ids: HashMap<Uuid, Uuid> = HashMap::new();
    for row in &employees {
        let id: Uuid = row.try_get("id")?;
        let code: String = row.try_get("employee_code")?;
        let first_th: String = row.try_get("first_name_th")?;
        let last_th: String = row.try_get("last_name_th")?;
        let first_en: Option<String> = row.try_get("first_name_en")?;
        let last_en: Option<String> = row.try_get("last_name_en")?;
        let department: Option<String> = row.try_get("department")?;
        let title: String = row.try_get("position_title")?;
        let status: String = row.try_get("status")?;
        let hire_date: Option<String> = row.try_get("hire_date")?;
        let label_en = first_en
            .zip(last_en)
            .map(|(first, last)| format!("{first} {last}"));
        let node = put_node(
            &mut tx,
            tenant,
            NodeClass::Employee,
            &id.to_string(),
            &format!("{first_th} {last_th}"),
            label_en.as_deref(),
            json!({
                "code": code,
                "status": status,
                "hire_date": hire_date,
                "department": department,
                "position": title,
            }),
            &mut budget,
        )
        .await?;
        let Some(node) = node else { continue };
        employee_ids.insert(id, node);
        if let Some(department) = department.as_ref() {
            if let Some(dept) = department_ids.get(department) {
                put_edge(&mut tx, tenant, node, "member_of", *dept).await?;
            }
        }
        if let Some(position) = position_ids.get(&title) {
            put_edge(&mut tx, tenant, *position, "held_by", node).await?;
        }
    }

    // Skill from training records.
    let training = sqlx::query(
        "SELECT employee_id, title FROM employee_training_records
          WHERE tenant_id = $1 AND title <> ''
          ORDER BY title",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    let mut skill_ids: HashMap<String, Uuid> = HashMap::new();
    for row in &training {
        let employee_id: Uuid = row.try_get("employee_id")?;
        let title: String = row.try_get("title")?;
        let skill = match skill_ids.get(&title) {
            Some(id) => Some(*id),
            None => {
                let node = put_node(
                    &mut tx,
                    tenant,
                    NodeClass::Skill,
                    &title,
                    &title,
                    None,
                    json!({ "category": "training" }),
                    &mut budget,
                )
                .await?;
                if let Some(node) = node {
                    skill_ids.insert(title.clone(), node);
                }
                node
            }
        };
        if let (Some(employee), Some(skill)) = (employee_ids.get(&employee_id), skill) {
            put_edge(&mut tx, tenant, *employee, "has_skill", skill).await?;
        }
    }

    // Policy from tenant-authored leave types (tenant_id IS NULL are statutory
    // defaults, not this tenant's authored policy).
    let policies = sqlx::query(
        "SELECT name, label_th, label_en, max_days_per_year::text AS max_days
           FROM leave_types WHERE tenant_id = $1 ORDER BY name",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    for row in &policies {
        let name: String = row.try_get("name")?;
        let label_th: String = row.try_get("label_th")?;
        let label_en: Option<String> = row.try_get("label_en")?;
        let max_days: Option<String> = row.try_get("max_days")?;
        if let Some(node) = put_node(
            &mut tx,
            tenant,
            NodeClass::Policy,
            &name,
            &label_th,
            label_en.as_deref(),
            json!({ "kind": "leave", "max_days_per_year": max_days }),
            &mut budget,
        )
        .await?
        {
            put_edge(&mut tx, tenant, node, "applies_to", org).await?;
        }
    }

    // PayrollItem from payslip lines.
    let items = sqlx::query(
        "SELECT DISTINCT pl.code, pl.name, pl.line_type
           FROM payslip_lines pl
           JOIN payslips ps ON ps.id = pl.payslip_id
           JOIN payroll_runs pr ON pr.id = ps.payroll_run_id
          WHERE pr.tenant_id = $1
          ORDER BY pl.code",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    let mut item_ids: HashMap<String, Uuid> = HashMap::new();
    for row in &items {
        let code: String = row.try_get("code")?;
        let name: String = row.try_get("name")?;
        let line_type: String = row.try_get("line_type")?;
        if let Some(node) = put_node(
            &mut tx,
            tenant,
            NodeClass::PayrollItem,
            &code,
            &name,
            None,
            json!({ "kind": line_type }),
            &mut budget,
        )
        .await?
        {
            item_ids.insert(code, node);
        }
    }

    // Cycle from payroll runs.
    let cycles = sqlx::query(
        "SELECT id, period_start::text AS period_start, period_end::text AS period_end, status
           FROM payroll_runs WHERE tenant_id = $1 ORDER BY period_start",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    let mut cycle_ids: HashMap<Uuid, Uuid> = HashMap::new();
    for row in &cycles {
        let id: Uuid = row.try_get("id")?;
        let start: String = row.try_get("period_start")?;
        let end: String = row.try_get("period_end")?;
        let status: String = row.try_get("status")?;
        if let Some(node) = put_node(
            &mut tx,
            tenant,
            NodeClass::Cycle,
            &id.to_string(),
            &format!("{start} – {end}"),
            None,
            json!({ "period_start": start, "period_end": end, "status": status }),
            &mut budget,
        )
        .await?
        {
            cycle_ids.insert(id, node);
        }
    }

    // Cycle --contains--> PayrollItem.
    let cycle_items = sqlx::query(
        "SELECT DISTINCT pr.id AS run_id, pl.code
           FROM payroll_runs pr
           JOIN payslips ps ON ps.payroll_run_id = pr.id
           JOIN payslip_lines pl ON pl.payslip_id = ps.id
          WHERE pr.tenant_id = $1",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    for row in &cycle_items {
        let run_id: Uuid = row.try_get("run_id")?;
        let code: String = row.try_get("code")?;
        if let (Some(cycle), Some(item)) = (cycle_ids.get(&run_id), item_ids.get(&code)) {
            put_edge(&mut tx, tenant, *cycle, "contains", *item).await?;
        }
    }

    // Document from employee documents.
    let documents = sqlx::query(
        "SELECT id, employee_id, title, doc_type, expiry_date::text AS expiry_date
           FROM employee_documents WHERE tenant_id = $1 ORDER BY created_at",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    for row in &documents {
        let id: Uuid = row.try_get("id")?;
        let employee_id: Uuid = row.try_get("employee_id")?;
        let title: String = row.try_get("title")?;
        let doc_type: String = row.try_get("doc_type")?;
        let expiry_date: Option<String> = row.try_get("expiry_date")?;
        if let Some(node) = put_node(
            &mut tx,
            tenant,
            NodeClass::Document,
            &id.to_string(),
            &title,
            None,
            json!({ "doc_type": doc_type, "expiry": expiry_date }),
            &mut budget,
        )
        .await?
        {
            if let Some(employee) = employee_ids.get(&employee_id) {
                put_edge(&mut tx, tenant, node, "belongs_to", *employee).await?;
            }
        }
    }

    // Benefit and Process have no source table in v1; they stay empty until
    // authored seeds land (design §7 step 3).

    let node_rows = sqlx::query(
        "SELECT class, count(*) AS n FROM kg_nodes WHERE tenant_id = $1 GROUP BY class",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    let mut nodes: BTreeMap<NodeClass, i64> =
        NodeClass::ALL.into_iter().map(|class| (class, 0)).collect();
    for row in &node_rows {
        let class: String = row.try_get("class")?;
        nodes.insert(class.parse()?, row.try_get("n")?);
    }
    let edges: i64 = sqlx::query_scalar("SELECT count(*) FROM kg_edges WHERE tenant_id = $1")
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(RebuildReport {
        nodes,
        edges,
        capped: budget == 0,
    })
}

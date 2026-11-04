//! Read-only direct-report directory for line managers. Deliberately exposes a
//! small, non-payroll field set; salary, bank, national id, SSO number and other
//! sensitive columns never leave the database through this module.

use serde::Serialize;
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct TeamMemberView {
    pub id: Uuid,
    pub employee_code: String,
    pub employee_name: String,
    pub position_title: String,
    pub department: Option<String>,
    pub status: String,
}

/// Employees whose `manager_id` points at the caller's employee record.
/// Tenant scoping is both `current_tenant_id()` and the explicit `tenant_id`.
pub async fn list_my_team(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
) -> Result<Vec<TeamMemberView>, sqlx::Error> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT e2.id, e2.employee_code, e2.first_name_th, e2.last_name_th,
                  e2.position_title, e2.department, e2.status
             FROM employees e1
             JOIN employees e2
               ON e2.manager_id = e1.id
              AND e2.tenant_id = e1.tenant_id
            WHERE e1.tenant_id = $1
              AND e1.user_id = $2
              AND e2.status <> 'terminated'
            ORDER BY e2.employee_code"#,
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.iter()
        .map(|row| {
            Ok(TeamMemberView {
                id: row.try_get("id")?,
                employee_code: row.try_get("employee_code")?,
                employee_name: format!(
                    "{} {}",
                    row.try_get::<String, _>("first_name_th")?,
                    row.try_get::<String, _>("last_name_th")?
                ),
                position_title: row.try_get("position_title")?,
                department: row.try_get("department")?,
                status: row.try_get("status")?,
            })
        })
        .collect()
}

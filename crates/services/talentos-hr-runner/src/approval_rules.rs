//! Conditional leave approval rules: route a leave request to an approval chain
//! by leave type and/or minimum days. Selected at request creation; rules can be
//! listed, created and deleted by an HR admin (tenant-scoped via RLS).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

use super::leave::LEAVE_TYPES;

#[derive(Debug, thiserror::Error)]
pub enum ApprovalRuleError {
    #[error("leave_type must be a known leave type")]
    InvalidType,
    #[error("min_days must be greater than 0 and at most 365")]
    InvalidMinDays,
    #[error(
        "chain must be 1..3 roles from manager/hr_admin with no duplicates, ending in hr_admin"
    )]
    InvalidChain,
    #[error("approval rule not found")]
    NotFound,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Serialize)]
pub struct ApprovalRuleView {
    pub id: i64,
    pub leave_type: Option<String>,
    pub min_days: Option<Decimal>,
    pub chain: Vec<String>,
    pub priority: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalRuleInput {
    /// `None` matches every leave type.
    pub leave_type: Option<String>,
    /// `None` matches every duration.
    pub min_days: Option<Decimal>,
    pub chain: Vec<String>,
    #[serde(default)]
    pub priority: Option<i32>,
}

fn valid_chain(chain: &[String]) -> bool {
    let n = chain.len();
    let valid_roles = ["manager", "hr_admin"];
    let unique: std::collections::HashSet<_> = chain.iter().collect();
    n > 0
        && n <= 3
        && unique.len() == n
        && chain.iter().all(|r| valid_roles.contains(&r.as_str()))
        && chain.last().map(String::as_str) == Some("hr_admin")
}

fn validate(input: &ApprovalRuleInput) -> Result<(), ApprovalRuleError> {
    if let Some(leave_type) = input.leave_type.as_deref() {
        if !LEAVE_TYPES.contains(&leave_type) {
            return Err(ApprovalRuleError::InvalidType);
        }
    }
    if let Some(min_days) = input.min_days {
        if min_days <= Decimal::ZERO || min_days > Decimal::from(365) {
            return Err(ApprovalRuleError::InvalidMinDays);
        }
    }
    if !valid_chain(&input.chain) {
        return Err(ApprovalRuleError::InvalidChain);
    }
    Ok(())
}

/// All approval rules for the tenant, most specific first.
pub async fn list_rules(
    pool: &TenantPool,
    tenant_id: TenantId,
) -> Result<Vec<ApprovalRuleView>, ApprovalRuleError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT id, leave_type, min_days, chain, priority, created_at
             FROM leave_approval_rules
            WHERE tenant_id = $1
            ORDER BY (leave_type IS NOT NULL) DESC,
                     (min_days IS NOT NULL) DESC,
                     priority DESC,
                     id"#,
    )
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.iter()
        .map(|row| {
            Ok(ApprovalRuleView {
                id: row.try_get("id")?,
                leave_type: row.try_get("leave_type")?,
                min_days: row.try_get("min_days")?,
                chain: row.try_get("chain")?,
                priority: row.try_get("priority")?,
                created_at: row.try_get("created_at")?,
            })
        })
        .collect()
}

/// Create an approval rule.
pub async fn create_rule(
    pool: &TenantPool,
    tenant_id: TenantId,
    created_by: Uuid,
    input: ApprovalRuleInput,
) -> Result<ApprovalRuleView, ApprovalRuleError> {
    validate(&input)?;

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(
        r#"INSERT INTO leave_approval_rules
             (tenant_id, leave_type, min_days, chain, priority, created_by)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, leave_type, min_days, chain, priority, created_at"#,
    )
    .bind(tenant_id.0)
    .bind(&input.leave_type)
    .bind(input.min_days)
    .bind(&input.chain)
    .bind(input.priority.unwrap_or(0))
    .bind(created_by)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(ApprovalRuleView {
        id: row.try_get("id")?,
        leave_type: row.try_get("leave_type")?,
        min_days: row.try_get("min_days")?,
        chain: row.try_get("chain")?,
        priority: row.try_get("priority")?,
        created_at: row.try_get("created_at")?,
    })
}

/// Delete an approval rule.
pub async fn delete_rule(
    pool: &TenantPool,
    tenant_id: TenantId,
    id: i64,
) -> Result<(), ApprovalRuleError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let deleted = sqlx::query("DELETE FROM leave_approval_rules WHERE id = $1 AND tenant_id = $2")
        .bind(id)
        .bind(tenant_id.0)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if deleted.rows_affected() == 0 {
        return Err(ApprovalRuleError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_validation_matches_default_rules() {
        assert!(valid_chain(&["hr_admin".to_string()]));
        assert!(valid_chain(&[
            "manager".to_string(),
            "hr_admin".to_string()
        ]));
        assert!(!valid_chain(&[]));
        assert!(!valid_chain(&["manager".to_string()]));
        assert!(!valid_chain(&[
            "manager".to_string(),
            "manager".to_string(),
            "hr_admin".to_string()
        ]));
        assert!(!valid_chain(&[
            "hr_admin".to_string(),
            "manager".to_string()
        ]));
    }
}

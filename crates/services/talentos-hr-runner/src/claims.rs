//! Expense claims: self-service submission with receipt upload, and HR/manager approval.
//!
//! Reimbursement through payroll is deferred to a future slice.

use std::path::PathBuf;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

const DESCRIPTION_MAX_CHARS: usize = 500;
const VALID_CATEGORIES: &[&str] = &["travel", "meal", "supplies", "other"];
const VALID_STATUSES: &[&str] = &["pending", "approved", "rejected", "cancelled"];
const ALLOWED_RECEIPT_TYPES: &[&str] = &["image/jpeg", "image/png", "application/pdf"];
const MAX_RECEIPT_BYTES: usize = 5 * 1024 * 1024;

/// Directory for claim receipt files; overridable via `CLAIM_UPLOAD_DIR`.
pub fn upload_dir() -> PathBuf {
    std::env::var("CLAIM_UPLOAD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/talentos-claims"))
}

#[derive(Debug, thiserror::Error)]
pub enum ClaimError {
    #[error("employee not found for this tenant")]
    EmployeeNotFound,
    #[error("expense claim not found")]
    NotFound,
    #[error("expense claim is not in pending status")]
    NotPending,
    #[error("expense claim cannot be cancelled in its current status")]
    NotCancellable,
    #[error("category must be one of: travel, meal, supplies, other")]
    InvalidCategory,
    #[error("amount must be greater than 0")]
    InvalidAmount,
    #[error("expense_date is required")]
    InvalidDate,
    #[error("description must be at most 500 characters")]
    InvalidDescription,
    #[error("invalid status filter")]
    InvalidStatus,
    #[error("receipt error: {0}")]
    Receipt(#[from] ReceiptError),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum ReceiptError {
    #[error("receipt is too large; maximum size is 5 MB")]
    TooLarge,
    #[error("receipt type must be image/jpeg, image/png, or application/pdf")]
    InvalidType,
    #[error("receipt already set")]
    AlreadySet,
    #[error("receipt not found")]
    NotFound,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaimInput {
    pub category: String,
    pub amount: Decimal,
    pub expense_date: NaiveDate,
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ClaimView {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub category: String,
    pub amount: Decimal,
    pub expense_date: NaiveDate,
    pub description: Option<String>,
    pub status: String,
    pub has_receipt: bool,
}

#[derive(Debug, Serialize)]
pub struct PendingClaim {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub employee_name: String,
    pub category: String,
    pub amount: Decimal,
    pub expense_date: NaiveDate,
    pub description: Option<String>,
    pub has_receipt: bool,
}

fn validate_input(input: &ClaimInput) -> Result<(), ClaimError> {
    if !VALID_CATEGORIES.contains(&input.category.as_str()) {
        return Err(ClaimError::InvalidCategory);
    }
    if input.amount <= Decimal::ZERO {
        return Err(ClaimError::InvalidAmount);
    }
    if input.expense_date == NaiveDate::MIN {
        return Err(ClaimError::InvalidDate);
    }
    if let Some(ref d) = input.description {
        if d.chars().count() > DESCRIPTION_MAX_CHARS {
            return Err(ClaimError::InvalidDescription);
        }
    }
    Ok(())
}

/// Create an expense claim for the caller's linked employee.
pub async fn create_claim(
    pool: &TenantPool,
    tenant_id: TenantId,
    caller_user_id: Uuid,
    input: ClaimInput,
) -> Result<ClaimView, ClaimError> {
    validate_input(&input)?;

    let mut tx = pool.begin_tenant(tenant_id).await?;

    let employee = sqlx::query(
        "SELECT id, entity_id FROM employees WHERE tenant_id = $1 AND user_id = $2 LIMIT 1",
    )
    .bind(tenant_id.0)
    .bind(caller_user_id)
    .fetch_optional(&mut *tx)
    .await?;

    let employee = employee.ok_or(ClaimError::EmployeeNotFound)?;
    let employee_id: Uuid = employee.try_get("id")?;
    let entity_id: Uuid = employee.try_get("entity_id")?;

    let row = sqlx::query(
        r#"INSERT INTO expense_claims
             (tenant_id, entity_id, employee_id, category, amount, expense_date,
              description, status)
           VALUES ($1,$2,$3,$4,$5,$6,$7,'pending')
           RETURNING id, status"#,
    )
    .bind(tenant_id.0)
    .bind(entity_id)
    .bind(employee_id)
    .bind(&input.category)
    .bind(input.amount)
    .bind(input.expense_date)
    .bind(&input.description)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(ClaimView {
        id: row.try_get("id")?,
        employee_id,
        category: input.category,
        amount: input.amount,
        expense_date: input.expense_date,
        description: input.description,
        status: row.try_get("status")?,
        has_receipt: false,
    })
}

/// List expense claims for one employee.
pub async fn list_claims(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
    status: Option<&str>,
) -> Result<Vec<ClaimView>, ClaimError> {
    if let Some(s) = status {
        if !VALID_STATUSES.contains(&s) {
            return Err(ClaimError::InvalidStatus);
        }
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT id, employee_id, category, amount, expense_date, description, status,
                  (receipt_path IS NOT NULL) AS has_receipt
           FROM expense_claims
           WHERE tenant_id = $1 AND employee_id = $2
             AND ($3::text IS NULL OR status = $3)
           ORDER BY (status = 'pending') DESC, expense_date DESC, created_at DESC
           LIMIT 200"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(status)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.into_iter()
        .map(|row| {
            Ok(ClaimView {
                id: row.try_get("id")?,
                employee_id: row.try_get("employee_id")?,
                category: row.try_get("category")?,
                amount: row.try_get("amount")?,
                expense_date: row.try_get("expense_date")?,
                description: row.try_get("description")?,
                status: row.try_get("status")?,
                has_receipt: row.try_get("has_receipt")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

/// Cancel an expense claim. Only the owning employee may cancel, and only
/// claims in `pending` status.
pub async fn cancel_claim(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    claim_id: Uuid,
) -> Result<String, ClaimError> {
    let employee_id = match employee_of_user(pool, tenant_id, user_id).await? {
        Some(id) => id,
        None => return Err(ClaimError::NotFound),
    };

    let mut tx = pool.begin_tenant(tenant_id).await?;

    let updated = sqlx::query(
        r#"UPDATE expense_claims
           SET status = 'cancelled', updated_at = now()
           WHERE id = $1 AND tenant_id = $2 AND employee_id = $3 AND status = 'pending'
           RETURNING status"#,
    )
    .bind(claim_id)
    .bind(tenant_id.0)
    .bind(employee_id)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(row) = updated {
        let status: String = row.try_get("status")?;
        tx.commit().await?;
        return Ok(status);
    }

    let row = sqlx::query(
        r#"SELECT status, employee_id FROM expense_claims
           WHERE id = $1 AND tenant_id = $2"#,
    )
    .bind(claim_id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?;

    let _ = tx.rollback().await;
    match row {
        Some(r) => {
            let owner: Uuid = r.try_get("employee_id")?;
            if owner != employee_id {
                Err(ClaimError::NotFound)
            } else {
                Err(ClaimError::NotCancellable)
            }
        }
        None => Err(ClaimError::NotFound),
    }
}

/// Approve or reject a pending expense claim.
pub async fn decide_claim(
    pool: &TenantPool,
    tenant_id: TenantId,
    approver_user_id: Uuid,
    claim_id: Uuid,
    approve: bool,
) -> Result<String, ClaimError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;

    let updated = sqlx::query(
        r#"UPDATE expense_claims
           SET status = $3, decided_by = $4, decided_at = now(), updated_at = now()
           WHERE id = $1 AND tenant_id = $2 AND status = 'pending'
           RETURNING status"#,
    )
    .bind(claim_id)
    .bind(tenant_id.0)
    .bind(if approve { "approved" } else { "rejected" })
    .bind(approver_user_id)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(row) = updated {
        let status: String = row.try_get("status")?;
        tx.commit().await?;
        return Ok(status);
    }

    let row = sqlx::query("SELECT status FROM expense_claims WHERE id = $1 AND tenant_id = $2")
        .bind(claim_id)
        .bind(tenant_id.0)
        .fetch_optional(&mut *tx)
        .await?;

    let _ = tx.rollback().await;
    match row {
        Some(_) => Err(ClaimError::NotPending),
        None => Err(ClaimError::NotFound),
    }
}

/// All pending expense claims across the tenant, newest first.
pub async fn pending_approvals(
    pool: &TenantPool,
    tenant_id: TenantId,
) -> Result<Vec<PendingClaim>, ClaimError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT c.id, c.employee_id, e.employee_code, e.first_name_th, e.last_name_th,
                  c.category, c.amount, c.expense_date, c.description,
                  (c.receipt_path IS NOT NULL) AS has_receipt
           FROM expense_claims c
           JOIN employees e ON e.id = c.employee_id
           WHERE c.tenant_id = $1 AND c.status = 'pending'
           ORDER BY c.created_at DESC
           LIMIT 200"#,
    )
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.into_iter()
        .map(|row| {
            Ok(PendingClaim {
                id: row.try_get("id")?,
                employee_id: row.try_get("employee_id")?,
                employee_code: row.try_get("employee_code")?,
                employee_name: format!(
                    "{} {}",
                    row.try_get::<String, _>("first_name_th")?,
                    row.try_get::<String, _>("last_name_th")?
                ),
                category: row.try_get("category")?,
                amount: row.try_get("amount")?,
                expense_date: row.try_get("expense_date")?,
                description: row.try_get("description")?,
                has_receipt: row.try_get("has_receipt")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

/// Employee record owned by a login user (self-service default).
pub async fn employee_of_user(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
) -> Result<Option<Uuid>, ClaimError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query("SELECT id FROM employees WHERE tenant_id = $1 AND user_id = $2 LIMIT 1")
        .bind(tenant_id.0)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
    tx.commit().await?;
    row.map(|r| r.try_get("id")).transpose().map_err(Into::into)
}

/// Validate receipt bytes and content type, then persist to disk and update the claim row.
/// Only the owning employee may attach a receipt, and only while the claim is pending.
pub async fn set_receipt(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    claim_id: Uuid,
    content_type: &str,
    bytes: &[u8],
) -> Result<(), ClaimError> {
    if bytes.len() > MAX_RECEIPT_BYTES {
        return Err(ReceiptError::TooLarge.into());
    }
    if !ALLOWED_RECEIPT_TYPES.contains(&content_type) {
        return Err(ReceiptError::InvalidType.into());
    }

    let employee_id = match employee_of_user(pool, tenant_id, user_id).await? {
        Some(id) => id,
        None => return Err(ClaimError::NotFound),
    };

    let ext = match content_type {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "application/pdf" => "pdf",
        _ => return Err(ReceiptError::InvalidType.into()),
    };

    let mut tx = pool.begin_tenant(tenant_id).await?;

    // Guard ownership + pending status; reject replacing an existing receipt.
    let claim = sqlx::query(
        r#"SELECT employee_id, receipt_path FROM expense_claims
           WHERE id = $1 AND tenant_id = $2 FOR UPDATE"#,
    )
    .bind(claim_id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(claim) = claim else {
        let _ = tx.rollback().await;
        return Err(ClaimError::NotFound);
    };

    let owner: Uuid = claim.try_get("employee_id")?;
    if owner != employee_id {
        let _ = tx.rollback().await;
        return Err(ClaimError::NotFound);
    }

    let existing: Option<String> = claim.try_get("receipt_path")?;
    if existing.is_some() {
        let _ = tx.rollback().await;
        return Err(ReceiptError::AlreadySet.into());
    }

    let filename = format!("{}.{}", Uuid::new_v4(), ext);
    let dir = upload_dir();
    let path = dir.join(&filename);

    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::write(&path, bytes).await?;

    let updated = sqlx::query(
        r#"UPDATE expense_claims
           SET receipt_path = $3, receipt_type = $4, updated_at = now()
           WHERE id = $1 AND tenant_id = $2 AND status = 'pending'
           RETURNING id"#,
    )
    .bind(claim_id)
    .bind(tenant_id.0)
    .bind(path.to_string_lossy().to_string())
    .bind(content_type)
    .fetch_optional(&mut *tx)
    .await?;

    if updated.is_none() {
        // Pending-only upload: the claim was decided between read and write.
        let _ = tokio::fs::remove_file(&path).await;
        let _ = tx.rollback().await;
        return Err(ClaimError::NotPending);
    }

    tx.commit().await?;
    Ok(())
}

/// Receipt payload returned by [`download_receipt`].
pub struct ReceiptFile {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// Download a receipt. The caller must have already verified ownership or HR permission.
pub async fn download_receipt(
    pool: &TenantPool,
    tenant_id: TenantId,
    claim_id: Uuid,
) -> Result<Option<ReceiptFile>, ClaimError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(
        "SELECT receipt_path, receipt_type FROM expense_claims WHERE id = $1 AND tenant_id = $2",
    )
    .bind(claim_id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;

    let Some(row) = row else { return Ok(None) };
    let path: Option<String> = row.try_get("receipt_path")?;
    let content_type: Option<String> = row.try_get("receipt_type")?;

    match (path, content_type) {
        (Some(path), Some(content_type)) => {
            let bytes = match tokio::fs::read(&path).await {
                Ok(b) => b,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            Ok(Some(ReceiptFile {
                content_type,
                bytes,
            }))
        }
        _ => Ok(None),
    }
}

/// Returns the employee_id of the claim owner when the claim exists in this tenant.
pub async fn owner_of_claim(
    pool: &TenantPool,
    tenant_id: TenantId,
    claim_id: Uuid,
) -> Result<Option<Uuid>, ClaimError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row =
        sqlx::query("SELECT employee_id FROM expense_claims WHERE id = $1 AND tenant_id = $2")
            .bind(claim_id)
            .bind(tenant_id.0)
            .fetch_optional(&mut *tx)
            .await?;
    tx.commit().await?;
    row.map(|r| r.try_get("employee_id"))
        .transpose()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_allowlist() {
        assert!(VALID_CATEGORIES.contains(&"travel"));
        assert!(VALID_CATEGORIES.contains(&"meal"));
        assert!(!VALID_CATEGORIES.contains(&"hotel"));
    }

    #[test]
    fn receipt_type_allowlist() {
        assert!(ALLOWED_RECEIPT_TYPES.contains(&"image/jpeg"));
        assert!(ALLOWED_RECEIPT_TYPES.contains(&"image/png"));
        assert!(ALLOWED_RECEIPT_TYPES.contains(&"application/pdf"));
        assert!(!ALLOWED_RECEIPT_TYPES.contains(&"image/gif"));
    }

    #[test]
    fn description_too_long_is_rejected() {
        let input = ClaimInput {
            category: "meal".into(),
            amount: Decimal::new(100, 0),
            expense_date: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            description: Some("x".repeat(501)),
        };
        assert!(matches!(
            validate_input(&input),
            Err(ClaimError::InvalidDescription)
        ));
    }

    #[test]
    fn zero_amount_is_rejected() {
        let input = ClaimInput {
            category: "meal".into(),
            amount: Decimal::ZERO,
            expense_date: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            description: None,
        };
        assert!(matches!(
            validate_input(&input),
            Err(ClaimError::InvalidAmount)
        ));
    }

    #[test]
    fn extension_by_content_type() {
        assert_eq!(ext_for_type("image/jpeg"), Some("jpg"));
        assert_eq!(ext_for_type("image/png"), Some("png"));
        assert_eq!(ext_for_type("application/pdf"), Some("pdf"));
        assert_eq!(ext_for_type("text/plain"), None);
    }

    fn ext_for_type(content_type: &str) -> Option<&'static str> {
        match content_type {
            "image/jpeg" => Some("jpg"),
            "image/png" => Some("png"),
            "application/pdf" => Some("pdf"),
            _ => None,
        }
    }
}

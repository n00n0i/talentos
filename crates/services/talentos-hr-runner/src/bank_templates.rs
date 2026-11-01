//! Per-bank payout file template registry.
//!
//! A row is one bank's transfer-file layout: the column order (`field_order`)
//! plus optional header labels (`header_row`). The payout export resolves each
//! source against the employee's pay record. Tenant rows are created via the
//! API (admin, audited); global seed rows are migration-only.
//!
//! All queries carry `tenant_id = $1` explicitly; writes use
//! `pool.begin_tenant(tenant_id)` so RLS sees the right tenant.

use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum BankTemplateError {
    #[error("unknown field order entry: {0}")]
    InvalidField(String),
    #[error("bank template {0} not found")]
    NotFound(Uuid),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// Values a bank payout column may pull from. Mirrors the `bank_payout`
/// source vocabulary in `forms::resolve_bank_value` (unknown sources resolve
/// to an empty cell, so the registry accepts any of these and rejects the
/// rest up front).
pub const PAYOUT_SOURCES: &[&str] = &[
    "employee_code",
    "employee_name",
    "bank_name",
    "bank_account",
    "net",
];

/// One bank payout file layout, as returned by `list_bank_templates` and
/// `create_bank_template`.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct BankTemplate {
    pub id: Uuid,
    pub bank_name: String,
    pub layout_name: String,
    pub field_order: Vec<String>,
    pub header_row: Option<String>,
    pub unverified: bool,
    pub sign_off_note: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// New bank payout layout. `header_row` is a comma-separated label list
/// aligned positionally with `field_order`; entries map to the `PAYOUT_SOURCES`
/// vocabulary.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct BankTemplateInput {
    pub bank_name: String,
    pub layout_name: String,
    pub field_order: Vec<String>,
    pub header_row: Option<String>,
    /// Defaults to true when omitted: every layout ships unverified until an
    /// accountant signs off against the bank's live spec.
    pub unverified: Option<bool>,
    pub sign_off_note: Option<String>,
}

/// Tenant's layouts (including global seeds) for one bank, or all banks when
/// `bank_name` is None. RLS: `tenant_id = $1` on the owning tenant.
pub async fn list_bank_templates(
    pool: &TenantPool,
    tenant_id: TenantId,
    bank_name: Option<&str>,
) -> Result<Vec<BankTemplate>, BankTemplateError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT id, bank_name, layout_name, field_order, header_row,
                  unverified, sign_off_note, created_at
             FROM bank_templates
            WHERE (tenant_id = $1 OR tenant_id IS NULL)
              AND ($2::TEXT IS NULL OR bank_name = $2)
            ORDER BY (tenant_id IS NOT NULL) DESC, created_at"#,
    )
    .bind(tenant_id.0)
    .bind(bank_name)
    .fetch_all(&mut *tx)
    .await?;
    let _ = tx.rollback().await;

    rows.iter()
        .map(|row| {
            Ok(BankTemplate {
                id: row.try_get("id")?,
                bank_name: row.try_get("bank_name")?,
                layout_name: row.try_get("layout_name")?,
                field_order: row.try_get("field_order")?,
                header_row: row.try_get("header_row")?,
                unverified: row.try_get("unverified")?,
                sign_off_note: row.try_get("sign_off_note")?,
                created_at: row.try_get("created_at")?,
            })
        })
        .collect()
}

/// Create a tenant layout and audit the action. `field_order` entries must be
/// in the payout source vocabulary; `unverified` defaults to true so nothing
/// ships as "verified" without an explicit flag.
pub async fn create_bank_template(
    pool: &TenantPool,
    tenant_id: TenantId,
    created_by: Uuid,
    input: BankTemplateInput,
) -> Result<BankTemplate, BankTemplateError> {
    if input.field_order.is_empty() {
        return Err(BankTemplateError::InvalidField(
            "field_order must not be empty".to_string(),
        ));
    }
    for field in &input.field_order {
        if !PAYOUT_SOURCES.contains(&field.as_str()) {
            return Err(BankTemplateError::InvalidField(field.clone()));
        }
    }
    let unverified = input.unverified.unwrap_or(true);

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(
        r#"INSERT INTO bank_templates
             (tenant_id, bank_name, layout_name, field_order, header_row,
              unverified, sign_off_note)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING id, bank_name, layout_name, field_order, header_row,
                     unverified, sign_off_note, created_at"#,
    )
    .bind(tenant_id.0)
    .bind(&input.bank_name)
    .bind(&input.layout_name)
    .bind(&input.field_order)
    .bind(&input.header_row)
    .bind(unverified)
    .bind(&input.sign_off_note)
    .fetch_one(&mut *tx)
    .await?;

    // Significant action: record who registered which layout.
    let audit_id: Uuid = row.try_get("id")?;
    sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'payout_template.create', 'bank_template', $3, $4)"#,
    )
    .bind(tenant_id.0)
    .bind(created_by.to_string())
    .bind(audit_id.to_string())
    .bind(serde_json::json!({
        "bank_name": input.bank_name,
        "layout_name": input.layout_name,
        "field_order": input.field_order,
        "unverified": unverified,
        "sign_off_note": input.sign_off_note,
    }))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(BankTemplate {
        id: row.try_get("id")?,
        bank_name: row.try_get("bank_name")?,
        layout_name: row.try_get("layout_name")?,
        field_order: row.try_get("field_order")?,
        header_row: row.try_get("header_row")?,
        unverified: row.try_get("unverified")?,
        sign_off_note: row.try_get("sign_off_note")?,
        created_at: row.try_get("created_at")?,
    })
}

/// Load one tenant-owned row by id. Global seeds are not creatable via the
/// API, so a missing row means either no such id or another tenant's row.
pub async fn get_bank_template(
    pool: &TenantPool,
    tenant_id: TenantId,
    id: Uuid,
) -> Result<BankTemplate, BankTemplateError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(
        r#"SELECT id, bank_name, layout_name, field_order, header_row,
                  unverified, sign_off_note, created_at
             FROM bank_templates
            WHERE id = $1 AND tenant_id = $2"#,
    )
    .bind(id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?;
    let _ = tx.rollback().await;

    let Some(row) = row else {
        return Err(BankTemplateError::NotFound(id));
    };
    Ok(BankTemplate {
        id: row.try_get("id")?,
        bank_name: row.try_get("bank_name")?,
        layout_name: row.try_get("layout_name")?,
        field_order: row.try_get("field_order")?,
        header_row: row.try_get("header_row")?,
        unverified: row.try_get("unverified")?,
        sign_off_note: row.try_get("sign_off_note")?,
        created_at: row.try_get("created_at")?,
    })
}

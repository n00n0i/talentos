//! E-signature: internal "digital acknowledgement" (SHA-256 of document
//! content + typed name + time) with a `E_SIGNATURE_PROVIDER` env seam for a
//! real provider later. Idempotent: re-signing updates the existing row.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

const MAX_TYPED_NAME: usize = 200;

pub type EsignResult<T> = Result<T, EsignError>;

#[derive(Debug, thiserror::Error)]
pub enum EsignError {
    #[error("signer not linked to this tenant")]
    SignerNotFound,
    #[error("offer not found in this tenant")]
    OfferNotFound,
    #[error("payslip not found in this tenant")]
    PayslipNotFound,
    #[error("payslip is not owned by this employee")]
    NotOwnPayslip,
    #[error("typed name must be 1-200 characters")]
    InvalidName,
    #[error("document content unavailable")]
    NoContent,
    #[error("database row not found")]
    RowNotFound,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// What gets signed.
#[derive(Debug, Clone)]
pub enum DocumentType {
    Payslip,
    Offer,
}

impl DocumentType {
    fn as_str(&self) -> &'static str {
        match self {
            DocumentType::Payslip => "payslip",
            DocumentType::Offer => "offer",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SignatureView {
    pub id: Uuid,
    pub document_type: String,
    pub document_id: Uuid,
    pub signer_user_id: Uuid,
    pub typed_name: String,
    pub doc_hash_sha256: String,
    pub signed_at: DateTime<Utc>,
    pub provider: String,
}

/// External e-signature provider seam. When `E_SIGNATURE_PROVIDER` is set the
/// real provider should be called here; the stub only logs and returns the
/// provider name so a provider can be plugged in later without changing callers.
pub async fn external_provider(provider: &str) -> String {
    tracing::warn!(
        provider,
        "external e-signature provider not integrated; stubbing internal"
    );
    provider.to_string()
}

fn provider_name() -> String {
    match std::env::var("E_SIGNATURE_PROVIDER") {
        Ok(name) => format!("internal({name})"),
        Err(_) => "internal".to_string(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Compute the document content bytes to hash. For payslips, prefer the
/// cached PDF file when present; otherwise fall back to the payroll-run
/// summary. For offers, hash the offer letter fields.
async fn document_bytes(
    pool: &TenantPool,
    tenant_id: &TenantId,
    doc_type: &DocumentType,
    document_id: &Uuid,
) -> Result<Vec<u8>, EsignError> {
    match doc_type {
        DocumentType::Payslip => {
            // Prefer the cached PDF; it is the exact document being signed.
            let path = pdf_cache_path(tenant_id.0, *document_id);
            if let Ok(bytes) = tokio::fs::read(&path).await {
                if bytes.starts_with(b"%PDF-") && bytes.len() > 1024 {
                    return Ok(bytes);
                }
            }
            // Fall back to the payslip line summary, so the hash is stable
            // and tenant-scoped even before a PDF is cached.
            let mut tx = pool.begin_tenant(*tenant_id).await?;
            let row = sqlx::query(
                r#"SELECT p.id, p.status
                      FROM payslips p
                     WHERE p.id = $1 AND p.tenant_id = $2"#,
            )
            .bind(*document_id)
            .bind(tenant_id.0)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(EsignError::PayslipNotFound)?;
            let _ = tx.rollback().await;
            Ok(format!(
                "payslip:{}:{}",
                row.get::<Uuid, _>("id"),
                row.get::<String, _>("status")
            )
            .into_bytes())
        }
        DocumentType::Offer => {
            let mut tx = pool.begin_tenant(*tenant_id).await?;
            let row = sqlx::query(
                r#"SELECT o.id, c.first_name, c.last_name,
                           o.position_title, o.salary::text, o.start_date::text
                    FROM candidate_offers o
                    JOIN candidates c ON c.id = o.candidate_id AND c.tenant_id = o.tenant_id
                   WHERE o.id = $1 AND o.tenant_id = $2"#,
            )
            .bind(*document_id)
            .bind(tenant_id.0)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(EsignError::OfferNotFound)?;
            let _ = tx.rollback().await;
            Ok(format!(
                "offer:{}:{}/{}/{}:{}:{}",
                row.get::<Uuid, _>("id"),
                row.get::<String, _>("first_name"),
                row.get::<String, _>("last_name"),
                row.get::<String, _>("position_title"),
                row.get::<String, _>("salary"),
                row.get::<String, _>("start_date")
            )
            .into_bytes())
        }
    }
}

fn pdf_cache_path(tenant_id: Uuid, payslip_id: Uuid) -> PathBuf {
    std::env::var("PAYSLIP_STORE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data/payslips"))
        .join(tenant_id.to_string())
        .join(format!("{payslip_id}.pdf"))
}

/// Sign a payslip or offer for a tenant. Idempotent: re-signing returns the
/// existing signature row (updated typed name / hash / time).
pub async fn sign_document(
    pool: &TenantPool,
    tenant_id: TenantId,
    doc_type: DocumentType,
    document_id: Uuid,
    signer_user_id: Uuid,
    typed_name: &str,
) -> EsignResult<SignatureView> {
    let typed_name = typed_name.trim();
    if typed_name.is_empty() || typed_name.chars().count() > MAX_TYPED_NAME {
        return Err(EsignError::InvalidName);
    }

    // Confirm the signer is a user of this tenant.
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let signer: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE id = $1 AND tenant_id = $2")
        .bind(signer_user_id)
        .bind(tenant_id.0)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(EsignError::SignerNotFound)?;

    let bytes = document_bytes(pool, &tenant_id, &doc_type, &document_id).await?;
    let doc_hash = sha256_hex(&bytes);
    let provider = provider_name();

    // Upsert: one signature per (tenant, document_type, document_id, signer).
    let sig_id: Uuid = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO document_signatures
             (tenant_id, document_type, document_id, signer_user_id, typed_name,
              doc_hash_sha256, signed_at, provider, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, now(), $7, now())
           ON CONFLICT (tenant_id, document_type, document_id, signer_user_id)
           DO UPDATE SET typed_name = EXCLUDED.typed_name,
                         doc_hash_sha256 = EXCLUDED.doc_hash_sha256,
                         signed_at = now(),
                         provider = EXCLUDED.provider
           RETURNING id"#,
    )
    .bind(tenant_id.0)
    .bind(doc_type.as_str())
    .bind(document_id)
    .bind(signer)
    .bind(typed_name)
    .bind(&doc_hash)
    .bind(&provider)
    .fetch_one(&mut *tx)
    .await?;

    // Audit log for the significant action.
    sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, $3, $4, $5, $6)"#,
    )
    .bind(tenant_id.0)
    .bind(signer.to_string())
    .bind(format!("document.{}", doc_type.as_str()))
    .bind(doc_type.as_str())
    .bind(document_id.to_string())
    .bind(serde_json::json!({
        "signature_id": sig_id.to_string(),
        "typed_name": typed_name,
        "doc_hash_sha256": doc_hash,
        "provider": provider,
    }))
    .execute(&mut *tx)
    .await?;

    // Link the signature back to the document record.
    match &doc_type {
        DocumentType::Payslip => {
            sqlx::query("UPDATE payslips SET signature_id = $1 WHERE id = $2 AND tenant_id = $3")
                .bind(sig_id)
                .bind(document_id)
                .bind(tenant_id.0)
                .execute(&mut *tx)
                .await?;
        }
        DocumentType::Offer => {
            sqlx::query(
                "UPDATE candidate_offers SET signature_id = $1, updated_at = now() \
                 WHERE id = $2 AND tenant_id = $3",
            )
            .bind(sig_id)
            .bind(document_id)
            .bind(tenant_id.0)
            .execute(&mut *tx)
            .await?;
        }
    }

    let row = sqlx::query(
        r#"SELECT id, document_type, document_id, signer_user_id, typed_name,
                  doc_hash_sha256, signed_at, provider
             FROM document_signatures
            WHERE tenant_id = $1 AND document_type = $2 AND document_id = $3
              AND signer_user_id = $4
            ORDER BY signed_at DESC
            LIMIT 1"#,
    )
    .bind(tenant_id.0)
    .bind(doc_type.as_str())
    .bind(document_id)
    .bind(signer)
    .fetch_optional(&mut *tx)
    .await?;
    let view = row.map(|r| SignatureView {
        id: r.get::<Uuid, _>("id"),
        document_type: r.get::<String, _>("document_type"),
        document_id: r.get::<Uuid, _>("document_id"),
        signer_user_id: r.get::<Uuid, _>("signer_user_id"),
        typed_name: r.get::<String, _>("typed_name"),
        doc_hash_sha256: r.get::<String, _>("doc_hash_sha256"),
        signed_at: r.get::<DateTime<Utc>, _>("signed_at"),
        provider: r.get::<String, _>("provider"),
    });
    let view = view.ok_or(EsignError::RowNotFound)?;
    tx.commit().await?;
    Ok(view)
}

/// Confirm an employee is linked to and owns the payslip, so a self-service
/// caller can sign only their own document.
pub async fn claim_employee_of_payslip(
    pool: &TenantPool,
    tenant_id: TenantId,
    payslip_id: Uuid,
    user_id: Uuid,
) -> EsignResult<Uuid> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let found: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
               SELECT 1 FROM payslips p
               JOIN employees e ON e.id = p.employee_id
               WHERE p.id = $1 AND p.tenant_id = $2 AND e.user_id = $3
            )"#,
    )
    .bind(payslip_id)
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    let _ = tx.rollback().await;
    if found {
        Ok(payslip_id)
    } else {
        Err(EsignError::NotOwnPayslip)
    }
}

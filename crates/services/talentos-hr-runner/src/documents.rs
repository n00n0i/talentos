//! Employee documents: metadata, one stored file per document, expiry views,
//! and employee self-service reads. Tenant isolation is `begin_tenant` + RLS.

use std::path::PathBuf;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use sqlx::Row;
use talentos_attendance::bangkok_offset;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;
const ALLOWED_FILE_TYPES: &[&str] = &["application/pdf", "image/jpeg", "image/png"];
const DOC_TYPES: &[&str] = &[
    "contract",
    "work_permit",
    "visa",
    "id_card",
    "certificate",
    "other",
];

pub fn upload_dir() -> PathBuf {
    std::env::var("DOCUMENT_UPLOAD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/talentos-documents"))
}

#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("employee not found for this tenant")]
    EmployeeNotFound,
    #[error("document not found")]
    NotFound,
    #[error("invalid document type")]
    InvalidType,
    #[error("title must be 1-200 characters")]
    InvalidTitle,
    #[error("document number must be at most 100 characters")]
    InvalidNumber,
    #[error("expiry date must not precede the issue date")]
    InvalidDates,
    #[error("file is too large; maximum size is 5 MB")]
    FileTooLarge,
    #[error("unsupported file type; use PDF, JPEG, or PNG")]
    InvalidFileType,
    #[error("document has no file")]
    NoFile,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Clone)]
pub struct DocumentInput {
    pub doc_type: String,
    pub title: String,
    pub doc_number: Option<String>,
    pub issued_date: Option<NaiveDate>,
    pub expiry_date: Option<NaiveDate>,
}

#[derive(Debug, Serialize)]
pub struct DocumentView {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub doc_type: String,
    pub title: String,
    pub doc_number: Option<String>,
    pub issued_date: Option<NaiveDate>,
    pub expiry_date: Option<NaiveDate>,
    /// Days until expiry; negative when already expired.
    pub days_left: Option<i64>,
    pub has_file: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ExpiringDocument {
    pub employee_code: String,
    pub employee_name: String,
    #[serde(flatten)]
    pub document: DocumentView,
}

pub struct DocumentFile {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

async fn write_private(
    dir: &std::path::Path,
    path: &std::path::Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    tokio::fs::write(path, bytes).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).await?;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    }
    Ok(())
}

fn today() -> NaiveDate {
    Utc::now().with_timezone(&bangkok_offset()).date_naive()
}

fn map_document(
    row: &sqlx::postgres::PgRow,
    today: NaiveDate,
) -> Result<DocumentView, sqlx::Error> {
    let expiry_date: Option<NaiveDate> = row.try_get("expiry_date")?;
    Ok(DocumentView {
        id: row.try_get("id")?,
        employee_id: row.try_get("employee_id")?,
        doc_type: row.try_get("doc_type")?,
        title: row.try_get("title")?,
        doc_number: row.try_get("doc_number")?,
        issued_date: row.try_get("issued_date")?,
        expiry_date,
        days_left: expiry_date.map(|d| (d - today).num_days()),
        has_file: row.try_get::<Option<String>, _>("file_path")?.is_some(),
        created_at: row.try_get("created_at")?,
    })
}

fn validate(input: &DocumentInput) -> Result<String, DocumentError> {
    if !DOC_TYPES.contains(&input.doc_type.as_str()) {
        return Err(DocumentError::InvalidType);
    }
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(DocumentError::InvalidTitle);
    }
    if input
        .doc_number
        .as_deref()
        .is_some_and(|n| n.trim().chars().count() > 100)
    {
        return Err(DocumentError::InvalidNumber);
    }
    if let (Some(issued), Some(expiry)) = (input.issued_date, input.expiry_date) {
        if expiry < issued {
            return Err(DocumentError::InvalidDates);
        }
    }
    Ok(title.to_string())
}

pub async fn create_document(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    employee_id: Uuid,
    input: &DocumentInput,
) -> Result<DocumentView, DocumentError> {
    let title = validate(input)?;
    let mut tx = pool.begin_tenant(tenant_id).await?;

    let employee: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM employees WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.0)
            .bind(employee_id)
            .fetch_optional(&mut *tx)
            .await?;
    if employee.is_none() {
        return Err(DocumentError::EmployeeNotFound);
    }

    let row = sqlx::query(
        r#"INSERT INTO employee_documents
             (tenant_id, employee_id, doc_type, title, doc_number,
              issued_date, expiry_date, uploaded_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
           RETURNING id, employee_id, doc_type, title, doc_number,
                     issued_date, expiry_date, file_path, created_at"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(&input.doc_type)
    .bind(&title)
    .bind(
        input
            .doc_number
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty()),
    )
    .bind(input.issued_date)
    .bind(input.expiry_date)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    let view = map_document(&row, today())?;
    tx.commit().await?;
    Ok(view)
}

pub async fn list_documents(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
) -> Result<Vec<DocumentView>, DocumentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT id, employee_id, doc_type, title, doc_number, issued_date,
                  expiry_date, file_path, created_at
             FROM employee_documents
            WHERE tenant_id = $1 AND employee_id = $2
            ORDER BY expiry_date NULLS LAST, created_at DESC"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let today = today();
    rows.iter()
        .map(|row| map_document(row, today).map_err(DocumentError::from))
        .collect()
}

pub async fn expiring_documents(
    pool: &TenantPool,
    tenant_id: TenantId,
    days: i64,
) -> Result<Vec<ExpiringDocument>, DocumentError> {
    let days = days.clamp(1, 365);
    let today = today();
    let limit = today + Duration::days(days);
    let floor = today - Duration::days(365);
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT d.id, d.employee_id, d.doc_type, d.title, d.doc_number, d.issued_date,
                  d.expiry_date, d.file_path, d.created_at,
                  e.employee_code, e.first_name_th, e.last_name_th
             FROM employee_documents d
             JOIN employees e ON e.id = d.employee_id
            WHERE d.tenant_id = $1 AND d.expiry_date IS NOT NULL
              AND d.expiry_date BETWEEN $2 AND $3
              AND e.status <> 'terminated'
            ORDER BY d.expiry_date, e.employee_code
            LIMIT 500"#,
    )
    .bind(tenant_id.0)
    .bind(floor)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.iter()
        .map(|row| {
            Ok(ExpiringDocument {
                employee_code: row.try_get("employee_code")?,
                employee_name: format!(
                    "{} {}",
                    row.try_get::<String, _>("first_name_th")?,
                    row.try_get::<String, _>("last_name_th")?
                ),
                document: map_document(row, today)?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(DocumentError::from)
}

pub async fn delete_document(
    pool: &TenantPool,
    tenant_id: TenantId,
    document_id: Uuid,
) -> Result<(), DocumentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let path: Option<String> = sqlx::query_scalar(
        "DELETE FROM employee_documents WHERE tenant_id = $1 AND id = $2 RETURNING file_path",
    )
    .bind(tenant_id.0)
    .bind(document_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(DocumentError::NotFound)?;
    tx.commit().await?;
    if let Some(path) = path {
        let _ = tokio::fs::remove_file(path).await;
    }
    Ok(())
}

pub async fn set_file(
    pool: &TenantPool,
    tenant_id: TenantId,
    document_id: Uuid,
    content_type: &str,
    bytes: &[u8],
) -> Result<(), DocumentError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(DocumentError::FileTooLarge);
    }
    if !ALLOWED_FILE_TYPES.contains(&content_type) {
        return Err(DocumentError::InvalidFileType);
    }
    let ext = match content_type {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        _ => "pdf",
    };

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT file_path FROM employee_documents
          WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(tenant_id.0)
    .bind(document_id)
    .fetch_optional(&mut *tx)
    .await?
    .flatten();
    // Distinguish "no row" from "row without a file".
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM employee_documents WHERE tenant_id = $1 AND id = $2)",
    )
    .bind(tenant_id.0)
    .bind(document_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(DocumentError::NotFound);
    }

    let dir = upload_dir();
    let filename = format!("{}.{}", Uuid::new_v4(), ext);
    let path = dir.join(&filename);
    write_private(&dir, &path, bytes).await?;

    let updated = sqlx::query(
        "UPDATE employee_documents
            SET file_path = $3, file_type = $4, updated_at = now()
          WHERE tenant_id = $1 AND id = $2
          RETURNING id",
    )
    .bind(tenant_id.0)
    .bind(document_id)
    .bind(path.to_string_lossy().to_string())
    .bind(content_type)
    .fetch_optional(&mut *tx)
    .await?;
    if updated.is_none() {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(DocumentError::NotFound);
    }
    tx.commit().await?;
    if let Some(old) = existing {
        let _ = tokio::fs::remove_file(old).await;
    }
    Ok(())
}

async fn file_for(
    pool: &TenantPool,
    tenant_id: TenantId,
    document_id: Uuid,
    owner_user: Option<Uuid>,
) -> Result<DocumentFile, DocumentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let filter = if owner_user.is_some() {
        " AND EXISTS (SELECT 1 FROM employees e
                       WHERE e.id = d.employee_id AND e.user_id = $3)"
    } else {
        ""
    };
    let sql = format!(
        "SELECT d.file_path, d.file_type FROM employee_documents d
          WHERE d.tenant_id = $1 AND d.id = $2{filter}"
    );
    let mut query = sqlx::query(&sql).bind(tenant_id.0).bind(document_id);
    if let Some(user_id) = owner_user {
        query = query.bind(user_id);
    }
    let row = query.fetch_optional(&mut *tx).await?;
    tx.commit().await?;

    let Some(row) = row else {
        return Err(DocumentError::NotFound);
    };
    let path: Option<String> = row.try_get("file_path")?;
    let Some(path) = path else {
        return Err(DocumentError::NoFile);
    };
    let content_type: Option<String> = row.try_get("file_type")?;
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(DocumentError::NotFound),
        Err(e) => return Err(e.into()),
    };
    Ok(DocumentFile {
        content_type: content_type.unwrap_or_else(|| "application/octet-stream".into()),
        bytes,
    })
}

pub async fn get_file(
    pool: &TenantPool,
    tenant_id: TenantId,
    document_id: Uuid,
) -> Result<DocumentFile, DocumentError> {
    file_for(pool, tenant_id, document_id, None).await
}

pub async fn list_my_documents(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
) -> Result<Vec<DocumentView>, DocumentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT d.id, d.employee_id, d.doc_type, d.title, d.doc_number, d.issued_date,
                  d.expiry_date, d.file_path, d.created_at
             FROM employee_documents d
             JOIN employees e ON e.id = d.employee_id
            WHERE d.tenant_id = $1 AND e.user_id = $2
            ORDER BY d.expiry_date NULLS LAST, d.created_at DESC"#,
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let today = today();
    rows.iter()
        .map(|row| map_document(row, today).map_err(DocumentError::from))
        .collect()
}

pub async fn get_my_file(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    document_id: Uuid,
) -> Result<DocumentFile, DocumentError> {
    file_for(pool, tenant_id, document_id, Some(user_id)).await
}

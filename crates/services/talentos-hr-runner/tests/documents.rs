//! Employee documents: expiry views, file storage, and self-service scoping.
//! Requires DATABASE_URL; skips when unset.

use chrono::{Duration, Utc};
use sqlx::PgPool;
use talentos_attendance::bangkok_offset;
use talentos_db::TenantPool;
use talentos_hr_runner::documents::{
    create_document, delete_document, expiring_documents, get_file, get_my_file, list_documents,
    list_my_documents, set_file, upload_dir, DocumentError, DocumentInput,
};
use talentos_kernel::TenantId;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

fn today() -> chrono::NaiveDate {
    Utc::now().with_timezone(&bangkok_offset()).date_naive()
}

fn input(doc_type: &str, title: &str, in_days: Option<i64>) -> DocumentInput {
    DocumentInput {
        doc_type: doc_type.into(),
        title: title.into(),
        doc_number: Some("A-123".into()),
        issued_date: Some(today() - Duration::days(365)),
        expiry_date: in_days.map(|d| today() + Duration::days(d)),
    }
}

async fn file_path(raw: &PgPool, tenant_id: uuid::Uuid, id: uuid::Uuid) -> Option<String> {
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let path = sqlx::query_scalar("SELECT file_path FROM employee_documents WHERE id = $1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .expect("file path");
    let _ = tx.rollback().await;
    path
}

#[tokio::test]
async fn create_list_and_expiry_alerts() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let (_, employee_id) = entities[0];
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let tenant = TenantId(tenant_id);

    let soon = create_document(
        &pool,
        tenant,
        user_id,
        employee_id,
        &input("work_permit", "ใบอนุญาตทำงาน", Some(10)),
    )
    .await
    .expect("expiring document");
    assert_eq!(soon.days_left, Some(10));

    let expired = create_document(
        &pool,
        tenant,
        user_id,
        employee_id,
        &input("visa", "วีซ่าหมดอายุ", Some(-5)),
    )
    .await
    .expect("expired document");
    assert_eq!(expired.days_left, Some(-5));

    create_document(
        &pool,
        tenant,
        user_id,
        employee_id,
        &input("contract", "สัญญาจ้าง", None),
    )
    .await
    .expect("document without expiry");

    let listed = list_documents(&pool, tenant, employee_id)
        .await
        .expect("list");
    assert_eq!(listed.len(), 3);

    let expiring = expiring_documents(&pool, tenant, 30).await.expect("expiry");
    assert_eq!(expiring.len(), 2, "expired + within 30 days");
    assert_eq!(expiring[0].document.title, "วีซ่าหมดอายุ");
    assert_eq!(expiring[0].employee_code, "EMP000");
    assert_eq!(expiring[1].document.title, "ใบอนุญาตทำงาน");

    let err = create_document(
        &pool,
        tenant,
        user_id,
        employee_id,
        &input("passport", "ไม่รองรับ", None),
    )
    .await
    .expect_err("unknown type");
    assert!(matches!(err, DocumentError::InvalidType));

    let mut bad_number = input("contract", "เลขยาว", None);
    bad_number.doc_number = Some("x".repeat(101));
    let err = create_document(&pool, tenant, user_id, employee_id, &bad_number)
        .await
        .expect_err("document number too long");
    assert!(matches!(err, DocumentError::InvalidNumber));

    let mut bad_dates = input("contract", "วันที่ผิด", Some(1));
    bad_dates.issued_date = Some(today() + Duration::days(10));
    let err = create_document(&pool, tenant, user_id, employee_id, &bad_dates)
        .await
        .expect_err("expiry before issue");
    assert!(matches!(err, DocumentError::InvalidDates));

    // An employee from another tenant is not addressable.
    let (other_tenant, other_entities) = seed_tenant_entities(&raw, 1).await;
    let err = create_document(
        &pool,
        tenant,
        user_id,
        other_entities[0].1,
        &input("contract", "ข้าม tenant", None),
    )
    .await
    .expect_err("cross-tenant employee");
    assert!(matches!(err, DocumentError::EmployeeNotFound));

    cleanup(&raw, tenant_id).await;
    cleanup(&raw, other_tenant).await;
}

#[tokio::test]
async fn upload_get_replace_and_delete_removes_files() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let (_, employee_id) = entities[0];
    let user_id = user_id_of(&raw, tenant_id, employee_id).await;
    let tenant = TenantId(tenant_id);
    std::env::set_var(
        "DOCUMENT_UPLOAD_DIR",
        upload_dir().join(format!("test-{}", uuid::Uuid::new_v4())),
    );

    let doc = create_document(
        &pool,
        tenant,
        user_id,
        employee_id,
        &input("contract", "สัญญาจ้าง", None),
    )
    .await
    .expect("document");

    assert!(matches!(
        get_file(&pool, tenant, doc.id).await,
        Err(DocumentError::NoFile)
    ));
    let oversized = vec![0u8; 5 * 1024 * 1024 + 1];
    let err = set_file(&pool, tenant, doc.id, "application/pdf", &oversized)
        .await
        .expect_err("oversized file");
    assert!(matches!(err, DocumentError::FileTooLarge));

    set_file(&pool, tenant, doc.id, "application/pdf", b"%PDF-1.4 first")
        .await
        .expect("first upload");
    let first = get_file(&pool, tenant, doc.id).await.expect("first read");
    assert_eq!(first.content_type, "application/pdf");
    assert_eq!(first.bytes, b"%PDF-1.4 first");
    let first_path = file_path(&raw, tenant_id, doc.id)
        .await
        .expect("stored path");

    set_file(&pool, tenant, doc.id, "image/png", b"PNG second")
        .await
        .expect("replace upload");
    let second = get_file(&pool, tenant, doc.id).await.expect("second read");
    assert_eq!(second.content_type, "image/png");
    assert_eq!(second.bytes, b"PNG second");
    assert!(
        !std::path::Path::new(&first_path).exists(),
        "replaced file is removed"
    );
    let second_path = file_path(&raw, tenant_id, doc.id)
        .await
        .expect("new stored path");

    let err = set_file(&pool, tenant, doc.id, "text/plain", b"x")
        .await
        .expect_err("unsupported type");
    assert!(matches!(err, DocumentError::InvalidFileType));

    delete_document(&pool, tenant, doc.id)
        .await
        .expect("delete");
    assert!(
        !std::path::Path::new(&second_path).exists(),
        "deleted document removes its file"
    );
    assert!(matches!(
        get_file(&pool, tenant, doc.id).await,
        Err(DocumentError::NotFound)
    ));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn documents_are_not_reachable_from_another_tenant() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_a, entities_a) = seed_tenant_entities(&raw, 1).await;
    let (_, employee_a) = entities_a[0];
    let user_a = user_id_of(&raw, tenant_a, employee_a).await;
    std::env::set_var(
        "DOCUMENT_UPLOAD_DIR",
        upload_dir().join(format!("test-{}", uuid::Uuid::new_v4())),
    );
    let doc = create_document(
        &pool,
        TenantId(tenant_a),
        user_a,
        employee_a,
        &input("contract", "ของ tenant A", None),
    )
    .await
    .expect("document");
    set_file(
        &pool,
        TenantId(tenant_a),
        doc.id,
        "application/pdf",
        b"%PDF A",
    )
    .await
    .expect("file");

    let (tenant_b, _) = seed_tenant_entities(&raw, 1).await;
    let tenant_b = TenantId(tenant_b);
    assert!(list_documents(&pool, tenant_b, employee_a)
        .await
        .expect("cross-tenant list")
        .is_empty());
    assert!(matches!(
        get_file(&pool, tenant_b, doc.id).await,
        Err(DocumentError::NotFound)
    ));
    assert!(matches!(
        delete_document(&pool, tenant_b, doc.id).await,
        Err(DocumentError::NotFound)
    ));
    assert!(matches!(
        set_file(&pool, tenant_b, doc.id, "application/pdf", b"x").await,
        Err(DocumentError::NotFound)
    ));
    // The owner can still read the document.
    assert_eq!(
        get_file(&pool, TenantId(tenant_a), doc.id)
            .await
            .expect("owner read")
            .bytes,
        b"%PDF A"
    );

    delete_document(&pool, TenantId(tenant_a), doc.id)
        .await
        .expect("owner delete");
    cleanup(&raw, tenant_a).await;
    cleanup(&raw, tenant_b.0).await;
}

#[tokio::test]
async fn self_service_lists_and_downloads_only_own_documents() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let (_, employee_a) = entities[0];
    let (_, employee_b) = entities[1];
    let user_a = user_id_of(&raw, tenant_id, employee_a).await;
    let user_b = user_id_of(&raw, tenant_id, employee_b).await;
    let tenant = TenantId(tenant_id);
    std::env::set_var(
        "DOCUMENT_UPLOAD_DIR",
        upload_dir().join(format!("test-{}", uuid::Uuid::new_v4())),
    );

    let doc_a = create_document(
        &pool,
        tenant,
        user_a,
        employee_a,
        &input("contract", "ของ A", None),
    )
    .await
    .expect("A document");
    let doc_b = create_document(
        &pool,
        tenant,
        user_b,
        employee_b,
        &input("contract", "ของ B", None),
    )
    .await
    .expect("B document");
    set_file(&pool, tenant, doc_a.id, "application/pdf", b"%PDF A")
        .await
        .expect("A file");
    set_file(&pool, tenant, doc_b.id, "application/pdf", b"%PDF B")
        .await
        .expect("B file");

    let mine = list_my_documents(&pool, tenant, user_a)
        .await
        .expect("my list");
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].id, doc_a.id);
    assert!(get_my_file(&pool, tenant, user_a, doc_a.id).await.is_ok());
    assert!(matches!(
        get_my_file(&pool, tenant, user_a, doc_b.id).await,
        Err(DocumentError::NotFound)
    ));

    delete_document(&pool, tenant, doc_a.id)
        .await
        .expect("delete A");
    delete_document(&pool, tenant, doc_b.id)
        .await
        .expect("delete B");
    cleanup(&raw, tenant_id).await;
}

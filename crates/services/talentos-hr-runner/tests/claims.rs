//! Expense claim service tests.
//!
//! Skips when DATABASE_URL is unset so `cargo test` passes offline.

use chrono::NaiveDate;
use rust_decimal_macros::dec;
use sqlx::PgPool;
use talentos_db::TenantPool;
use talentos_hr_runner::claims::{
    cancel_claim, create_claim, decide_claim, download_receipt, employee_of_user, list_claims,
    owner_of_claim, pending_approvals, set_receipt, upload_dir, ClaimInput, ReceiptError,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{cleanup, database_url, ensure_role, seed_tenant_entities, user_id_of};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn input() -> ClaimInput {
    ClaimInput {
        category: "meal".into(),
        amount: dec!(500),
        expense_date: d(2026, 10, 27),
        description: Some("team lunch".into()),
    }
}

#[tokio::test]
async fn create_and_list_own_claims() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let claim = create_claim(&pool, tenant, user_id, input())
        .await
        .expect("create claim");
    assert_eq!(claim.category, "meal");
    assert_eq!(claim.amount, dec!(500));
    assert_eq!(claim.status, "pending");
    assert!(!claim.has_receipt);

    let employee_id = employee_of_user(&pool, tenant, user_id)
        .await
        .expect("resolve")
        .unwrap();
    let claims = list_claims(&pool, tenant, employee_id, None)
        .await
        .expect("list claims");
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].id, claim.id);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn unlinked_user_gets_empty_claims() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, _entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);

    let linked = employee_of_user(&pool, tenant, Uuid::new_v4())
        .await
        .unwrap();
    assert!(linked.is_none());

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn cancel_pending_succeeds() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let claim = create_claim(&pool, tenant, user_id, input())
        .await
        .expect("create claim");
    let status = cancel_claim(&pool, tenant, user_id, claim.id)
        .await
        .expect("cancel");
    assert_eq!(status, "cancelled");

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn cancel_other_employees_claim_returns_not_found() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let user0 = user_id_of(&raw, tenant_id, entities[0].1).await;
    let user1 = user_id_of(&raw, tenant_id, entities[1].1).await;

    let claim = create_claim(&pool, tenant, user0, input())
        .await
        .expect("create claim");
    let err = cancel_claim(&pool, tenant, user1, claim.id)
        .await
        .unwrap_err();
    assert!(matches!(err, talentos_hr_runner::ClaimError::NotFound));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn cancel_approved_claim_returns_conflict() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let claim = create_claim(&pool, tenant, user_id, input())
        .await
        .expect("create claim");

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("UPDATE expense_claims SET status = 'approved' WHERE id = $1")
        .bind(claim.id)
        .execute(&mut *tx)
        .await
        .expect("approve");
    tx.commit().await.expect("commit");

    let err = cancel_claim(&pool, tenant, user_id, claim.id)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        talentos_hr_runner::ClaimError::NotCancellable
    ));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn decide_once_then_decide_again_returns_conflict() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let approver = user_id_of(&raw, tenant_id, entities[0].1).await;
    let requester = user_id_of(&raw, tenant_id, entities[1].1).await;
    ensure_role(&raw, tenant_id, "hr_admin", approver).await;

    let claim = create_claim(&pool, tenant, requester, input())
        .await
        .expect("create claim");

    let status = decide_claim(&pool, tenant, approver, claim.id, true)
        .await
        .expect("approve");
    assert_eq!(status, "approved");

    let err = decide_claim(&pool, tenant, approver, claim.id, false)
        .await
        .unwrap_err();
    assert!(matches!(err, talentos_hr_runner::ClaimError::NotPending));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn hr_inbox_lists_pending_with_code_and_name() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 2).await;
    let tenant = TenantId(tenant_id);

    let user0 = user_id_of(&raw, tenant_id, entities[0].1).await;
    let user1 = user_id_of(&raw, tenant_id, entities[1].1).await;

    let pending = create_claim(&pool, tenant, user0, input())
        .await
        .expect("create pending");
    let approved = create_claim(&pool, tenant, user1, input())
        .await
        .expect("create approved");

    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("UPDATE expense_claims SET status = 'approved' WHERE id = $1")
        .bind(approved.id)
        .execute(&mut *tx)
        .await
        .expect("approve");
    tx.commit().await.expect("commit");

    let inbox = pending_approvals(&pool, tenant).await.expect("inbox");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].id, pending.id);
    assert_eq!(inbox[0].employee_code, "EMP000");
    assert_eq!(inbox[0].employee_name, "ทดสอบ พนักงาน");
    assert_eq!(inbox[0].category, "meal");
    assert_eq!(inbox[0].amount, dec!(500));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn tenant_isolation_hides_cross_tenant_claim() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool_a = TenantPool::connect(&url, 2).await.expect("tenant pool a");
    let pool_b = TenantPool::connect(&url, 2).await.expect("tenant pool b");
    let (tenant_a, entities_a) = seed_tenant_entities(&raw, 1).await;
    let (tenant_b, _entities_b) = seed_tenant_entities(&raw, 1).await;
    let user_a = user_id_of(&raw, tenant_a, entities_a[0].1).await;

    let claim = create_claim(&pool_a, TenantId(tenant_a), user_a, input())
        .await
        .expect("create claim");

    let owner = owner_of_claim(&pool_b, TenantId(tenant_b), claim.id)
        .await
        .expect("lookup cross-tenant");
    assert!(owner.is_none());

    cleanup(&raw, tenant_a).await;
    cleanup(&raw, tenant_b).await;
}

#[tokio::test]
async fn receipt_upload_validates_size_and_type() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let claim = create_claim(&pool, tenant, user_id, input())
        .await
        .expect("create claim");

    let oversized = vec![0u8; 5 * 1024 * 1024 + 1];
    let err = set_receipt(&pool, tenant, user_id, claim.id, "image/png", &oversized)
        .await
        .unwrap_err();
    let inner = match err {
        talentos_hr_runner::ClaimError::Receipt(r) => r,
        other => panic!("expected Receipt error, got {other:?}"),
    };
    assert!(matches!(inner, ReceiptError::TooLarge));

    let err = set_receipt(&pool, tenant, user_id, claim.id, "image/gif", b"GIF")
        .await
        .unwrap_err();
    let inner = match err {
        talentos_hr_runner::ClaimError::Receipt(r) => r,
        other => panic!("expected Receipt error, got {other:?}"),
    };
    assert!(matches!(inner, ReceiptError::InvalidType));

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn receipt_upload_generates_uuid_filename_and_download_roundtrips() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    std::env::set_var(
        "CLAIM_UPLOAD_DIR",
        upload_dir().join(format!("test-{}", Uuid::new_v4())),
    );

    let claim = create_claim(&pool, tenant, user_id, input())
        .await
        .expect("create claim");
    let png = b"\x89PNG\r\n\x1a\n";
    set_receipt(&pool, tenant, user_id, claim.id, "image/png", png)
        .await
        .expect("upload receipt");

    // Find the generated file path and assert it is a flat uuid filename under upload_dir.
    let mut tx = raw.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let path: String = sqlx::query_scalar("SELECT receipt_path FROM expense_claims WHERE id = $1")
        .bind(claim.id)
        .fetch_one(&mut *tx)
        .await
        .expect("fetch path");
    let _ = tx.rollback().await;
    let parent = std::path::Path::new(&path).parent().unwrap();
    assert_eq!(parent, upload_dir());
    let filename = std::path::Path::new(&path)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    assert!(filename.ends_with(".png"));
    let uuid_part = &filename[..filename.len() - 4];
    assert!(Uuid::parse_str(uuid_part).is_ok());

    let file = download_receipt(&pool, tenant, claim.id)
        .await
        .expect("download")
        .expect("file exists");
    assert_eq!(file.content_type, "image/png");
    assert_eq!(file.bytes, png);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn missing_receipt_download_returns_none() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let claim = create_claim(&pool, tenant, user_id, input())
        .await
        .expect("create claim");
    let file = download_receipt(&pool, tenant, claim.id)
        .await
        .expect("download");
    assert!(file.is_none());

    cleanup(&raw, tenant_id).await;
}

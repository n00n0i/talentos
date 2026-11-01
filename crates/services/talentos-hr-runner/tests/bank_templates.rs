//! Bank payout template registry: RLS isolation, audit logging, unverified default.
//! Requires DATABASE_URL; skips when unset.

use sqlx::PgPool;
use talentos_db::TenantPool;
use talentos_hr_runner::bank_templates::{
    create_bank_template, get_bank_template, list_bank_templates, BankTemplateError,
    BankTemplateInput, PAYOUT_SOURCES,
};
use talentos_kernel::TenantId;

mod common;
use common::{cleanup, database_url, seed_tenant_entities, user_id_of};

fn input(bank: &str, layout: &str) -> BankTemplateInput {
    BankTemplateInput {
        bank_name: bank.into(),
        layout_name: layout.into(),
        field_order: vec![
            "employee_code".into(),
            "bank_name".into(),
            "bank_account".into(),
            "net".into(),
        ],
        header_row: Some("code,bank,account,amount".into()),
        unverified: None, // exercises the default-true path
        sign_off_note: Some("needs accountant sign-off".into()),
    }
}

#[tokio::test]
async fn create_defaults_unverified_true_and_writes_audit_row() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;
    let tenant = TenantId(tenant_id);

    let created = create_bank_template(&pool, tenant, user_id, input("KTB", "CSV upload"))
        .await
        .expect("create template");
    assert!(
        created.unverified,
        "new layouts must ship unverified by default"
    );

    // Significant action leaves an audit row in the same tenant. Query with the
    // tenant GUC set so RLS (forced) surfaces the just-committed row.
    let mut tx = raw.begin().await.expect("begin audit check");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let audit_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'payout_template.create'",
    )
    .fetch_one(&mut *tx)
    .await
    .expect("audit count");
    let _ = tx.rollback().await;
    assert!(
        audit_rows >= 1,
        "create_bank_template must write an audit_log row"
    );

    // Round-trip the create through the read path.
    let listed = list_bank_templates(&pool, tenant, Some("KTB"))
        .await
        .expect("list");
    assert_eq!(listed.len(), 1, "tenant sees only its own KTB layout");
    assert_eq!(listed[0].field_order, created.field_order);

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn list_is_tenant_isolated() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_a, entities_a) = seed_tenant_entities(&raw, 1).await;
    let (tenant_b, entities_b) = seed_tenant_entities(&raw, 1).await;
    let user_a = user_id_of(&raw, tenant_a, entities_a[0].1).await;
    let user_b = user_id_of(&raw, tenant_b, entities_b[0].1).await;

    let created_a = create_bank_template(&pool, TenantId(tenant_a), user_a, input("SCB", "CSV"))
        .await
        .expect("A template");
    let _created_b = create_bank_template(&pool, TenantId(tenant_b), user_b, input("SCB", "CSV"))
        .await
        .expect("B template");

    // A sees its own row, not B's — even though both use the same bank name.
    let listed_a = list_bank_templates(&pool, TenantId(tenant_a), Some("SCB"))
        .await
        .expect("A list");
    assert_eq!(listed_a.len(), 1);
    assert_eq!(listed_a[0].id, created_a.id, "A must not see B's layout");

    cleanup(&raw, tenant_a).await;
    cleanup(&raw, tenant_b).await;
}

#[tokio::test]
async fn create_rejects_unknown_source() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, entities) = seed_tenant_entities(&raw, 1).await;
    let user_id = user_id_of(&raw, tenant_id, entities[0].1).await;

    let mut bad = input("BBL", "CSV");
    bad.field_order = vec!["salary".into()];
    let result = create_bank_template(&pool, TenantId(tenant_id), user_id, bad.clone()).await;
    assert!(
        matches!(result, Err(BankTemplateError::InvalidField(ref f)) if f == "salary"),
        "unknown source must be rejected up front: {result:?}"
    );
    assert!(
        PAYOUT_SOURCES
            .iter()
            .all(|s| !bad.field_order.iter().any(|f| f == s)),
        "sanity: 'salary' is not a payout source"
    );

    cleanup(&raw, tenant_id).await;
}

#[tokio::test]
async fn get_returns_tenant_rows_only() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_a, _entities_a) = seed_tenant_entities(&raw, 1).await;
    let (tenant_b, entities_b) = seed_tenant_entities(&raw, 1).await;
    let user_b = user_id_of(&raw, tenant_b, entities_b[0].1).await;

    let created_b = create_bank_template(&pool, TenantId(tenant_b), user_b, input("BAY", "CSV"))
        .await
        .expect("B template");

    // A cannot fetch B's layout by id.
    let cross = get_bank_template(&pool, TenantId(tenant_a), created_b.id).await;
    assert!(matches!(cross, Err(BankTemplateError::NotFound(_))));

    // B fetches its own row.
    let own = get_bank_template(&pool, TenantId(tenant_b), created_b.id)
        .await
        .expect("B own row");
    assert_eq!(own.id, created_b.id);

    cleanup(&raw, tenant_a).await;
    cleanup(&raw, tenant_b).await;
}

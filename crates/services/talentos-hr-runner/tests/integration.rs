//! Integration Control Plane service: connector lifecycle, published immutability,
//! connection CRUD, secret handling, tenant isolation.
//! Requires DATABASE_URL; skips when unset.

use serde_json::json;
use sqlx::PgPool;
use talentos_db::TenantPool;
use talentos_hr_runner::integration::{
    create_connection, create_connector, delete_connection, disable_connector, get_connection,
    get_connector, list_connections, list_connectors, publish_connector, record_connection_test,
    update_connection, update_draft, ConnectionInput, ConnectionTest, ConnectionUpdate,
    ConnectorInput, ConnectorUpdate, IntegrationError,
};
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;
use common::{database_url, seed_tenant_entities};

fn connector(version: &str) -> ConnectorInput {
    ConnectorInput {
        connector_id: "talentos.rest".into(),
        name: "REST / OpenAPI".into(),
        version: version.into(),
        protocol: "rest".into(),
        manifest: json!({
            "id": "talentos.rest",
            "version": version,
            "protocol": "rest",
            "operations": [{ "name": "probe", "kind": "probe" }]
        }),
        risk: "low".into(),
    }
}

fn connection(name: &str, secret_ref: Option<&str>) -> ConnectionInput {
    ConnectionInput {
        connector_id: "talentos.rest".into(),
        connector_version: "1.0.0".into(),
        name: name.into(),
        endpoint: "https://api.example.com".into(),
        secret_ref: secret_ref.map(str::to_owned),
        state: None,
    }
}

#[tokio::test]
async fn connector_lifecycle_and_published_immutability() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, _) = seed_tenant_entities(&raw, 1).await;
    let (other_tenant_id, _) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let other = TenantId(other_tenant_id);

    let draft = create_connector(&pool, tenant, &connector("1.0.0"))
        .await
        .expect("create draft");
    assert_eq!(draft.status, "draft");

    let duplicate = create_connector(&pool, tenant, &connector("1.0.0")).await;
    assert!(matches!(
        duplicate,
        Err(IntegrationError::DuplicateConnectorVersion)
    ));

    let updated = update_draft(
        &pool,
        tenant,
        "talentos.rest",
        "1.0.0",
        &ConnectorUpdate {
            name: Some("REST connector".into()),
            risk: Some("medium".into()),
            ..ConnectorUpdate::default()
        },
    )
    .await
    .expect("update draft");
    assert_eq!(updated.name, "REST connector");
    assert_eq!(updated.risk, "medium");

    let published = publish_connector(&pool, tenant, "talentos.rest", "1.0.0")
        .await
        .expect("publish");
    assert_eq!(published.status, "published");

    let edit = update_draft(
        &pool,
        tenant,
        "talentos.rest",
        "1.0.0",
        &ConnectorUpdate {
            name: Some("Nope".into()),
            ..ConnectorUpdate::default()
        },
    )
    .await;
    assert!(matches!(edit, Err(IntegrationError::ConnectorPublished)));

    let again = publish_connector(&pool, tenant, "talentos.rest", "1.0.0").await;
    assert!(matches!(
        again,
        Err(IntegrationError::ConnectorAlreadyPublished)
    ));

    // The DB trigger is the backstop: even a direct write cannot change published content.
    let mut tx = raw.begin().await.expect("begin direct");
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let direct = sqlx::query(
        "UPDATE integration_connectors SET manifest = '{\"tampered\":true}'
          WHERE tenant_id = $1 AND connector_id = 'talentos.rest' AND version = '1.0.0'",
    )
    .bind(tenant_id)
    .execute(&mut *tx)
    .await;
    let _ = tx.rollback().await;
    assert!(direct.is_err(), "trigger must reject published edits");

    // Disabled connectors are hidden from the default listing but still fetchable.
    let disabled = disable_connector(&pool, tenant, "talentos.rest", "1.0.0")
        .await
        .expect("disable");
    assert_eq!(disabled.status, "disabled");
    let (default_list, default_total) = list_connectors(&pool, tenant, false, 50, 0)
        .await
        .expect("list default");
    assert!(default_list.is_empty());
    assert_eq!(default_total, 0);
    let (all, total) = list_connectors(&pool, tenant, true, 50, 0)
        .await
        .expect("list all");
    assert_eq!(total, 1);
    assert_eq!(all[0].status, "disabled");

    let disable_again = disable_connector(&pool, tenant, "talentos.rest", "1.0.0").await;
    assert!(matches!(
        disable_again,
        Err(IntegrationError::ConnectorAlreadyDisabled)
    ));

    // Tenant isolation: another tenant sees nothing.
    let cross = get_connector(&pool, other, "talentos.rest", "1.0.0").await;
    assert!(matches!(cross, Err(IntegrationError::ConnectorNotFound)));
    let (other_list, other_total) = list_connectors(&pool, other, true, 50, 0)
        .await
        .expect("other list");
    assert!(other_list.is_empty());
    assert_eq!(other_total, 0);
}

#[tokio::test]
async fn connection_crud_secret_handling_and_isolation() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let raw = PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 2).await.expect("tenant pool");
    let (tenant_id, _) = seed_tenant_entities(&raw, 1).await;
    let (other_tenant_id, _) = seed_tenant_entities(&raw, 1).await;
    let tenant = TenantId(tenant_id);
    let other = TenantId(other_tenant_id);

    create_connector(&pool, tenant, &connector("1.0.0"))
        .await
        .expect("connector");

    let secret = "secret/data/talentos/tenants/x/connectors/rest";
    let with_secret = create_connection(&pool, tenant, &connection("HRIS prod", Some(secret)))
        .await
        .expect("connection with secret");
    assert!(with_secret.has_secret);
    let serialized = serde_json::to_string(&with_secret).expect("serialize");
    assert!(
        !serialized.contains(secret),
        "secret_ref must never be echoed"
    );

    let without_secret = create_connection(&pool, tenant, &connection("HRIS staging", None))
        .await
        .expect("connection without secret");
    assert!(!without_secret.has_secret);

    let (list, total) = list_connections(&pool, tenant, 50, 0)
        .await
        .expect("list connections");
    assert_eq!(total, 2);
    assert!(list.iter().all(|c| serde_json::to_string(c)
        .expect("serialize")
        .find(secret)
        .is_none()));

    let updated = update_connection(
        &pool,
        tenant,
        with_secret.id,
        &ConnectionUpdate {
            name: Some("HRIS production".into()),
            endpoint: Some("https://api2.example.com".into()),
            state: Some("disabled".into()),
            ..ConnectionUpdate::default()
        },
    )
    .await
    .expect("update connection");
    assert_eq!(updated.name, "HRIS production");
    assert_eq!(updated.state, "disabled");
    assert!(updated.has_secret);

    let duplicate_name = update_connection(
        &pool,
        tenant,
        without_secret.id,
        &ConnectionUpdate {
            name: Some("HRIS production".into()),
            ..ConnectionUpdate::default()
        },
    )
    .await;
    assert!(matches!(
        duplicate_name,
        Err(IntegrationError::DuplicateConnectionName)
    ));

    let ok_test = record_connection_test(
        &pool,
        tenant,
        with_secret.id,
        &ConnectionTest {
            ok: true,
            latency_ms: Some(42),
            error: None,
        },
    )
    .await
    .expect("record ok test");
    assert!(ok_test.last_verified_at.is_some());
    assert_eq!(ok_test.last_test.expect("last_test")["ok"], json!(true));

    let failed_test = record_connection_test(
        &pool,
        tenant,
        with_secret.id,
        &ConnectionTest {
            ok: false,
            latency_ms: None,
            error: Some("connector.timeout".into()),
        },
    )
    .await
    .expect("record failed test");
    assert!(failed_test.last_verified_at.is_none());

    let missing_connector = create_connection(
        &pool,
        tenant,
        &ConnectionInput {
            connector_version: "9.9.9".into(),
            ..connection("Missing", None)
        },
    )
    .await;
    assert!(matches!(
        missing_connector,
        Err(IntegrationError::ConnectorNotFound)
    ));

    // Tenant isolation.
    let cross = get_connection(&pool, other, with_secret.id).await;
    assert!(matches!(cross, Err(IntegrationError::ConnectionNotFound)));
    let (other_list, other_total) = list_connections(&pool, other, 50, 0)
        .await
        .expect("other list");
    assert!(other_list.is_empty());
    assert_eq!(other_total, 0);

    delete_connection(&pool, tenant, with_secret.id)
        .await
        .expect("delete connection");
    let gone = get_connection(&pool, tenant, with_secret.id).await;
    assert!(matches!(gone, Err(IntegrationError::ConnectionNotFound)));
    let delete_again = delete_connection(&pool, tenant, with_secret.id).await;
    assert!(matches!(
        delete_again,
        Err(IntegrationError::ConnectionNotFound)
    ));
}

#[test]
fn connection_view_has_no_secret_ref_field() {
    let view = talentos_hr_runner::integration::ConnectionView {
        id: Uuid::nil(),
        connector_id: "talentos.rest".into(),
        connector_version: "1.0.0".into(),
        name: "n".into(),
        endpoint: "https://x".into(),
        state: "active".into(),
        has_secret: true,
        last_test: None,
        last_verified_at: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let value = serde_json::to_value(view).expect("serialize");
    assert!(value.get("secret_ref").is_none());
    assert_eq!(value["has_secret"], json!(true));
}

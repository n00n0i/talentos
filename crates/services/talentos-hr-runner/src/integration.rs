//! Integration Control Plane: connector registry and tenant connections.
//!
//! Tenant isolation is `begin_tenant` + explicit `tenant_id` predicates. Connector
//! versions are immutable once published: the service rejects draft edits and the
//! database trigger `integration_connectors_immutable` enforces it for any writer.
//! Secrets are stored as OpenBao references only; views expose `has_secret`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

pub const CONNECTION_STATES: &[&str] = &["active", "disabled"];

#[derive(Debug, thiserror::Error)]
pub enum IntegrationError {
    #[error("connector_id must be 1-200 characters")]
    InvalidConnectorId,
    #[error("version must be 1-50 characters")]
    InvalidVersion,
    #[error("name must be 1-200 characters")]
    InvalidName,
    #[error("protocol must be 1-50 characters")]
    InvalidProtocol,
    #[error("risk must be 1-50 characters")]
    InvalidRisk,
    #[error("manifest must be a JSON object")]
    InvalidManifest,
    #[error("endpoint must be 1-2000 characters")]
    InvalidEndpoint,
    #[error("secret_ref must be 1-1000 characters when provided")]
    InvalidSecretRef,
    #[error("connection state must be active or disabled")]
    InvalidState,
    #[error("test result latency_ms must not be negative")]
    InvalidTestResult,
    #[error("connector not found")]
    ConnectorNotFound,
    #[error("connection not found")]
    ConnectionNotFound,
    #[error("published connector versions are immutable")]
    ConnectorPublished,
    #[error("disabled connectors cannot be edited")]
    ConnectorDisabled,
    #[error("connector version is already published")]
    ConnectorAlreadyPublished,
    #[error("connector version is already disabled")]
    ConnectorAlreadyDisabled,
    #[error("connector version already exists")]
    DuplicateConnectorVersion,
    #[error("connection name already exists")]
    DuplicateConnectionName,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

fn required(value: &str, max: usize, error: IntegrationError) -> Result<String, IntegrationError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(error);
    }
    Ok(value.to_owned())
}

fn unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}

/// A database trigger uses `check_violation` for the published-immutability guard.
fn published_immutable_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db)
        if db.code().as_deref() == Some("23514")
            && db.message().contains("published connector versions are immutable"))
}

#[derive(Debug, Clone)]
pub struct ConnectorInput {
    pub connector_id: String,
    pub name: String,
    pub version: String,
    pub protocol: String,
    pub manifest: Value,
    pub risk: String,
}

#[derive(Debug, Clone, Default)]
pub struct ConnectorUpdate {
    pub name: Option<String>,
    pub protocol: Option<String>,
    pub manifest: Option<Value>,
    pub risk: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectorView {
    pub id: Uuid,
    pub connector_id: String,
    pub name: String,
    pub version: String,
    pub protocol: String,
    pub manifest: Value,
    pub risk: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const CONNECTOR_COLUMNS: &str = "id, connector_id, name, version, protocol, manifest, risk, \
                                 status, created_at, updated_at";

fn map_connector(row: &sqlx::postgres::PgRow) -> Result<ConnectorView, sqlx::Error> {
    Ok(ConnectorView {
        id: row.try_get("id")?,
        connector_id: row.try_get("connector_id")?,
        name: row.try_get("name")?,
        version: row.try_get("version")?,
        protocol: row.try_get("protocol")?,
        manifest: row.try_get("manifest")?,
        risk: row.try_get("risk")?,
        status: row.try_get("status")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub async fn create_connector(
    pool: &TenantPool,
    tenant_id: TenantId,
    input: &ConnectorInput,
) -> Result<ConnectorView, IntegrationError> {
    let connector_id = required(
        &input.connector_id,
        200,
        IntegrationError::InvalidConnectorId,
    )?;
    let name = required(&input.name, 200, IntegrationError::InvalidName)?;
    let version = required(&input.version, 50, IntegrationError::InvalidVersion)?;
    let protocol = required(&input.protocol, 50, IntegrationError::InvalidProtocol)?;
    let risk = required(&input.risk, 50, IntegrationError::InvalidRisk)?;
    if !input.manifest.is_object() {
        return Err(IntegrationError::InvalidManifest);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(&format!(
        "INSERT INTO integration_connectors
             (tenant_id, connector_id, name, version, protocol, manifest, risk)
         VALUES ($1,$2,$3,$4,$5,$6,$7)
         RETURNING {CONNECTOR_COLUMNS}"
    ))
    .bind(tenant_id.0)
    .bind(&connector_id)
    .bind(&name)
    .bind(&version)
    .bind(&protocol)
    .bind(&input.manifest)
    .bind(&risk)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if unique_violation(&e) {
            IntegrationError::DuplicateConnectorVersion
        } else {
            IntegrationError::Db(e)
        }
    })?;
    let view = map_connector(&row)?;
    tx.commit().await?;
    Ok(view)
}

pub async fn list_connectors(
    pool: &TenantPool,
    tenant_id: TenantId,
    include_disabled: bool,
    limit: i64,
    offset: i64,
) -> Result<(Vec<ConnectorView>, i64), IntegrationError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(&format!(
        "SELECT {CONNECTOR_COLUMNS}
           FROM integration_connectors
          WHERE tenant_id = $1 AND ($2 OR status <> 'disabled')
          ORDER BY created_at DESC, id
          LIMIT $3 OFFSET $4"
    ))
    .bind(tenant_id.0)
    .bind(include_disabled)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await?;
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM integration_connectors
          WHERE tenant_id = $1 AND ($2 OR status <> 'disabled')",
    )
    .bind(tenant_id.0)
    .bind(include_disabled)
    .fetch_one(&mut *tx)
    .await?;
    let views = rows
        .iter()
        .map(map_connector)
        .collect::<Result<Vec<_>, _>>()?;
    tx.commit().await?;
    Ok((views, total))
}

pub async fn get_connector(
    pool: &TenantPool,
    tenant_id: TenantId,
    connector_id: &str,
    version: &str,
) -> Result<ConnectorView, IntegrationError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(&format!(
        "SELECT {CONNECTOR_COLUMNS}
           FROM integration_connectors
          WHERE tenant_id = $1 AND connector_id = $2 AND version = $3"
    ))
    .bind(tenant_id.0)
    .bind(connector_id)
    .bind(version)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(IntegrationError::ConnectorNotFound)?;
    let view = map_connector(&row)?;
    tx.commit().await?;
    Ok(view)
}

/// Edit a draft. A published row is rejected with [`IntegrationError::ConnectorPublished`];
/// a disabled row cannot be edited either.
pub async fn update_draft(
    pool: &TenantPool,
    tenant_id: TenantId,
    connector_id: &str,
    version: &str,
    update: &ConnectorUpdate,
) -> Result<ConnectorView, IntegrationError> {
    let name = update
        .name
        .as_deref()
        .map(|v| required(v, 200, IntegrationError::InvalidName))
        .transpose()?;
    let protocol = update
        .protocol
        .as_deref()
        .map(|v| required(v, 50, IntegrationError::InvalidProtocol))
        .transpose()?;
    let risk = update
        .risk
        .as_deref()
        .map(|v| required(v, 50, IntegrationError::InvalidRisk))
        .transpose()?;
    if update.manifest.as_ref().is_some_and(|m| !m.is_object()) {
        return Err(IntegrationError::InvalidManifest);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let current = sqlx::query(&format!(
        "SELECT {CONNECTOR_COLUMNS}
           FROM integration_connectors
          WHERE tenant_id = $1 AND connector_id = $2 AND version = $3"
    ))
    .bind(tenant_id.0)
    .bind(connector_id)
    .bind(version)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(IntegrationError::ConnectorNotFound)?;
    let current = map_connector(&current)?;
    match current.status.as_str() {
        "published" => return Err(IntegrationError::ConnectorPublished),
        "disabled" => return Err(IntegrationError::ConnectorDisabled),
        _ => {}
    }

    let row = sqlx::query(&format!(
        "UPDATE integration_connectors
            SET name = $4, protocol = $5, manifest = $6, risk = $7, updated_at = now()
          WHERE tenant_id = $1 AND connector_id = $2 AND version = $3
          RETURNING {CONNECTOR_COLUMNS}"
    ))
    .bind(tenant_id.0)
    .bind(connector_id)
    .bind(version)
    .bind(name.unwrap_or(current.name))
    .bind(protocol.unwrap_or(current.protocol))
    .bind(update.manifest.clone().unwrap_or(current.manifest))
    .bind(risk.unwrap_or(current.risk))
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if published_immutable_violation(&e) {
            IntegrationError::ConnectorPublished
        } else {
            IntegrationError::Db(e)
        }
    })?;
    let view = map_connector(&row)?;
    tx.commit().await?;
    Ok(view)
}

pub async fn publish_connector(
    pool: &TenantPool,
    tenant_id: TenantId,
    connector_id: &str,
    version: &str,
) -> Result<ConnectorView, IntegrationError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(&format!(
        "UPDATE integration_connectors
            SET status = 'published', updated_at = now()
          WHERE tenant_id = $1 AND connector_id = $2 AND version = $3 AND status = 'draft'
          RETURNING {CONNECTOR_COLUMNS}"
    ))
    .bind(tenant_id.0)
    .bind(connector_id)
    .bind(version)
    .fetch_optional(&mut *tx)
    .await?;

    let view = match row {
        Some(row) => map_connector(&row)?,
        None => {
            let status: Option<String> = sqlx::query_scalar(
                "SELECT status FROM integration_connectors
                  WHERE tenant_id = $1 AND connector_id = $2 AND version = $3",
            )
            .bind(tenant_id.0)
            .bind(connector_id)
            .bind(version)
            .fetch_optional(&mut *tx)
            .await?;
            return match status.as_deref() {
                None => Err(IntegrationError::ConnectorNotFound),
                Some("published") => Err(IntegrationError::ConnectorAlreadyPublished),
                Some(_) => Err(IntegrationError::ConnectorDisabled),
            };
        }
    };
    tx.commit().await?;
    Ok(view)
}

pub async fn disable_connector(
    pool: &TenantPool,
    tenant_id: TenantId,
    connector_id: &str,
    version: &str,
) -> Result<ConnectorView, IntegrationError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(&format!(
        "UPDATE integration_connectors
            SET status = 'disabled', updated_at = now()
          WHERE tenant_id = $1 AND connector_id = $2 AND version = $3 AND status <> 'disabled'
          RETURNING {CONNECTOR_COLUMNS}"
    ))
    .bind(tenant_id.0)
    .bind(connector_id)
    .bind(version)
    .fetch_optional(&mut *tx)
    .await?;

    let view = match row {
        Some(row) => map_connector(&row)?,
        None => {
            let status: Option<String> = sqlx::query_scalar(
                "SELECT status FROM integration_connectors
                  WHERE tenant_id = $1 AND connector_id = $2 AND version = $3",
            )
            .bind(tenant_id.0)
            .bind(connector_id)
            .bind(version)
            .fetch_optional(&mut *tx)
            .await?;
            return match status.as_deref() {
                None => Err(IntegrationError::ConnectorNotFound),
                Some("disabled") => Err(IntegrationError::ConnectorAlreadyDisabled),
                Some(_) => Err(IntegrationError::ConnectorDisabled),
            };
        }
    };
    tx.commit().await?;
    Ok(view)
}

#[derive(Debug, Clone)]
pub struct ConnectionInput {
    pub connector_id: String,
    pub connector_version: String,
    pub name: String,
    pub endpoint: String,
    pub secret_ref: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ConnectionUpdate {
    pub name: Option<String>,
    pub endpoint: Option<String>,
    pub secret_ref: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionView {
    pub id: Uuid,
    pub connector_id: String,
    pub connector_version: String,
    pub name: String,
    pub endpoint: String,
    pub state: String,
    pub has_secret: bool,
    pub last_test: Option<Value>,
    pub last_verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const CONNECTION_COLUMNS: &str =
    "id, connector_id, connector_version, name, endpoint, secret_ref, \
                                  state, last_test, last_verified_at, created_at, updated_at";

fn map_connection(row: &sqlx::postgres::PgRow) -> Result<ConnectionView, sqlx::Error> {
    Ok(ConnectionView {
        id: row.try_get("id")?,
        connector_id: row.try_get("connector_id")?,
        connector_version: row.try_get("connector_version")?,
        name: row.try_get("name")?,
        endpoint: row.try_get("endpoint")?,
        state: row.try_get("state")?,
        has_secret: row.try_get::<Option<String>, _>("secret_ref")?.is_some(),
        last_test: row.try_get("last_test")?,
        last_verified_at: row.try_get("last_verified_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn validate_state(state: &str) -> Result<String, IntegrationError> {
    if CONNECTION_STATES.contains(&state) {
        Ok(state.to_owned())
    } else {
        Err(IntegrationError::InvalidState)
    }
}

fn validate_secret_ref(secret_ref: Option<&str>) -> Result<Option<String>, IntegrationError> {
    match secret_ref {
        None => Ok(None),
        Some(value) => required(value, 1000, IntegrationError::InvalidSecretRef).map(Some),
    }
}

pub async fn create_connection(
    pool: &TenantPool,
    tenant_id: TenantId,
    input: &ConnectionInput,
) -> Result<ConnectionView, IntegrationError> {
    let connector_id = required(
        &input.connector_id,
        200,
        IntegrationError::InvalidConnectorId,
    )?;
    let connector_version = required(
        &input.connector_version,
        50,
        IntegrationError::InvalidVersion,
    )?;
    let name = required(&input.name, 200, IntegrationError::InvalidName)?;
    let endpoint = required(&input.endpoint, 2000, IntegrationError::InvalidEndpoint)?;
    let secret_ref = validate_secret_ref(input.secret_ref.as_deref())?;
    let state = match input.state.as_deref() {
        Some(state) => validate_state(state)?,
        None => "active".to_owned(),
    };

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let connector_exists: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM integration_connectors
          WHERE tenant_id = $1 AND connector_id = $2 AND version = $3",
    )
    .bind(tenant_id.0)
    .bind(&connector_id)
    .bind(&connector_version)
    .fetch_optional(&mut *tx)
    .await?;
    if connector_exists.is_none() {
        return Err(IntegrationError::ConnectorNotFound);
    }

    let row = sqlx::query(&format!(
        "INSERT INTO integration_connections
             (tenant_id, connector_id, connector_version, name, endpoint, secret_ref, state)
         VALUES ($1,$2,$3,$4,$5,$6,$7)
         RETURNING {CONNECTION_COLUMNS}"
    ))
    .bind(tenant_id.0)
    .bind(&connector_id)
    .bind(&connector_version)
    .bind(&name)
    .bind(&endpoint)
    .bind(&secret_ref)
    .bind(&state)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if unique_violation(&e) {
            IntegrationError::DuplicateConnectionName
        } else {
            IntegrationError::Db(e)
        }
    })?;
    let view = map_connection(&row)?;
    tx.commit().await?;
    Ok(view)
}

pub async fn list_connections(
    pool: &TenantPool,
    tenant_id: TenantId,
    limit: i64,
    offset: i64,
) -> Result<(Vec<ConnectionView>, i64), IntegrationError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(&format!(
        "SELECT {CONNECTION_COLUMNS}
           FROM integration_connections
          WHERE tenant_id = $1
          ORDER BY created_at DESC, id
          LIMIT $2 OFFSET $3"
    ))
    .bind(tenant_id.0)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await?;
    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM integration_connections WHERE tenant_id = $1")
            .bind(tenant_id.0)
            .fetch_one(&mut *tx)
            .await?;
    let views = rows
        .iter()
        .map(map_connection)
        .collect::<Result<Vec<_>, _>>()?;
    tx.commit().await?;
    Ok((views, total))
}

pub async fn get_connection(
    pool: &TenantPool,
    tenant_id: TenantId,
    id: Uuid,
) -> Result<ConnectionView, IntegrationError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(&format!(
        "SELECT {CONNECTION_COLUMNS}
           FROM integration_connections
          WHERE tenant_id = $1 AND id = $2"
    ))
    .bind(tenant_id.0)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(IntegrationError::ConnectionNotFound)?;
    let view = map_connection(&row)?;
    tx.commit().await?;
    Ok(view)
}

pub async fn update_connection(
    pool: &TenantPool,
    tenant_id: TenantId,
    id: Uuid,
    update: &ConnectionUpdate,
) -> Result<ConnectionView, IntegrationError> {
    let name = update
        .name
        .as_deref()
        .map(|v| required(v, 200, IntegrationError::InvalidName))
        .transpose()?;
    let endpoint = update
        .endpoint
        .as_deref()
        .map(|v| required(v, 2000, IntegrationError::InvalidEndpoint))
        .transpose()?;
    let secret_ref = validate_secret_ref(update.secret_ref.as_deref())?;
    let state = update.state.as_deref().map(validate_state).transpose()?;

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(&format!(
        "UPDATE integration_connections
            SET name = COALESCE($3, name),
                endpoint = COALESCE($4, endpoint),
                secret_ref = COALESCE($5, secret_ref),
                state = COALESCE($6, state),
                updated_at = now()
          WHERE tenant_id = $1 AND id = $2
          RETURNING {CONNECTION_COLUMNS}"
    ))
    .bind(tenant_id.0)
    .bind(id)
    .bind(&name)
    .bind(&endpoint)
    .bind(&secret_ref)
    .bind(&state)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| {
        if unique_violation(&e) {
            IntegrationError::DuplicateConnectionName
        } else {
            IntegrationError::Db(e)
        }
    })?
    .ok_or(IntegrationError::ConnectionNotFound)?;
    let view = map_connection(&row)?;
    tx.commit().await?;
    Ok(view)
}

pub async fn delete_connection(
    pool: &TenantPool,
    tenant_id: TenantId,
    id: Uuid,
) -> Result<(), IntegrationError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let deleted: Option<Uuid> = sqlx::query_scalar(
        "DELETE FROM integration_connections WHERE tenant_id = $1 AND id = $2 RETURNING id",
    )
    .bind(tenant_id.0)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if deleted.is_none() {
        return Err(IntegrationError::ConnectionNotFound);
    }
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionTest {
    pub ok: bool,
    pub latency_ms: Option<i64>,
    pub error: Option<String>,
}

pub async fn record_connection_test(
    pool: &TenantPool,
    tenant_id: TenantId,
    id: Uuid,
    test: &ConnectionTest,
) -> Result<ConnectionView, IntegrationError> {
    if test.latency_ms.is_some_and(|ms| ms < 0) {
        return Err(IntegrationError::InvalidTestResult);
    }
    let result = serde_json::to_value(test).map_err(|_| IntegrationError::InvalidTestResult)?;

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(&format!(
        "UPDATE integration_connections
            SET last_test = $3,
                last_verified_at = CASE WHEN $4 THEN now() ELSE NULL END,
                updated_at = now()
          WHERE tenant_id = $1 AND id = $2
          RETURNING {CONNECTION_COLUMNS}"
    ))
    .bind(tenant_id.0)
    .bind(id)
    .bind(&result)
    .bind(test.ok)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(IntegrationError::ConnectionNotFound)?;
    let view = map_connection(&row)?;
    tx.commit().await?;
    Ok(view)
}

//! Integration Control Plane: connector registry and tenant connection management.
//!
//! Secrets never leave OpenBao: connection responses expose `has_secret` only.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;
use talentos_hr_runner::integration::{
    self, ConnectionInput, ConnectionTest, ConnectionUpdate, ConnectorInput, ConnectorUpdate,
    IntegrationError,
};

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ConnectorListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub include_disabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct VersionQuery {
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateConnectorBody {
    pub connector_id: String,
    pub name: String,
    pub version: String,
    pub protocol: String,
    pub manifest: Value,
    pub risk: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConnectorBody {
    pub name: Option<String>,
    pub protocol: Option<String>,
    pub manifest: Option<Value>,
    pub risk: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VersionBody {
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateConnectionBody {
    pub connector_id: String,
    pub connector_version: String,
    pub name: String,
    pub endpoint: String,
    pub secret_ref: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConnectionBody {
    pub name: Option<String>,
    pub endpoint: Option<String>,
    pub secret_ref: Option<String>,
    pub state: Option<String>,
}

fn page(limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
    (
        limit.unwrap_or(50).clamp(1, 200),
        offset.unwrap_or(0).max(0),
    )
}

/// GET /api/v1/integrations/connectors
pub async fn list_connectors(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<ConnectorListQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "integration", "read") {
        return forbidden;
    }
    let (limit, offset) = page(query.limit, query.offset);
    match integration::list_connectors(
        &state.pool,
        ctx.tenant_id,
        query.include_disabled.unwrap_or(false),
        limit,
        offset,
    )
    .await
    {
        Ok((items, total)) => Json(json!({
            "items": items,
            "total": total,
            "limit": limit,
            "offset": offset,
            "next_offset": (offset + limit < total).then_some(offset + limit),
        }))
        .into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/integrations/connectors
pub async fn create_connector(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<CreateConnectorBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "integration", "manage") {
        return forbidden;
    }
    let input = ConnectorInput {
        connector_id: body.connector_id,
        name: body.name,
        version: body.version,
        protocol: body.protocol,
        manifest: body.manifest,
        risk: body.risk,
    };
    match integration::create_connector(&state.pool, ctx.tenant_id, &input).await {
        Ok(connector) => {
            (StatusCode::CREATED, Json(json!({ "connector": connector }))).into_response()
        }
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/integrations/connectors/:connector_id?version=
pub async fn get_connector(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(connector_id): Path<String>,
    Query(query): Query<VersionQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "integration", "read") {
        return forbidden;
    }
    let Some(version) = query.version else {
        return invalid("version query parameter is required");
    };
    match integration::get_connector(&state.pool, ctx.tenant_id, &connector_id, &version).await {
        Ok(connector) => Json(json!({ "connector": connector })).into_response(),
        Err(e) => map_error(e),
    }
}

/// PATCH /api/v1/integrations/connectors/:connector_id?version=
pub async fn update_connector(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(connector_id): Path<String>,
    Query(query): Query<VersionQuery>,
    Json(body): Json<UpdateConnectorBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "integration", "manage") {
        return forbidden;
    }
    let Some(version) = query.version else {
        return invalid("version query parameter is required");
    };
    let update = ConnectorUpdate {
        name: body.name,
        protocol: body.protocol,
        manifest: body.manifest,
        risk: body.risk,
    };
    match integration::update_draft(&state.pool, ctx.tenant_id, &connector_id, &version, &update)
        .await
    {
        Ok(connector) => Json(json!({ "connector": connector })).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/integrations/connectors/:connector_id/publish
pub async fn publish_connector(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(connector_id): Path<String>,
    Json(body): Json<VersionBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "integration", "manage") {
        return forbidden;
    }
    match integration::publish_connector(&state.pool, ctx.tenant_id, &connector_id, &body.version)
        .await
    {
        Ok(connector) => Json(json!({ "connector": connector })).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/integrations/connectors/:connector_id/disable
pub async fn disable_connector(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(connector_id): Path<String>,
    Json(body): Json<VersionBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "integration", "manage") {
        return forbidden;
    }
    match integration::disable_connector(&state.pool, ctx.tenant_id, &connector_id, &body.version)
        .await
    {
        Ok(connector) => Json(json!({ "connector": connector })).into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/integrations/connections
pub async fn list_connections(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<PageQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "integration", "read") {
        return forbidden;
    }
    let (limit, offset) = page(query.limit, query.offset);
    match integration::list_connections(&state.pool, ctx.tenant_id, limit, offset).await {
        Ok((items, total)) => Json(json!({
            "items": items,
            "total": total,
            "limit": limit,
            "offset": offset,
            "next_offset": (offset + limit < total).then_some(offset + limit),
        }))
        .into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/integrations/connections
pub async fn create_connection(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<CreateConnectionBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "integration", "manage") {
        return forbidden;
    }
    let input = ConnectionInput {
        connector_id: body.connector_id,
        connector_version: body.connector_version,
        name: body.name,
        endpoint: body.endpoint,
        secret_ref: body.secret_ref,
        state: body.state,
    };
    match integration::create_connection(&state.pool, ctx.tenant_id, &input).await {
        Ok(connection) => (
            StatusCode::CREATED,
            Json(json!({ "connection": connection })),
        )
            .into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/integrations/connections/:id
pub async fn get_connection(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "integration", "read") {
        return forbidden;
    }
    match integration::get_connection(&state.pool, ctx.tenant_id, id).await {
        Ok(connection) => Json(json!({ "connection": connection })).into_response(),
        Err(e) => map_error(e),
    }
}

/// PATCH /api/v1/integrations/connections/:id
pub async fn update_connection(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateConnectionBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "integration", "manage") {
        return forbidden;
    }
    let update = ConnectionUpdate {
        name: body.name,
        endpoint: body.endpoint,
        secret_ref: body.secret_ref,
        state: body.state,
    };
    match integration::update_connection(&state.pool, ctx.tenant_id, id, &update).await {
        Ok(connection) => Json(json!({ "connection": connection })).into_response(),
        Err(e) => map_error(e),
    }
}

/// DELETE /api/v1/integrations/connections/:id
pub async fn delete_connection(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "integration", "manage") {
        return forbidden;
    }
    match integration::delete_connection(&state.pool, ctx.tenant_id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/integrations/connections/:id/test — records an externally run test.
pub async fn record_connection_test(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<ConnectionTest>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "integration", "manage") {
        return forbidden;
    }
    match integration::record_connection_test(&state.pool, ctx.tenant_id, id, &body).await {
        Ok(connection) => Json(json!({ "connection": connection })).into_response(),
        Err(e) => map_error(e),
    }
}

fn map_error(e: IntegrationError) -> Response {
    use IntegrationError::*;
    match e {
        ConnectorNotFound | ConnectionNotFound => {
            error(StatusCode::NOT_FOUND, "not_found", &e.to_string())
        }
        ConnectorPublished => error(
            StatusCode::CONFLICT,
            "connector_published_immutable",
            &e.to_string(),
        ),
        ConnectorDisabled => error(StatusCode::CONFLICT, "connector_disabled", &e.to_string()),
        ConnectorAlreadyPublished => {
            error(StatusCode::CONFLICT, "already_published", &e.to_string())
        }
        ConnectorAlreadyDisabled => error(StatusCode::CONFLICT, "already_disabled", &e.to_string()),
        DuplicateConnectorVersion => {
            error(StatusCode::CONFLICT, "version_conflict", &e.to_string())
        }
        DuplicateConnectionName => error(StatusCode::CONFLICT, "name_conflict", &e.to_string()),
        InvalidConnectorId | InvalidVersion | InvalidName | InvalidProtocol | InvalidRisk
        | InvalidManifest | InvalidEndpoint | InvalidSecretRef | InvalidState
        | InvalidTestResult => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            &e.to_string(),
        ),
        Db(_) => {
            tracing::error!(error = %e, "integration endpoint failed");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "An unexpected error occurred",
            )
        }
    }
}

fn invalid(message: &str) -> Response {
    error(StatusCode::BAD_REQUEST, "invalid_request", message)
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: code.into(),
            message: message.into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

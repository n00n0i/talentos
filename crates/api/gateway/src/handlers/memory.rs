//! Agent memory endpoints (pgvector-backed).

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use talentos_memory::{Layer, MemoryError, NewMemory};

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct RememberBody {
    pub layer: String,
    pub content: String,
    pub importance: Option<f32>,
    pub scope_key: Option<String>,
    pub metadata: Option<Value>,
    pub ttl_seconds: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct RecallQuery {
    #[serde(default)]
    pub q: String,
    /// Comma-separated layer names; absent means all four.
    pub layers: Option<String>,
    pub limit: Option<u32>,
}

/// POST /api/v1/ai/memory
pub async fn remember(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<RememberBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    let Some(layer) = Layer::parse(&body.layer) else {
        return invalid(&format!(
            "layer must be one of working, episodic, semantic, procedural (got '{}')",
            body.layer
        ));
    };

    let mut memory = NewMemory::new(layer, &body.content);
    if let Some(importance) = body.importance {
        memory.importance = importance;
    }
    memory.scope_key = body.scope_key.as_deref().unwrap_or("");
    memory.metadata = body.metadata.unwrap_or_else(|| json!({}));
    memory.ttl_seconds = body.ttl_seconds;

    match state
        .memory
        .remember(ctx.tenant_id, Some(ctx.user_id), memory)
        .await
    {
        Ok(id) => (
            StatusCode::CREATED,
            Json(json!({ "id": id, "layer": layer.as_str() })),
        )
            .into_response(),
        Err(e) => memory_error(e),
    }
}

/// GET /api/v1/ai/memory?q=…&layers=episodic,semantic&limit=10
pub async fn recall(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<RecallQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    let layers = match query.layers.as_deref() {
        None | Some("") => Layer::ALL.to_vec(),
        Some(raw) => {
            let mut parsed = Vec::new();
            for name in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                match Layer::parse(name) {
                    Some(layer) => parsed.push(layer),
                    None => return invalid(&format!("unknown memory layer '{name}'")),
                }
            }
            if parsed.is_empty() {
                return invalid("layers must name at least one layer");
            }
            parsed
        }
    };

    let limit = query.limit.unwrap_or(10).clamp(1, 100);
    match state
        .memory
        .recall(ctx.tenant_id, &query.q, &layers, limit)
        .await
    {
        Ok(hits) => (StatusCode::OK, Json(talentos_memory::hits_json(&hits))).into_response(),
        Err(e) => memory_error(e),
    }
}

/// GET /api/v1/ai/memory/stats
pub async fn stats(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match state.memory.stats(ctx.tenant_id).await {
        Ok(stats) => (StatusCode::OK, Json(json!({ "layers": stats }))).into_response(),
        Err(e) => memory_error(e),
    }
}

/// DELETE /api/v1/ai/memory/expired
pub async fn purge(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match state.memory.forget_expired(ctx.tenant_id).await {
        Ok(purged) => (StatusCode::OK, Json(json!({ "purged": purged }))).into_response(),
        Err(e) => memory_error(e),
    }
}

fn invalid(message: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(ErrorResponse {
            error: "invalid_memory".into(),
            message: message.to_string(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn memory_error(error: MemoryError) -> Response {
    match error {
        MemoryError::Invalid(message) => invalid(&message),
        MemoryError::Database(e) => {
            tracing::error!(error = %e, "memory query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "internal_error".into(),
                    message: "An unexpected error occurred".into(),
                    details: vec![],
                    request_id: Uuid::new_v4().to_string(),
                    retry_after_seconds: None,
                }),
            )
                .into_response()
        }
    }
}

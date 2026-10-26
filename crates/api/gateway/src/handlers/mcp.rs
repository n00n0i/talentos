//! MCP streamable HTTP endpoint.
//!
//! One POST carries one JSON-RPC message. There is no session: the JWT on the
//! request is the only identity, which is what makes the server stateless and
//! safe to scale horizontally.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use uuid::Uuid;

use talentos_model_gw::HuakatiClient;
use talentos_tools::{ToolContext, ToolRegistry};

use crate::middleware::auth::Authed;
use crate::state::AppState;

const ALLOW_DESTRUCTIVE: &str = "x-talentos-allow-destructive";

/// POST /api/v1/mcp
pub async fn post(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let message: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return json_rpc_error(-32700, &format!("parse error: {e}"));
        }
    };

    let model_gw = HuakatiClient::new().ok().map(Arc::new);
    let registry: ToolRegistry =
        talentos_tools::tools::default_registry(model_gw.clone(), state.pool.clone());
    let tool_ctx = ToolContext {
        tenant_id: ctx.tenant_id,
        user_id: ctx.user_id,
        entity_id: ctx.entity_id,
        run_id: Uuid::new_v4(),
        db: state.pool.clone(),
        model_gw,
        extensions: Default::default(),
    };
    let allow_destructive = headers
        .get(ALLOW_DESTRUCTIVE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));

    match talentos_mcp::handle(&message, &registry, &tool_ctx, allow_destructive).await {
        // Notifications (e.g. notifications/initialized) have no response body.
        None => StatusCode::ACCEPTED.into_response(),
        Some(reply) => {
            let mut response = (StatusCode::OK, Json(reply)).into_response();
            // Clients send this after initialize; echoing it keeps them happy
            // without the server having to remember anything.
            if let Ok(value) = talentos_mcp::LATEST_PROTOCOL_VERSION.parse() {
                response.headers_mut().insert("mcp-protocol-version", value);
            }
            response
        }
    }
}

/// GET /api/v1/mcp — no server-initiated SSE stream is offered.
pub async fn method_not_allowed() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({
            "jsonrpc": "2.0",
            "error": { "code": -32600, "message": "only POST is supported (no server-initiated stream)" }
        })),
    )
        .into_response()
}

/// DELETE /api/v1/mcp — stateless server, nothing to terminate.
pub async fn session_close() -> StatusCode {
    StatusCode::NO_CONTENT
}

fn json_rpc_error(code: i64, message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "jsonrpc": "2.0",
            "id": Value::Null,
            "error": { "code": code, "message": message }
        })),
    )
        .into_response()
}

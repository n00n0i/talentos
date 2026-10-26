//! Tenant WASM plugin endpoints.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use talentos_wasm_host::{Capability, CapabilitySet, PluginHost, PluginLimits, PluginSpec};

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::Authed;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct InvokeBody {
    pub function: String,
    #[serde(default)]
    pub input: serde_json::Value,
}

/// GET /api/v1/plugins — plugins enabled for this tenant (never returns bytes).
pub async fn list(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let rows = sqlx::query(
        r#"SELECT name, capabilities, enabled, octet_length(wasm) AS size
           FROM tenant_plugins WHERE tenant_id = $1 ORDER BY name"#,
    )
    .bind(ctx.tenant_id.0)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    match rows {
        Ok(rows) => {
            use sqlx::Row;
            let plugins: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "name": r.try_get::<String, _>("name").unwrap_or_default(),
                        "capabilities": r.try_get::<Vec<String>, _>("capabilities").unwrap_or_default(),
                        "enabled": r.try_get::<bool, _>("enabled").unwrap_or(false),
                        "bytes": r.try_get::<i32, _>("size").unwrap_or(0),
                    })
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "plugins": plugins })),
            )
                .into_response()
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

/// POST /api/v1/plugins/:name/invoke — run a plugin function in the sandbox.
pub async fn invoke(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(name): Path<String>,
    Json(body): Json<InvokeBody>,
) -> Response {
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let row = sqlx::query(
        r#"SELECT wasm, capabilities FROM tenant_plugins
           WHERE tenant_id = $1 AND name = $2 AND enabled"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(&name)
    .fetch_optional(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    let row = match row {
        Ok(Some(r)) => r,
        Ok(None) => return not_found(&name),
        Err(e) => return internal_err(&e.to_string()),
    };

    use sqlx::Row;
    let wasm: Vec<u8> = match row.try_get("wasm") {
        Ok(w) => w,
        Err(e) => return internal_err(&e.to_string()),
    };
    let caps: Vec<String> = row.try_get("capabilities").unwrap_or_default();
    let requires = CapabilitySet::of(caps.iter().filter_map(|c| Capability::parse(c)));

    let tenant_id = ctx.tenant_id;
    let plugin_name = name.clone();
    let function = body.function.clone();
    let input = body.input.clone();

    // Extism is synchronous; keep it off the async reactor.
    let result = tokio::task::spawn_blocking(move || {
        let mut host = PluginHost::new(PluginLimits::default());
        let declared = requires.clone();
        host.register(
            tenant_id,
            PluginSpec {
                name: plugin_name.clone(),
                wasm,
                requires: declared,
            },
            requires,
        )?;
        host.invoke(tenant_id, &plugin_name, &function, input)
    })
    .await;

    match result {
        Ok(Ok(outcome)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "plugin": outcome.plugin, "output": outcome.output })),
        )
            .into_response(),
        Ok(Err(e)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorResponse {
                error: "plugin_error".into(),
                message: e.to_string(),
                details: vec![],
                request_id: Uuid::new_v4().to_string(),
                retry_after_seconds: None,
            }),
        )
            .into_response(),
        Err(e) => internal_err(&e.to_string()),
    }
}

fn not_found(name: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "not_found".into(),
            message: format!("plugin '{name}' is not installed for this tenant"),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "plugin endpoint failed");
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

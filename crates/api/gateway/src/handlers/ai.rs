//! AI surface: list available tools and run the agent loop on a task.
//!
//! The agent runs in-process; see `docs/BUILD_STATUS.md` for the Temporal swap.

use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use talentos_agent::{AgentRunner, RunReport};
use talentos_kernel::shared::{AgentContext, AgentResult};
use talentos_model_gw::{ChatRequest, GatewayConfig, HuakatiClient, Message};
use talentos_rag::HybridSearch;

use crate::dto::requests::AiSettingsUpdate;
use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::secrets::{ai_secret_path, AiSecret};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct RunAgentRequest {
    pub task: String,
    /// Allow the run to execute irreversible tools (payroll approval, termination).
    #[serde(default)]
    pub allow_destructive: bool,
}

#[derive(Debug, Serialize)]
pub struct RunAgentResponse {
    pub run_id: String,
    pub outcome: String,
    pub cycles: usize,
    pub approvals_requested: Vec<String>,
}

/// GET /api/v1/ai/tools
pub async fn list_tools() -> Response {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "tools": talentos_tools::tools::TOOL_NAMES })),
    )
        .into_response()
}

/// POST /api/v1/ai/agents/run
pub async fn run_agent(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<RunAgentRequest>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    let model_gw = match HuakatiClient::new() {
        Ok(c) => c,
        Err(e) => return unavailable(&format!("model gateway not configured: {e}")),
    };

    let mut built = talentos_tools::tools::default_registry(
        Some(Arc::new(model_gw.clone())),
        state.pool.clone(),
    );
    talentos_memory::tools::register_tools(&mut built, state.memory.clone());
    let registry = Arc::new(built);

    let agent_ctx = AgentContext {
        tenant_id: ctx.tenant_id,
        user_id: ctx.user_id,
        entity_id: ctx.entity_id,
        roles: ctx.roles.clone(),
        run_id: Uuid::new_v4(),
    };

    let runner = AgentRunner {
        context: agent_ctx,
        model_gw: model_gw.clone(),
        db: state.pool.clone(),
        hybrid_search: HybridSearch::new(model_gw, state.pool.pool().clone()),
        tool_registry: registry,
        max_cycles: 3,
    };

    // Human gate: this endpoint never auto-approves destructive tools unless the
    // caller explicitly opted in.
    let allow = body.allow_destructive;
    let report: RunReport = runner
        .run(&body.task, move |_tool| async move { allow })
        .await;

    let outcome = match &report.result {
        AgentResult::Success(_) => "success",
        AgentResult::Failed(_) => "failed",
        AgentResult::MaxCyclesExceeded => "max_cycles_exceeded",
        AgentResult::ApprovalRejected(_) => "approval_rejected",
    };

    (
        StatusCode::OK,
        Json(RunAgentResponse {
            run_id: runner.context.run_id.to_string(),
            outcome: outcome.to_string(),
            cycles: report.cycles.len(),
            approvals_requested: report.approvals_requested,
        }),
    )
        .into_response()
}

#[derive(Debug, Serialize)]
pub struct AiSettingsResponse {
    pub provider: String,
    pub base_url: Option<String>,
    pub model: String,
    pub mcp_enabled: bool,
    pub skills: serde_json::Value,
    pub api_key_set: bool,
    pub secrets_backend: &'static str,
}

/// GET /api/v1/ai/settings — never reads or returns the provider key.
pub async fn get_settings(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let row = sqlx::query(
        "SELECT provider, base_url, model, mcp_enabled, skills, api_key_set
           FROM ai_settings WHERE tenant_id = $1",
    )
    .bind(ctx.tenant_id.0)
    .fetch_optional(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    let backend = secrets_backend(&state);
    match row {
        Ok(Some(row)) => (
            StatusCode::OK,
            Json(AiSettingsResponse {
                provider: row.get("provider"),
                base_url: row.get("base_url"),
                model: row.get("model"),
                mcp_enabled: row.get("mcp_enabled"),
                skills: row.get("skills"),
                api_key_set: row.get("api_key_set"),
                secrets_backend: backend,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::OK,
            Json(AiSettingsResponse {
                provider: "huakati".into(),
                base_url: None,
                model: "qwen2.5-72b-instruct".into(),
                mcp_enabled: true,
                skills: serde_json::json!([]),
                api_key_set: false,
                secrets_backend: backend,
            }),
        )
            .into_response(),
        Err(e) => internal_err(&e.to_string()),
    }
}

/// PUT /api/v1/ai/settings — non-secrets in Postgres, key only in OpenBao.
pub async fn put_settings(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<AiSettingsUpdate>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }

    let key = body.api_key.as_deref().filter(|k| !k.trim().is_empty());
    if let Some(key) = key {
        // Fail closed before any write: a disabled backend never stores a key.
        let Some(client) = &state.secrets else {
            return unavailable("OpenBao is not configured");
        };
        let secret = AiSecret {
            provider: body.provider.clone(),
            base_url: body.base_url.clone(),
            model: body.model.clone(),
            api_key: key.to_string(),
        };
        if let Err(e) = client.write_ai_secret(ctx.tenant_id.0, &secret).await {
            return unavailable(&format!("secret backend write failed: {e}"));
        }
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let saved = sqlx::query(
        r#"INSERT INTO ai_settings
             (tenant_id, provider, base_url, model, mcp_enabled, skills, api_key_set, secret_ref)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
           ON CONFLICT (tenant_id) DO UPDATE SET
               provider = EXCLUDED.provider,
               base_url = EXCLUDED.base_url,
               model = EXCLUDED.model,
               mcp_enabled = EXCLUDED.mcp_enabled,
               skills = EXCLUDED.skills,
               api_key_set = CASE WHEN $7 THEN true ELSE ai_settings.api_key_set END,
               secret_ref = CASE WHEN $7 THEN EXCLUDED.secret_ref ELSE ai_settings.secret_ref END,
               updated_at = now()
           RETURNING api_key_set"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(&body.provider)
    .bind(&body.base_url)
    .bind(&body.model)
    .bind(body.mcp_enabled)
    .bind(&body.skills)
    .bind(key.is_some())
    .bind(key.map(|_| ai_secret_path(ctx.tenant_id.0)))
    .fetch_one(&mut *tx)
    .await;
    let api_key_set = match saved {
        Ok(row) => row.get::<bool, _>("api_key_set"),
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    (
        StatusCode::OK,
        Json(AiSettingsResponse {
            provider: body.provider,
            base_url: body.base_url,
            model: body.model,
            mcp_enabled: body.mcp_enabled,
            skills: body.skills,
            api_key_set,
            secrets_backend: secrets_backend(&state),
        }),
    )
        .into_response()
}

#[derive(Debug, Serialize)]
pub struct AiSettingsTestResponse {
    pub ok: bool,
    pub latency_ms: u64,
    pub model: String,
}

/// POST /api/v1/ai/settings/test — one cheap completion with the stored secret.
pub async fn test_settings(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    let Some(secrets) = &state.secrets else {
        return unavailable("OpenBao is not configured");
    };
    let secret = match secrets.read_ai_secret(ctx.tenant_id.0).await {
        Ok(Some(secret)) => secret,
        Ok(None) => return unavailable("AI provider is not configured"),
        Err(e) => return unavailable(&format!("secret backend read failed: {e}")),
    };

    let defaults = GatewayConfig::default();
    let config = GatewayConfig {
        base_url: secret.base_url.clone().unwrap_or(defaults.base_url.clone()),
        api_key: secret.api_key.clone(),
        primary_model: secret.model.clone(),
        ..defaults
    };
    let client = match HuakatiClient::with_config(config) {
        Ok(client) => client,
        Err(e) => return unavailable(&format!("model gateway not configured: {e}")),
    };

    let request = ChatRequest {
        model: secret.model.clone(),
        messages: vec![Message {
            role: "user".into(),
            content: "ping".into(),
            tool_calls: None,
        }],
        tools: None,
        stream: Some(false),
        temperature: Some(0.0),
        max_tokens: Some(8),
    };
    let started = Instant::now();
    match client
        .chat_with_request(&ctx.tenant_id.0.to_string(), request)
        .await
    {
        Ok(response) => (
            StatusCode::OK,
            Json(AiSettingsTestResponse {
                ok: true,
                latency_ms: started.elapsed().as_millis() as u64,
                model: response.model,
            }),
        )
            .into_response(),
        Err(e) => unavailable(&format!("model request failed: {e}")),
    }
}

#[derive(Debug, Serialize)]
pub struct KgCountsResponse {
    pub nodes: std::collections::BTreeMap<talentos_hr_runner::kg::NodeClass, i64>,
    pub edges: i64,
    /// Derived from `max(kg_nodes.updated_at)`: no extra column or migration.
    pub last_rebuilt_at: Option<DateTime<Utc>>,
}

/// GET /api/v1/ai/kg — class counts, edge count, last rebuild time.
pub async fn get_kg(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "ai", "chat") {
        return forbidden;
    }
    let nodes = match talentos_hr_runner::kg::counts(&state.pool, ctx.tenant_id).await {
        Ok(nodes) => nodes,
        Err(e) => return internal_err(&e.to_string()),
    };
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let stats = sqlx::query(
        "SELECT (SELECT count(*) FROM kg_edges WHERE tenant_id = $1) AS edges,
                (SELECT max(updated_at) FROM kg_nodes WHERE tenant_id = $1) AS last_rebuilt_at",
    )
    .bind(ctx.tenant_id.0)
    .fetch_one(&mut *tx)
    .await;
    let _ = tx.rollback().await;
    let stats = match stats {
        Ok(row) => row,
        Err(e) => return internal_err(&e.to_string()),
    };
    (
        StatusCode::OK,
        Json(KgCountsResponse {
            nodes,
            edges: stats.get("edges"),
            last_rebuilt_at: stats.get("last_rebuilt_at"),
        }),
    )
        .into_response()
}

/// POST /api/v1/ai/kg/rebuild — idempotent sync, capped by the service.
pub async fn rebuild_kg(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    match talentos_hr_runner::kg_sync::rebuild(&state.pool, ctx.tenant_id).await {
        Ok(report) => (StatusCode::OK, Json(report)).into_response(),
        Err(e) => internal_err(&e.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct KgNeighborsQuery {
    pub node_id: Uuid,
    pub depth: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct KgNeighborsResponse {
    pub nodes: Vec<talentos_hr_runner::kg::KgNode>,
    pub edges: Vec<talentos_hr_runner::kg::KgEdge>,
}

/// GET /api/v1/ai/kg/neighbors?node_id=&depth= — the graph inspector's data.
pub async fn kg_neighbors(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<KgNeighborsQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ai", "chat") {
        return forbidden;
    }
    // neighbors() clamps depth to 1..=5; 2 matches the agent's subgraph depth.
    let depth = query.depth.unwrap_or(2);
    match talentos_hr_runner::kg::neighbors(&state.pool, ctx.tenant_id, query.node_id, depth).await
    {
        Ok((nodes, edges)) => {
            (StatusCode::OK, Json(KgNeighborsResponse { nodes, edges })).into_response()
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

fn secrets_backend(state: &AppState) -> &'static str {
    if state.secrets.is_some() {
        "openbao"
    } else {
        "none"
    }
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "ai settings request failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "internal_error".into(),
            message: "Internal server error".into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn unavailable(message: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: "ai_unavailable".into(),
            message: message.to_string(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

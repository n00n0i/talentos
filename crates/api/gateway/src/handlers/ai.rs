//! AI surface: list available tools and run the agent loop on a task.
//!
//! The agent runs in-process; see `docs/BUILD_STATUS.md` for the Temporal swap.

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use talentos_agent::{AgentRunner, RunReport};
use talentos_kernel::shared::{AgentContext, AgentResult};
use talentos_model_gw::HuakatiClient;
use talentos_rag::HybridSearch;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::Authed;
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
    let model_gw = match HuakatiClient::new() {
        Ok(c) => c,
        Err(e) => return unavailable(&format!("model gateway not configured: {e}")),
    };

    let registry = Arc::new(talentos_tools::tools::default_registry(
        Some(Arc::new(model_gw.clone())),
        state.pool.clone(),
    ));

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

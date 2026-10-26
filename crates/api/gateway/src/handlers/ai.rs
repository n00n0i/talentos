//! AI API Handlers
//!
//! Endpoints for AI chat, agent runs, document management, and cost tracking.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc, NaiveDate};

use crate::handlers::{error_response, success_response};
use crate::middleware::auth::TenantContext;
use crate::state::AppState;

/// Build AI routes
pub fn ai_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/ai/chat", post(ai_chat_handler))
        .route("/api/v1/ai/agents/:id/run", post(start_agent_run))
        .route("/api/v1/ai/agents/:id/status", get(get_agent_status))
        .route("/api/v1/ai/agents/:id/approve", post(signal_approval))
        .route("/api/v1/ai/documents", post(upload_document))
        .route("/api/v1/ai/documents/search", post(search_documents))
        .route("/api/v1/ai/cost", get(get_cost_report))
}

/// POST /api/v1/ai/chat
async fn ai_chat_handler(
    State(_state): State<AppState>,
    tenant_ctx: TenantContext,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, Json<serde_json::Value>)> {
    // In real implementation, would call ModelGateway and Agent framework
    Ok(Json(ChatResponse {
        agent: req.agent,
        response: format!("AI response to: {}", req.message),
        run_id: Uuid::new_v4(),
        citations: vec![],
    }))
}

/// POST /api/v1/ai/agents/:id/run
async fn start_agent_run(
    State(_state): State<AppState>,
    tenant_ctx: TenantContext,
    Path(agent_id): Path<String>,
    Json(req): Json<StartAgentRunRequest>,
) -> Result<Json<StartAgentRunResponse>, (StatusCode, Json<serde_json::Value>)> {
    // In real implementation, would start Temporal workflow
    Ok(Json(StartAgentRunResponse {
        run_id: Uuid::new_v4(),
        agent_id,
        status: "running".to_string(),
        created_at: Utc::now(),
    }))
}

/// GET /api/v1/ai/agents/:id/status
async fn get_agent_status(
    State(_state): State<AppState>,
    tenant_ctx: TenantContext,
    Path(run_id): Path<Uuid>,
) -> Result<Json<AgentStatusResponse>, (StatusCode, Json<serde_json::Value>)> {
    // In real implementation, would query Temporal workflow state
    Ok(Json(AgentStatusResponse {
        run_id,
        agent_type: "PolicyQueryAgent".to_string(),
        state: "running".to_string(),
        current_phase: "OBSERVE".to_string(),
        plan: None,
        history: vec![],
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }))
}

/// POST /api/v1/ai/agents/:id/approve
async fn signal_approval(
    State(_state): State<AppState>,
    tenant_ctx: TenantContext,
    Path(run_id): Path<Uuid>,
    Json(req): Json<ApprovalSignalRequest>,
) -> Result<Json<ApprovalSignalResponse>, (StatusCode, Json<serde_json::Value>)> {
    // In real implementation, would send signal to Temporal workflow
    Ok(Json(ApprovalSignalResponse {
        run_id,
        decision: req.decision,
        processed_at: Utc::now(),
    }))
}

/// POST /api/v1/ai/documents
async fn upload_document(
    State(_state): State<AppState>,
    tenant_ctx: TenantContext,
    Json(req): Json<UploadDocumentRequest>,
) -> Result<Json<UploadDocumentResponse>, (StatusCode, Json<serde_json::Value>)> {
    // In real implementation, would store document and trigger RAG indexing
    Ok(Json(UploadDocumentResponse {
        document_id: Uuid::new_v4(),
        status: "indexing".to_string(),
        created_at: Utc::now(),
    }))
}

/// POST /api/v1/ai/documents/search
async fn search_documents(
    State(_state): State<AppState>,
    tenant_ctx: TenantContext,
    Json(req): Json<SearchDocumentsRequest>,
) -> Result<Json<SearchDocumentsResponse>, (StatusCode, Json<serde_json::Value>)> {
    // In real implementation, would call RAG hybrid search
    Ok(Json(SearchDocumentsResponse {
        query: req.query,
        results: vec![],
        total_hits: 0,
    }))
}

/// GET /api/v1/ai/cost
async fn get_cost_report(
    State(_state): State<AppState>,
    tenant_ctx: TenantContext,
    Query(params): Query<CostReportQuery>,
) -> Result<Json<CostReportResponse>, (StatusCode, Json<serde_json::Value>)> {
    // In real implementation, would query ai_cost_tracking table
    Ok(Json(CostReportResponse {
        tenant_id: tenant_ctx.tenant_id.0,
        period_start: params.from.unwrap_or_else(|| Utc::now().date_naive()),
        period_end: params.to.unwrap_or_else(|| Utc::now().date_naive()),
        by_model: vec![],
        total_cost_usd: 0.0,
        total_tokens: 0,
    }))
}

// --- Request/Response Types ---

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub agent: Option<String>,
    pub message: String,
    pub context: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub agent: Option<String>,
    pub response: String,
    pub run_id: Uuid,
    pub citations: Vec<Citation>,
}

#[derive(Debug, Serialize)]
pub struct Citation {
    pub chunk_text: String,
    pub source_doc: String,
    pub similarity: f32,
}

#[derive(Debug, Deserialize)]
pub struct StartAgentRunRequest {
    pub task: String,
    pub agent: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct StartAgentRunResponse {
    pub run_id: Uuid,
    pub agent_id: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AgentStatusResponse {
    pub run_id: Uuid,
    pub agent_type: String,
    pub state: String,
    pub current_phase: String,
    pub plan: Option<serde_json::Value>,
    pub history: Vec<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ApprovalSignalRequest {
    pub decision: String, // "approve" | "reject"
    pub comment: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApprovalSignalResponse {
    pub run_id: Uuid,
    pub decision: String,
    pub processed_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct UploadDocumentRequest {
    pub doc_type: String,
    pub source_path: String,
    pub content: Option<String>, // For direct text upload
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct UploadDocumentResponse {
    pub document_id: Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct SearchDocumentsRequest {
    pub query: String,
    pub filters: Option<SearchFilters>,
    pub top_k: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct SearchFilters {
    pub doc_type: Option<String>,
    pub entity_id: Option<Uuid>,
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
}

#[derive(Debug, Serialize)]
pub struct SearchDocumentsResponse {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub total_hits: usize,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub chunk_text: String,
    pub source_doc: String,
    pub similarity: f32,
    pub chunk_index: usize,
}

#[derive(Debug, Deserialize)]
pub struct CostReportQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CostReportResponse {
    pub tenant_id: Uuid,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub by_model: Vec<ModelCost>,
    pub total_cost_usd: f64,
    pub total_tokens: u64,
}

#[derive(Debug, Serialize)]
pub struct ModelCost {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub cost_usd: f64,
    pub date: NaiveDate,
}
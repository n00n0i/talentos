//! Shared workflow types - single source of truth for agent workflow types

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Agent workflow state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    pub phase: WorkflowPhase,
    pub current_step: usize,
    pub observations: Vec<Observation>,
    pub plans: Vec<ActionPlan>,
    pub results: Vec<ActResult>,
    pub reflections: Vec<Reflection>,
    pub cycle_count: usize,
    pub history: Vec<StepSnapshot>,
    pub pending_approval: Option<PendingApproval>,
}

impl WorkflowState {
    pub fn new() -> Self {
        Self {
            phase: WorkflowPhase::Observe,
            current_step: 0,
            observations: Vec::new(),
            plans: Vec::new(),
            results: Vec::new(),
            reflections: Vec::new(),
            cycle_count: 0,
            history: Vec::new(),
            pending_approval: None,
        }
    }

    pub fn record_step(&mut self, phase: &str, data: impl Serialize) {
        self.history.push(StepSnapshot {
            phase: match phase {
                "OBSERVE" => WorkflowPhase::Observe,
                "PLAN" => WorkflowPhase::Plan,
                "ACT" => WorkflowPhase::Act,
                "REFLECT" => WorkflowPhase::Reflect,
                "REPLAN" => WorkflowPhase::Replan,
                _ => WorkflowPhase::Observe,
            },
            timestamp: Utc::now(),
            data: serde_json::to_value(data).unwrap_or_default(),
        });
    }

    pub fn handle_approval(&mut self, signal: HumanApprovalSignal) {
        if let Some(pending) = &mut self.pending_approval {
            if pending.tool_name == signal.tool_name {
                pending.approved = signal.decision == ApprovalDecision::Approve;
                pending.approver_id = Some(signal.approver_user_id);
                pending.comment = signal.comment;
                pending.approved_at = Some(signal.timestamp);
            }
        }
    }

    pub fn handle_rejection(&mut self, signal: HumanRejectionSignal) {
        if let Some(pending) = &mut self.pending_approval {
            if pending.tool_name == signal.tool_name {
                pending.approved = false;
                pending.approver_id = Some(signal.approver_user_id);
                pending.comment = Some(signal.comment);
                pending.approved_at = Some(signal.timestamp);
            }
        }
    }

    pub fn add_context(&mut self, context: serde_json::Value) {
        if let Some(last_obs) = self.observations.last_mut() {
            if let Some(emp) = last_obs.employee_data.as_mut() {
                *emp = context;
            }
        }
    }
}

/// Workflow phase
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkflowPhase {
    Observe,
    Plan,
    Act,
    Reflect,
    Replan,
    Done,
    Failed,
}

/// Observation from OBSERVE phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub summary: String,
    pub relevant_chunks: Vec<Citation>,
    pub employee_data: Option<serde_json::Value>,
    pub payroll_data: Option<serde_json::Value>,
    pub leave_data: Option<serde_json::Value>,
}

/// Citation for grounding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub chunk_text: String,
    pub source_doc: DocReference,
    pub similarity: f32,
    pub chunk_index: usize,
}

/// Document reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocReference {
    pub tenant_id: Uuid,
    pub doc_id: Uuid,
    pub source_path: String,
}

/// Action plan from PLAN phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPlan {
    pub steps: Vec<PlanStep>,
    pub expected_outcome: String,
    pub requires_approval: bool,
    pub approval_tool: Option<String>,
}

/// Single step in the action plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub tool_name: String,
    pub parameters: serde_json::Value,
    pub description: String,
    pub is_destructive: bool,
}

/// Result from ACT phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActResult {
    pub tool_results: Vec<ToolResult>,
    pub is_complete: bool,
    pub error: Option<String>,
}

/// Single tool execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub success: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Reflection from REFLECT phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reflection {
    pub score: f32, // 0.0 to 1.0
    pub assessment: String,
    pub missing_info: Vec<String>,
    pub should_replan: bool,
}

/// Step snapshot for history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSnapshot {
    pub phase: WorkflowPhase,
    pub timestamp: DateTime<Utc>,
    pub data: serde_json::Value,
}

/// Pending approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    pub tool_name: String,
    pub approved: bool,
    pub approver_id: Option<Uuid>,
    pub comment: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
}

/// Agent context passed to activities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub entity_id: Option<Uuid>,
    pub roles: Vec<String>,
    pub run_id: Uuid,
}

/// Input for starting an agent workflow
#[derive(Clone, Serialize, Deserialize)]
pub struct AgentInput {
    pub agent_type: String,
    pub task: String,
    pub context: AgentContext,
}

/// Agent result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentResult {
    Success(ActResult),
    Failed(String),
    MaxCyclesExceeded,
    ApprovalRejected(String),
}

/// Agent error
#[derive(thiserror::Error, Debug)]
pub enum AgentError {
    #[error("Max cycles exceeded")]
    MaxCyclesExceeded,
    #[error("Approval rejected: {0}")]
    ApprovalRejected(String),
    #[error("Tool execution failed: {0}")]
    ToolFailed(String),
    #[error("Model error: {0}")]
    ModelError(#[from] crate::model_gw::error::ModelError),
    #[error("RAG error: {0}")]
    RagError(#[from] crate::rag::error::RAGError),
    #[error("Temporal error: {0}")]
    TemporalError(String),
}

/// Result from ACT phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActResult {
    pub tool_results: Vec<ToolResult>,
    pub is_complete: bool,
    pub error: Option<String>,
}

/// Single tool execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub success: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Reflection from REFLECT phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reflection {
    pub score: f32, // 0.0 to 1.0
    pub assessment: String,
    pub missing_info: Vec<String>,
    pub should_replan: bool,
}

/// Step snapshot for history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSnapshot {
    pub phase: WorkflowPhase,
    pub timestamp: DateTime<Utc>,
    pub data: serde_json::Value,
}

/// Pending approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    pub tool_name: String,
    pub approved: bool,
    pub approver_id: Option<Uuid>,
    pub comment: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
}

/// Human approval signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanApprovalSignal {
    pub run_id: Uuid,
    pub approver_user_id: Uuid,
    pub tool_name: String,
    pub decision: ApprovalDecision,
    pub comment: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Approval decision
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApprovalDecision {
    Approve,
    Reject,
}

/// Human rejection signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanRejectionSignal {
    pub tool_name: String,
    pub approver_user_id: Uuid,
    pub comment: String,
    pub timestamp: DateTime<Utc>,
}

/// Additional context signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdditionalContextSignal {
    pub context: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

/// Single step in the action plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub tool_name: String,
    pub parameters: serde_json::Value,
    pub description: String,
    pub is_destructive: bool,
}

/// Action plan from PLAN phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPlan {
    pub steps: Vec<PlanStep>,
    pub expected_outcome: String,
    pub requires_approval: bool,
    pub approval_tool: Option<String>,
}

/// Single step in the action plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub tool_name: String,
    pub parameters: serde_json::Value,
    pub description: String,
    pub is_destructive: bool,
}

/// Single tool execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub success: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Reflection from REFLECT phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reflection {
    pub score: f32, // 0.0 to 1.0
    pub assessment: String,
    pub missing_info: Vec<String>,
    pub should_replan: bool,
}

/// Step snapshot for history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSnapshot {
    pub phase: WorkflowPhase,
    pub timestamp: DateTime<Utc>,
    pub data: serde_json::Value,
}

/// Pending approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    pub tool_name: String,
    pub approved: bool,
    pub approver_id: Option<Uuid>,
    pub comment: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
}

/// Human approval signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanApprovalSignal {
    pub run_id: Uuid,
    pub approver_user_id: Uuid,
    pub tool_name: String,
    pub decision: ApprovalDecision,
    pub comment: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Approval decision
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApprovalDecision {
    Approve,
    Reject,
}

/// Human rejection signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanRejectionSignal {
    pub tool_name: String,
    pub approver_user_id: Uuid,
    pub comment: String,
    pub timestamp: DateTime<Utc>,
}

/// Additional context signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdditionalContextSignal {
    pub context: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

/// Agent result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentResult {
    Success(ActResult),
    Failed(String),
    MaxCyclesExceeded,
    ApprovalRejected(String),
}

/// Agent error
#[derive(thiserror::Error, Debug)]
pub enum AgentError {
    #[error("Max cycles exceeded")]
    MaxCyclesExceeded,
    #[error("Approval rejected: {0}")]
    ApprovalRejected(String),
    #[error("Tool execution failed: {0}")]
    ToolFailed(String),
    #[error("Model error: {0}")]
    ModelError(#[from] crate::model_gw::error::ModelError),
    #[error("RAG error: {0}")]
    RagError(#[from] crate::rag::error::RAGError),
    #[error("Temporal error: {0}")]
    TemporalError(String),
}

/// Human rejection signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanRejectionSignal {
    pub tool_name: String,
    pub approver_user_id: Uuid,
    pub comment: String,
    pub timestamp: DateTime<Utc>,
}

/// Additional context signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdditionalContextSignal {
    pub context: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

/// Agent context passed to activities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub entity_id: Option<Uuid>,
    pub roles: Vec<String>,
    pub run_id: Uuid,
}

/// Input for starting an agent workflow
#[derive(Clone, Serialize, Deserialize)]
pub struct AgentInput {
    pub agent_type: String,
    pub task: String,
    pub context: AgentContext,
}

/// Result from ACT phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActResult {
    pub tool_results: Vec<ToolResult>,
    pub is_complete: bool,
    pub error: Option<String>,
}

/// Single tool execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub success: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Reflection from REFLECT phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reflection {
    pub score: f32, // 0.0 to 1.0
    pub assessment: String,
    pub missing_info: Vec<String>,
    pub should_replan: bool,
}

/// Step snapshot for history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSnapshot {
    pub phase: WorkflowPhase,
    pub timestamp: DateTime<Utc>,
    pub data: serde_json::Value,
}

/// Pending approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    pub tool_name: String,
    pub approved: bool,
    pub approver_id: Option<Uuid>,
    pub comment: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
}

/// Human approval signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanApprovalSignal {
    pub run_id: Uuid,
    pub approver_user_id: Uuid,
    pub tool_name: String,
    pub decision: ApprovalDecision,
    pub comment: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Approval decision
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApprovalDecision {
    Approve,
    Reject,
}

/// Human rejection signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanRejectionSignal {
    pub tool_name: String,
    pub approver_user_id: Uuid,
    pub comment: String,
    pub timestamp: DateTime<Utc>,
}

/// Additional context signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdditionalContextSignal {
    pub context: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}
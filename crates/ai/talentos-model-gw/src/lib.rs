//! Model Gateway - huakati.online LLM client with fallback, rate limiting, circuit breaker, and cost tracking.

pub mod client;
pub mod cost;
pub mod error;
pub mod models;

pub use client::{HuakatiClient, ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse, Message, ToolDef};
pub use cost::TokenUsage;
pub use error::ModelError;

// Re-export shared workflow types from talentos-kernel
pub use talentos_kernel::shared::{
    AgentContext,
    Observation,
    Citation,
    DocReference,
    ActionPlan,
    PlanStep,
    ActResult,
    ToolResult,
    Reflection,
    WorkflowState,
    WorkflowPhase,
    StepSnapshot,
    HumanApprovalSignal,
    ApprovalDecision,
    AdditionalContextSignal,
    AgentResult,
    AgentError,
    WorkflowPhase as ModelWorkflowPhase,
    AgentContext,
    AgentInput,
    AgentResult,
    AgentError,
    WorkflowState,
    WorkflowPhase,
    StepSnapshot,
    HumanApprovalSignal,
    ApprovalDecision,
    AdditionalContextSignal,
    AgentResult,
    AgentError,
};
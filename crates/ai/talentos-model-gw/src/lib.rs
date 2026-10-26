//! Model Gateway - huakati.online LLM client with fallback, rate limiting,
//! circuit breaker, and cost tracking.

pub mod client;
pub mod cost;
pub mod error;
pub mod models;

pub use client::{GatewayConfig, HuakatiClient};
pub use cost::TokenUsage;
pub use error::ModelError;
pub use models::*;

// Re-export shared workflow types from talentos-kernel
pub use talentos_kernel::shared::{
    ActResult, ActionPlan, AdditionalContextSignal, AgentContext, AgentError, AgentInput,
    AgentResult, ApprovalDecision, Citation, DocReference, HumanApprovalSignal,
    HumanRejectionSignal, Observation, PendingApproval, PlanStep, Reflection, StepSnapshot,
    ToolResult, WorkflowPhase, WorkflowState,
};

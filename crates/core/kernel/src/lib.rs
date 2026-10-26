//! TalentOS Kernel — shared primitives used by every other crate.
//!
//! This crate has ZERO dependencies on web frameworks, databases, or async runtimes.
//! It defines the contract that all layers implement against.

pub mod tenant;
pub mod events;
pub mod errors;
pub mod money;
pub mod shared;

pub use tenant::{TenantId, EntityId, UserId, TenantContext};
pub use events::{DomainEvent, EventType};
pub use errors::KernelError;
pub use money::Money;
pub use shared::{
    // Workflow types
    WorkflowState, WorkflowPhase, Observation, ActionPlan, PlanStep,
    ActResult, ToolResult, Reflection, StepSnapshot,
    PendingApproval, HumanApprovalSignal, ApprovalDecision,
    HumanRejectionSignal, AdditionalContextSignal,
    AgentResult, AgentError, AgentContext, AgentInput,
    ActResult, ToolResult, Reflection, StepSnapshot,
    PendingApproval, HumanApprovalSignal, ApprovalDecision,
    HumanRejectionSignal, AdditionalContextSignal,
    AgentResult, AgentError, AgentContext, AgentInput,
    WorkflowState, WorkflowPhase, WorkflowPhase as KernelWorkflowPhase,
    // Re-exports
    AgentResult as KernelAgentResult,
    AgentError as KernelAgentError,
    AgentContext as KernelAgentContext,
    AgentInput as KernelAgentInput,
};
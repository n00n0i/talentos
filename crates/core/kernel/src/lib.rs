//! TalentOS Kernel — shared primitives used by every other crate.
//!
//! This crate has ZERO dependencies on web frameworks, databases, or async runtimes.
//! It defines the contract that all layers implement against.

pub mod errors;
pub mod events;
pub mod money;
pub mod shared;
pub mod tenant;

pub use errors::KernelError;
pub use events::{DomainEvent, EventType};
pub use money::Money;
pub use tenant::{EntityId, TenantContext, TenantId, UserId};

pub use shared::{
    ActResult, ActionPlan, AdditionalContextSignal, AgentContext, AgentError, AgentInput,
    AgentResult, ApprovalDecision, Citation, DocReference, HumanApprovalSignal,
    HumanRejectionSignal, Observation, PendingApproval, PlanStep, Reflection, StepSnapshot,
    ToolResult, WorkflowPhase, WorkflowState,
};

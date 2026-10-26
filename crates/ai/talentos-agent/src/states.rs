//! Agent state types - re-exported from talentos-kernel so there is one definition.

pub use talentos_kernel::shared::{
    ActResult, ActionPlan, AdditionalContextSignal, AgentContext, AgentError, AgentInput,
    AgentResult, ApprovalDecision, Citation, DocReference, HumanApprovalSignal,
    HumanRejectionSignal, Observation, PendingApproval, PlanStep, Reflection, StepSnapshot,
    ToolResult, WorkflowPhase, WorkflowState,
};

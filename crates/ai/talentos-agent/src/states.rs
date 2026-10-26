//! Agent workflow state types - re-exported from talentos-kernel

pub use talentos_kernel::shared::{
    WorkflowState, WorkflowPhase, Observation, ActionPlan, PlanStep,
    ActResult, ToolResult, Reflection, StepSnapshot,
    PendingApproval, HumanApprovalSignal, ApprovalDecision,
    HumanRejectionSignal, AdditionalContextSignal,
    AgentResult, AgentError, AgentContext, AgentInput,
    ActResult, ToolResult, Reflection, StepSnapshot,
    PendingApproval, HumanApprovalSignal, ApprovalDecision,
    HumanRejectionSignal, AdditionalContextSignal,
    AgentResult, AgentError, AgentContext, AgentInput,
    WorkflowState, WorkflowPhase, WorkflowPhase as AgentWorkflowPhase,
    Observation, Citation, DocReference,
    ActionPlan, PlanStep, ActResult, ToolResult, Reflection,
    StepSnapshot, PendingApproval,
    HumanApprovalSignal, ApprovalDecision,
    HumanRejectionSignal, AdditionalContextSignal,
    AgentResult, AgentError, AgentContext, AgentInput,
    // Re-export primitives
    WorkflowPhase as AgentWorkflowPhase,
};

use crate::states::WorkflowState as _; // Force re-export

/// Workflow phase (re-export)
pub type WorkflowPhase = talentos_kernel::shared::WorkflowPhase;

/// Observation from OBSERVE phase
pub type Observation = talentos_kernel::shared::Observation;

/// Citation for grounding
pub type Citation = talentos_kernel::shared::Citation;

/// Document reference
pub type DocReference = talentos_kernel::shared::DocReference;

/// Action plan from PLAN phase
pub type ActionPlan = talentos_kernel::shared::ActionPlan;

/// Single step in the action plan
pub type PlanStep = talentos_kernel::shared::PlanStep;

/// Result from ACT phase
pub type ActResult = talentos_kernel::shared::ActResult;

/// Single tool execution result
pub type ToolResult = talentos_kernel::shared::ToolResult;

/// Reflection from REFLECT phase
pub type Reflection = talentos_kernel::shared::Reflection;

/// Step snapshot for history
pub type StepSnapshot = talentos_kernel::shared::StepSnapshot;

/// Pending approval
pub type PendingApproval = talentos_kernel::shared::PendingApproval;

/// Human approval signal
pub type HumanApprovalSignal = talentos_kernel::shared::HumanApprovalSignal;

/// Approval decision
pub type ApprovalDecision = talentos_kernel::shared::ApprovalDecision;

/// Human rejection signal
pub type HumanRejectionSignal = talentos_kernel::shared::HumanRejectionSignal;

/// Additional context signal
pub type AdditionalContextSignal = talentos_kernel::shared::AdditionalContextSignal;

/// Agent result
pub type AgentResult = talentos_kernel::shared::AgentResult;

/// Agent error
pub type AgentError = talentos_kernel::shared::AgentError;

/// Agent context passed to activities
pub type AgentContext = talentos_kernel::shared::AgentContext;

/// Input for starting an agent workflow
pub type AgentInput = talentos_kernel::shared::AgentInput;

/// Workflow state
pub type WorkflowState = talentos_kernel::shared::WorkflowState;

/// Workflow phase
pub type WorkflowPhase = talentos_kernel::shared::WorkflowPhase;

/// Workflow phase (alias)
pub type AgentWorkflowPhase = talentos_kernel::shared::WorkflowPhase;

/// Observation from OBSERVE phase
pub type Observation = talentos_kernel::shared::Observation;

/// Citation for grounding
pub type Citation = talentos_kernel::shared::Citation;

/// Document reference
pub type DocReference = talentos_kernel::shared::DocReference;

/// Action plan from PLAN phase
pub type ActionPlan = talentos_kernel::shared::ActionPlan;

/// Single step in the action plan
pub type PlanStep = talentos_kernel::shared::PlanStep;

/// Result from ACT phase
pub type ActResult = talentos_kernel::shared::ActResult;

/// Single tool execution result
pub type ToolResult = talentos_kernel::shared::ToolResult;

/// Reflection from REFLECT phase
pub type Reflection = talentos_kernel::shared::Reflection;

/// Step snapshot for history
pub type StepSnapshot = talentos_kernel::shared::StepSnapshot

/// Pending approval
pub type PendingApproval = talentos_kernel::shared::PendingApproval

/// Human approval signal
pub type HumanApprovalSignal = talentos_kernel::shared::HumanApprovalSignal

/// Approval decision
pub type ApprovalDecision = talentos_kernel::shared::ApprovalDecision

/// Human rejection signal
pub type HumanRejectionSignal = talentos_kernel::shared::HumanRejectionSignal

/// Additional context signal
pub type AdditionalContextSignal = talentos_kernel::shared::AdditionalContextSignal

/// Agent result
pub type AgentResult = talentos_kernel::shared::AgentResult

/// Agent error
pub type AgentError = talentos_kernel::shared::AgentError

/// Agent context passed to activities
pub type AgentContext = talentos_kernel::shared::AgentContext

/// Input for starting an agent workflow
pub type AgentInput = talentos_kernel::shared::AgentInput
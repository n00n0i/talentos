//! Agent Framework - Temporal workflow for Observe→Plan→Act→Reflect agent loops.

pub mod workflow;
pub mod activities;
pub mod states;
pub mod error;

pub use workflow::AgentWorkflow;
pub use activities::{ObserveActivity, PlanActivity, ActActivity, ReflectActivity};
pub use states::{WorkflowState, WorkflowPhase, Observation, ActionPlan, ActResult, Reflection, AgentResult};
pub use error::AgentError;
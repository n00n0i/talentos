//! Agent Framework - Observe → Plan → Act → Reflect loop for HR/payroll tasks.

pub mod activities;
pub mod error;
pub mod states;
pub mod workflow;

pub use activities::{ActActivity, ObserveActivity, PlanActivity, ReflectActivity};
pub use error::AgentError;
pub use states::*;
pub use workflow::{AgentInput, AgentRunner, CycleTrace, RunReport};

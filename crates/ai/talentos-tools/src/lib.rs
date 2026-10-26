//! Built-in Tools - MCP tool wrappers with agent-specific validation and logic.

pub mod registry;
pub mod tools;

pub use registry::{AgentTool, ToolContext, ToolRegistry};
pub use tools::*;

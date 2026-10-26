use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use talentos_db::TenantPool;
use talentos_kernel::{EntityId, TenantId, UserId};
use talentos_model_gw::HuakatiClient;

/// Tool execution context
#[derive(Clone)]
pub struct ToolContext {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub entity_id: Option<EntityId>,
    pub run_id: uuid::Uuid,
    pub db: Arc<TenantPool>,
    /// `None` when HUAKATI_API_KEY is unset: local tools keep working, LLM-backed
    /// tools are simply not registered.
    pub model_gw: Option<Arc<HuakatiClient>>,
    /// Free-form per-request data (MCP extensions, trace ids, ...).
    pub extensions: HashMap<String, Value>,
}

/// Trait for agent tools
#[async_trait]
pub trait AgentTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;
    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult;
    fn is_destructive(&self) -> bool;
}

/// Tool execution result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: Value,
    pub error: Option<String>,
}

/// Definition of a tool as advertised to an LLM.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Tool registry
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn AgentTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register<T: AgentTool + 'static>(&mut self, tool: T) {
        let name = tool.name().to_string();
        self.tools.insert(name, Box::new(tool));
    }

    pub fn get(&self, name: &str) -> Option<&dyn AgentTool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn list_tools(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Execute a tool by name. Returns `Err` only when the tool is unknown;
    /// tool-level failures come back as `ToolResult { success: false, .. }`.
    pub async fn execute(
        &self,
        name: &str,
        input: Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, String> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| format!("Tool '{}' not found", name))?;
        Ok(tool.execute(input, ctx).await)
    }

    /// Names of every registered tool, in registration order.
    pub fn get_tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tools.keys().cloned().collect();
        names.sort();
        names
    }

    /// Tool definitions for LLM function calling.
    pub fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut defs: Vec<ToolDefinition> = self
            .tools
            .values()
            .map(|t| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.input_schema(),
            })
            .collect();
        defs.sort_by(|a, b| a.name.cmp(&b.name));
        defs
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

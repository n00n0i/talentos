use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rmcp::model::Extensions;
use serde_json::Value;
use talents_kernel::{TenantContext, TenantId, UserId, EntityId};
use talents_db::TenantPool;
use talents_model_gw::HuakatiClient;

/// Tool execution context
#[derive(Clone)]
pub struct ToolContext {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub entity_id: Option<EntityId>,
    pub run_id: uuid::Uuid,
    pub db: Arc<TenantPool>,
    pub model_gw: Arc<HuakatiClient>,
    pub extensions: Extensions,
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

    pub fn execute(&self, name: &str, input: Value, ctx: &ToolContext) -> Result<ToolResult, String> {
        self.tools
            .get(name)
            .ok_or_else(|| format!("Tool '{}' not found", name))?
            .execute(input, ctx)
            .await
            .map_err(|e| e.to_string())
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
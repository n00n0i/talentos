//! Memory tools for the agent loop and MCP.

use std::sync::Arc;

use serde_json::{json, Value};
use talentos_tools::registry::ToolResult;
use talentos_tools::{AgentTool, ToolContext, ToolRegistry};

use crate::{Layer, MemoryStore, NewMemory};

/// Register both memory tools. Call this wherever `default_registry` is built so
/// the in-process agent and MCP clients see the same surface.
pub fn register_tools(registry: &mut ToolRegistry, store: Arc<MemoryStore>) {
    registry.register(RecallMemoryTool::new(store.clone()));
    registry.register(RememberMemoryTool::new(store));
}

struct RecallMemoryTool {
    store: Arc<MemoryStore>,
}

impl RecallMemoryTool {
    fn new(store: Arc<MemoryStore>) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl AgentTool for RecallMemoryTool {
    fn name(&self) -> &str {
        "recall_memory"
    }

    fn description(&self) -> &str {
        "Recall relevant memories for the current tenant: past runs and decisions, \
durable facts, procedures, and current task context. Call this before planning."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "What you are trying to remember" },
                "layers": {
                    "type": "array",
                    "items": { "type": "string", "enum": ["working", "episodic", "semantic", "procedural"] },
                    "description": "Defaults to all four layers"
                },
                "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 8 }
            },
            "required": ["query"]
        })
    }

    fn is_destructive(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let query = input.get("query").and_then(Value::as_str).unwrap_or("");
        if query.trim().is_empty() {
            return fail("query is required");
        }
        let layers: Vec<Layer> = input
            .get("layers")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .filter_map(Layer::parse)
                    .collect()
            })
            .filter(|v: &Vec<Layer>| !v.is_empty())
            .unwrap_or_else(|| Layer::ALL.to_vec());
        let limit = input
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(8)
            .clamp(1, 50) as u32;

        match self
            .store
            .recall(ctx.tenant_id, query, &layers, limit)
            .await
        {
            Ok(hits) => ToolResult {
                success: true,
                output: crate::hits_json(&hits),
                error: None,
            },
            Err(e) => fail(&format!("recall failed: {e}")),
        }
    }
}

struct RememberMemoryTool {
    store: Arc<MemoryStore>,
}

impl RememberMemoryTool {
    fn new(store: Arc<MemoryStore>) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl AgentTool for RememberMemoryTool {
    fn name(&self) -> &str {
        "remember_memory"
    }

    fn description(&self) -> &str {
        "Store one durable or task-scoped memory. Use `episodic` for what happened, \
`semantic` for durable tenant facts, `procedural` for how-to, `working` for the \
current task (expires in an hour)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "content": { "type": "string", "maxLength": 8000 },
                "layer": {
                    "type": "string",
                    "enum": ["working", "episodic", "semantic", "procedural"],
                    "default": "episodic"
                },
                "importance": { "type": "number", "minimum": 0, "maximum": 1, "default": 0.5 },
                "scope_key": { "type": "string", "description": "Task/conversation id for working memory" },
                "ttl_seconds": { "type": "integer", "minimum": 60, "maximum": 2592000 },
                "metadata": { "type": "object" }
            },
            "required": ["content"]
        })
    }

    fn is_destructive(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let content = input.get("content").and_then(Value::as_str).unwrap_or("");
        let layer = input
            .get("layer")
            .and_then(Value::as_str)
            .and_then(Layer::parse)
            .unwrap_or(Layer::Episodic);
        let importance = input
            .get("importance")
            .and_then(Value::as_f64)
            .unwrap_or(0.5) as f32;
        let scope_key = input.get("scope_key").and_then(Value::as_str).unwrap_or("");
        let metadata = input.get("metadata").cloned().unwrap_or_else(|| json!({}));
        let ttl = input.get("ttl_seconds").and_then(Value::as_i64);

        let mut memory = NewMemory::new(layer, content);
        memory.importance = importance;
        memory.scope_key = scope_key;
        memory.metadata = metadata;
        memory.ttl_seconds = ttl;

        match self
            .store
            .remember(ctx.tenant_id, Some(ctx.user_id), memory)
            .await
        {
            Ok(id) => ToolResult {
                success: true,
                output: json!({ "id": id, "layer": layer.as_str() }),
                error: None,
            },
            Err(e) => fail(&format!("remember failed: {e}")),
        }
    }
}

fn fail(message: &str) -> ToolResult {
    ToolResult {
        success: false,
        output: Value::Null,
        error: Some(message.to_string()),
    }
}

//! Model Context Protocol over streamable HTTP.
//!
//! Stateless by design: each request carries its own JWT, so the tenant comes
//! from the token and never from a session. That removes the whole session
//! table — and the risk of one client resuming another client's tenant.
//!
//! Only the `tools` capability is advertised. The tools are the same
//! `ToolRegistry` the in-process agent loop uses, so an MCP client and the
//! built-in agent cannot drift apart.

use serde_json::{json, Value};
use talentos_tools::{ToolContext, ToolRegistry};

pub const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// JSON-RPC notifications carry no `id` and must not be answered.
pub fn is_notification(message: &Value) -> bool {
    message.get("id").is_none()
}

/// Dispatch one JSON-RPC message.
///
/// Returns `None` for notifications (JSON-RPC messages without an `id`), which
/// must not receive a response.
pub async fn handle(
    message: &Value,
    registry: &ToolRegistry,
    ctx: &ToolContext,
    allow_destructive: bool,
) -> Option<Value> {
    if is_notification(message) {
        return None;
    }
    let id = message.get("id").cloned()?;
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    let result = match method {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list(registry)),
        "tools/call" => call_tool(registry, ctx, allow_destructive, &params).await,
        other => Err((-32601, format!("method not found: {other}"))),
    };

    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message }
        }),
    })
}

pub fn initialize(params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let version = if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
        requested
    } else {
        LATEST_PROTOCOL_VERSION
    };

    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "talentos", "version": env!("CARGO_PKG_VERSION") },
        "instructions": "TalentOS HR and payroll. Every call runs inside the caller's tenant. \
    Destructive tools (payroll runs) additionally require the X-TalentOS-Allow-Destructive: true header."
    })
}

pub fn tools_list(registry: &ToolRegistry) -> Value {
    let tools: Vec<Value> = registry
        .get_tool_definitions()
        .into_iter()
        .map(|def| {
            let destructive = registry
                .get(&def.name)
                .map(|t| t.is_destructive())
                .unwrap_or(false);
            json!({
                "name": def.name,
                "description": def.description,
                "inputSchema": def.parameters,
                "annotations": {
                    "readOnlyHint": !destructive,
                    "destructiveHint": destructive,
                    "idempotentHint": !destructive,
                }
            })
        })
        .collect();

    json!({ "tools": tools })
}

/// Refuse unknown tools and destructive tools unless the caller opted in.
/// Split out so the security decision is testable without a database.
pub fn guard(
    registry: &ToolRegistry,
    name: &str,
    allow_destructive: bool,
) -> Result<(), (i64, String)> {
    let Some(tool) = registry.get(name) else {
        return Err((-32602, format!("unknown tool: {name}")));
    };
    if tool.is_destructive() && !allow_destructive {
        return Err((
            -32602,
            format!(
            "tool '{name}' is destructive; resend with header X-TalentOS-Allow-Destructive: true"
        ),
        ));
    }
    Ok(())
}

async fn call_tool(
    registry: &ToolRegistry,
    ctx: &ToolContext,
    allow_destructive: bool,
    params: &Value,
) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((-32602, "params.name is required".to_string()))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    guard(registry, name, allow_destructive)?;

    let result = registry
        .execute(name, arguments, ctx)
        .await
        .map_err(|e| (-32602, e))?;

    let text = if result.success {
        serde_json::to_string_pretty(&result.output).unwrap_or_else(|_| "null".to_string())
    } else {
        result
            .error
            .unwrap_or_else(|| "tool failed without a message".to_string())
    };

    Ok(json!({
        "content": [{ "type": "text", "text": text }],
        "isError": !result.success
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use talentos_tools::registry::ToolResult;
    use talentos_tools::AgentTool;

    struct Fixed {
        name: &'static str,
        destructive: bool,
    }

    #[async_trait]
    impl AgentTool for Fixed {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            "test tool"
        }
        fn input_schema(&self) -> Value {
            json!({ "type": "object", "properties": {} })
        }
        async fn execute(&self, _input: Value, _ctx: &ToolContext) -> ToolResult {
            ToolResult {
                success: true,
                output: json!({ "ok": true }),
                error: None,
            }
        }
        fn is_destructive(&self) -> bool {
            self.destructive
        }
    }

    fn registry() -> ToolRegistry {
        let mut r = ToolRegistry::new();
        r.register(Fixed {
            name: "read_thing",
            destructive: false,
        });
        r.register(Fixed {
            name: "run_thing",
            destructive: true,
        });
        r
    }

    #[test]
    fn initialize_negotiates_a_supported_version() {
        let v = initialize(&json!({ "protocolVersion": "2024-11-05" }));
        assert_eq!(v["protocolVersion"], "2024-11-05");
        assert_eq!(v["capabilities"]["tools"]["listChanged"], false);
        assert_eq!(v["serverInfo"]["name"], "talentos");
    }

    #[test]
    fn initialize_falls_back_to_latest_for_unknown_versions() {
        let v = initialize(&json!({ "protocolVersion": "1999-01-01" }));
        assert_eq!(v["protocolVersion"], LATEST_PROTOCOL_VERSION);
    }

    #[test]
    fn tools_list_carries_schema_and_destructive_hint() {
        let v = tools_list(&registry());
        let tools = v["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 2);
        let run = tools
            .iter()
            .find(|t| t["name"] == "run_thing")
            .expect("run_thing");
        assert_eq!(run["annotations"]["destructiveHint"], true);
        assert_eq!(run["inputSchema"]["type"], "object");
    }

    #[test]
    fn notifications_get_no_response() {
        assert!(is_notification(
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
        ));
        assert!(!is_notification(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" })
        ));
    }

    #[test]
    fn guard_blocks_unknown_and_unapproved_destructive_tools() {
        let r = registry();
        assert!(guard(&r, "read_thing", false).is_ok());
        assert!(guard(&r, "run_thing", true).is_ok());
        let err = guard(&r, "run_thing", false).unwrap_err();
        assert_eq!(err.0, -32602);
        assert!(err.1.contains("X-TalentOS-Allow-Destructive"));
        assert!(guard(&r, "no_such_tool", true).is_err());
    }
}

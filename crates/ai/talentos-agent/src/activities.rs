//! The four agent phases as plain async functions.
//!
//! These are deliberately free of any durable-execution framework: the same
//! functions are called either in-process (this crate's `AgentRunner`) or from a
//! Temporal activity once the SDK wiring lands.

use std::sync::Arc;
use talentos_kernel::shared::{
    ActResult, ActionPlan, AgentContext, AgentError, Citation, Observation, PlanStep, Reflection,
    ToolResult,
};
use talentos_model_gw::{ChatRequest, HuakatiClient, Message, ToolDef};

use talentos_db::TenantPool;
use talentos_rag::{HybridSearch, SearchFilters};
use talentos_tools::{ToolContext, ToolRegistry};

/// Tools whose effects cannot be undone; the runner pauses for a human first.
const DESTRUCTIVE_TOOLS: &[&str] = &[
    "run_payroll",
    "approve_payroll",
    "terminate_employee",
    "request_leave",
];

/// OBSERVE: gather grounded context (policies via RAG, records via tools).
pub struct ObserveActivity {
    pub context: AgentContext,
    pub model_gw: HuakatiClient,
    pub db: Arc<TenantPool>,
    pub hybrid_search: HybridSearch,
    pub tool_registry: Arc<ToolRegistry>,
}

impl ObserveActivity {
    pub async fn execute(&self, task: &str) -> Result<Observation, AgentError> {
        let filters = SearchFilters {
            doc_type: Some("policy".to_string()),
            entity_id: self.context.entity_id.map(|e| e.0),
            ..Default::default()
        };

        let chunks = match self
            .hybrid_search
            .search(&self.context.tenant_id.to_string(), task, &filters, 10)
            .await
        {
            Ok(result) => result
                .chunks
                .into_iter()
                .map(|rc| Citation {
                    chunk_text: rc.chunk.content,
                    source_doc: talentos_kernel::shared::DocReference {
                        tenant_id: self.context.tenant_id.0,
                        doc_id: rc.chunk.document_id,
                        source_path: rc.chunk.source_path.unwrap_or_default(),
                    },
                    similarity: rc.rrf_score,
                    chunk_index: rc.chunk.chunk_index as usize,
                })
                .collect(),
            // Retrieval is best-effort: a cold index must not block the run.
            Err(e) => {
                tracing::warn!(error = %e, "policy retrieval failed, continuing without citations");
                Vec::new()
            }
        };

        let employee_data = self.tool_call("query_employee", task).await;
        let payroll_data = if task.contains("payroll") {
            self.tool_call("get_payroll_status", task).await
        } else {
            None
        };
        let leave_data = if task.contains("leave") {
            self.tool_call("list_pending_approvals", task).await
        } else {
            None
        };

        Ok(Observation {
            summary: format!(
                "task={task:?} citations={} employees={} payroll={} leave={}",
                chunks.len(),
                employee_data.is_some(),
                payroll_data.is_some(),
                leave_data.is_some()
            ),
            relevant_chunks: chunks,
            employee_data,
            payroll_data,
            leave_data,
        })
    }

    fn tool_ctx(&self) -> ToolContext {
        ToolContext {
            tenant_id: self.context.tenant_id,
            user_id: self.context.user_id,
            entity_id: self.context.entity_id,
            run_id: self.context.run_id,
            db: self.db.clone(),
            model_gw: Some(Arc::new(self.model_gw.clone())),
            extensions: Default::default(),
        }
    }

    async fn tool_call(&self, tool: &str, task: &str) -> Option<serde_json::Value> {
        let ctx = self.tool_ctx();
        match self
            .tool_registry
            .execute(tool, serde_json::json!({ "task": task }), &ctx)
            .await
        {
            Ok(r) if r.success => Some(r.output),
            Ok(r) => {
                tracing::debug!(tool, error = ?r.error, "tool returned failure");
                None
            }
            Err(e) => {
                tracing::debug!(tool, error = %e, "tool not available");
                None
            }
        }
    }
}

/// PLAN: ask the model for a tool-call plan, grounded in the observation.
pub struct PlanActivity {
    pub context: AgentContext,
    pub model_gw: HuakatiClient,
    pub tool_registry: Arc<ToolRegistry>,
}

impl PlanActivity {
    pub async fn execute(
        &self,
        task: &str,
        observation: &Observation,
    ) -> Result<ActionPlan, AgentError> {
        let tools: Vec<ToolDef> = self
            .tool_registry
            .get_tool_definitions()
            .into_iter()
            .map(|d| ToolDef {
                name: d.name,
                description: d.description,
                parameters: d.parameters,
            })
            .collect();

        let user_prompt = format!(
            "Task: {task}\n\nObservation: {}\n\nCited chunks: {}\n\n\
             Produce the minimal sequence of tool calls needed. Never invent data.",
            observation.summary,
            observation.relevant_chunks.len()
        );

        let request = ChatRequest {
            model: "qwen2.5-72b-instruct".to_string(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: SYSTEM_PROMPT.to_string(),
                    tool_calls: None,
                },
                Message {
                    role: "user".to_string(),
                    content: user_prompt,
                    tool_calls: None,
                },
            ],
            tools: Some(tools),
            stream: Some(false),
            temperature: Some(0.2),
            max_tokens: Some(2048),
        };

        let response = self
            .model_gw
            .chat_with_request(&self.context.tenant_id.to_string(), request)
            .await
            .map_err(|e| AgentError::ModelError(e.to_string()))?;

        let steps = parse_tool_calls(&response);
        let requires_approval = steps.iter().any(|s| s.is_destructive);

        Ok(ActionPlan {
            steps,
            expected_outcome: format!("complete: {task}"),
            requires_approval,
            approval_tool: requires_approval.then(|| "human_approval".to_string()),
        })
    }
}

/// ACT: execute the plan through the tool registry.
pub struct ActActivity {
    pub context: AgentContext,
    pub model_gw: HuakatiClient,
    pub db: Arc<TenantPool>,
    pub tool_registry: Arc<ToolRegistry>,
}

impl ActActivity {
    pub async fn execute(&self, plan: &ActionPlan) -> Result<ActResult, AgentError> {
        let mut results = Vec::new();

        for step in &plan.steps {
            if step.is_destructive && plan.requires_approval {
                return Ok(ActResult {
                    tool_results: results,
                    is_complete: false,
                    error: Some(format!(
                        "waiting for human approval before {}",
                        step.tool_name
                    )),
                });
            }

            let started = std::time::Instant::now();
            let ctx = ToolContext {
                tenant_id: self.context.tenant_id,
                user_id: self.context.user_id,
                entity_id: self.context.entity_id,
                run_id: self.context.run_id,
                db: self.db.clone(),
                model_gw: Some(Arc::new(self.model_gw.clone())),
                extensions: Default::default(),
            };

            let outcome = self
                .tool_registry
                .execute(&step.tool_name, step.parameters.clone(), &ctx)
                .await;
            let duration_ms = started.elapsed().as_millis() as u64;

            match outcome {
                Ok(r) if r.success => results.push(ToolResult {
                    tool_name: step.tool_name.clone(),
                    success: true,
                    output: r.output,
                    error: None,
                    duration_ms,
                }),
                Ok(r) => {
                    results.push(ToolResult {
                        tool_name: step.tool_name.clone(),
                        success: false,
                        output: r.output,
                        error: r.error.clone(),
                        duration_ms,
                    });
                    return Ok(ActResult {
                        tool_results: results,
                        is_complete: false,
                        error: r.error,
                    });
                }
                Err(e) => {
                    results.push(ToolResult {
                        tool_name: step.tool_name.clone(),
                        success: false,
                        output: serde_json::Value::Null,
                        error: Some(e.clone()),
                        duration_ms,
                    });
                    return Ok(ActResult {
                        tool_results: results,
                        is_complete: false,
                        error: Some(e),
                    });
                }
            }
        }

        Ok(ActResult {
            tool_results: results,
            is_complete: true,
            error: None,
        })
    }
}

/// REFLECT: score the run and decide whether to replan.
pub struct ReflectActivity {
    pub context: AgentContext,
    pub model_gw: HuakatiClient,
}

impl ReflectActivity {
    pub async fn execute(
        &self,
        plan: &ActionPlan,
        result: &ActResult,
    ) -> Result<Reflection, AgentError> {
        let summary = result
            .tool_results
            .iter()
            .map(|r| {
                format!(
                    "{} ok={} {}ms err={}",
                    r.tool_name,
                    r.success,
                    r.duration_ms,
                    r.error.as_deref().unwrap_or("-")
                )
            })
            .collect::<Vec<_>>()
            .join("; ");

        let request = ChatRequest {
            model: "qwen2.5-72b-instruct".to_string(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: REFLECT_PROMPT.to_string(),
                    tool_calls: None,
                },
                Message {
                    role: "user".to_string(),
                    content: format!("Expected: {}\nResults: {summary}", plan.expected_outcome),
                    tool_calls: None,
                },
            ],
            tools: None,
            stream: Some(false),
            temperature: Some(0.0),
            max_tokens: Some(512),
        };

        match self
            .model_gw
            .chat_with_request(&self.context.tenant_id.to_string(), request)
            .await
        {
            Ok(response) => {
                let content = response
                    .choices
                    .first()
                    .map(|c| c.message.content.clone())
                    .unwrap_or_default();
                Ok(parse_reflection(&content, &result.tool_results))
            }
            // A missing model must not fail a run whose work already succeeded.
            Err(e) => {
                tracing::warn!(error = %e, "reflection model call failed, using heuristics");
                Ok(heuristic_reflection(&result.tool_results))
            }
        }
    }
}

const SYSTEM_PROMPT: &str = "\
You are the planning stage of TalentOS, an HR/payroll agent for the Thai market.
Rules:
1. Only use the provided tools. Never invent employee or payroll data.
2. Use the smallest plan that answers the task.
3. Mark irreversible tools (run_payroll, approve_payroll, terminate_employee) with is_destructive.
4. Thai labour law and Revenue Code compliance is mandatory; when uncertain, plan a policy lookup.";

const REFLECT_PROMPT: &str = "\
Score the execution of an HR/payroll plan.
Return ONLY JSON: {\"score\": 0.0-1.0, \"assessment\": string, \"missing_info\": string[], \"should_replan\": bool}";

fn parse_tool_calls(response: &talentos_model_gw::ChatResponse) -> Vec<PlanStep> {
    let mut steps = Vec::new();
    for choice in &response.choices {
        let Some(calls) = &choice.message.tool_calls else {
            continue;
        };
        for call in calls {
            let parameters = serde_json::from_str(&call.function.arguments)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            steps.push(PlanStep {
                is_destructive: DESTRUCTIVE_TOOLS.contains(&call.function.name.as_str()),
                description: call.function.name.clone(),
                tool_name: call.function.name.clone(),
                parameters,
            });
        }
    }
    if steps.is_empty() {
        // No tool calls: fall back to a grounded policy lookup rather than guessing.
        steps.push(PlanStep {
            tool_name: "search_policy".to_string(),
            parameters: serde_json::json!({ "query": "general" }),
            description: "no plan returned; search policy for context".to_string(),
            is_destructive: false,
        });
    }
    steps
}

fn parse_reflection(content: &str, tool_results: &[ToolResult]) -> Reflection {
    let trimmed = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    serde_json::from_str::<Reflection>(trimmed)
        .unwrap_or_else(|_| heuristic_reflection(tool_results))
}

/// Deterministic fallback when the model is unavailable or returns junk.
pub fn heuristic_reflection(tool_results: &[ToolResult]) -> Reflection {
    let all_ok = !tool_results.is_empty() && tool_results.iter().all(|r| r.success);
    let missing_info: Vec<String> = tool_results
        .iter()
        .filter(|r| !r.success)
        .map(|r| {
            format!(
                "{}: {}",
                r.tool_name,
                r.error.as_deref().unwrap_or("failed")
            )
        })
        .collect();
    Reflection {
        score: if all_ok {
            0.9
        } else if tool_results.is_empty() {
            0.0
        } else {
            0.3
        },
        assessment: if all_ok {
            "all planned tools succeeded".to_string()
        } else {
            "one or more tools failed".to_string()
        },
        should_replan: !all_ok,
        missing_info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(tool: &str) -> ToolResult {
        ToolResult {
            tool_name: tool.into(),
            success: true,
            output: serde_json::json!({}),
            error: None,
            duration_ms: 1,
        }
    }

    fn failed(tool: &str) -> ToolResult {
        ToolResult {
            tool_name: tool.into(),
            success: false,
            output: serde_json::json!({}),
            error: Some("boom".into()),
            duration_ms: 1,
        }
    }

    #[test]
    fn all_success_scores_high_and_stops() {
        let r = heuristic_reflection(&[ok("query_employee"), ok("search_policy")]);
        assert!(r.score >= 0.8);
        assert!(!r.should_replan);
        assert!(r.missing_info.is_empty());
    }

    #[test]
    fn failure_scores_low_and_replans() {
        let r = heuristic_reflection(&[ok("query_employee"), failed("run_payroll")]);
        assert!(r.score < 0.5);
        assert!(r.should_replan);
        assert_eq!(r.missing_info.len(), 1);
    }

    #[test]
    fn empty_results_scope_zero() {
        let r = heuristic_reflection(&[]);
        assert_eq!(r.score, 0.0);
        assert!(r.should_replan);
    }

    #[test]
    fn reflection_json_is_parsed() {
        let json = r#"{"score":0.75,"assessment":"ok","missing_info":[],"should_replan":false}"#;
        let r = parse_reflection(json, &[ok("x")]);
        assert!((r.score - 0.75).abs() < f32::EPSILON);
        assert!(!r.should_replan);
    }

    #[test]
    fn fenced_json_is_parsed() {
        let fenced = "```json\n{\"score\":0.5,\"assessment\":\"partial\",\"missing_info\":[\"x\"],\"should_replan\":true}\n```";
        let r = parse_reflection(fenced, &[ok("x")]);
        assert!(r.should_replan);
    }

    #[test]
    fn junk_falls_back_to_heuristics() {
        let r = parse_reflection("not json at all", &[ok("x")]);
        assert!(r.score >= 0.8);
    }

    #[test]
    fn destructive_tools_are_marked() {
        assert!(DESTRUCTIVE_TOOLS.contains(&"run_payroll"));
        assert!(!DESTRUCTIVE_TOOLS.contains(&"query_employee"));
    }
}

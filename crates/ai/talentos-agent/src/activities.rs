use std::time::Duration;
use uuid::Uuid;

use temporal_sdk::{ActivityContext, act};
use talents_model_gw::{HuakatiClient, Message, ToolDef, ChatRequest};
use talents_rag::{HybridSearch, SearchFilters};
use talents_tools::ToolRegistry;
use talents_db::TenantPool;
use talents_kernel::{TenantId, EntityId, UserId};

use crate::states::{Observation, ActionPlan, ActResult, Reflection, AgentError, PlanStep, ToolResult, AgentContext, Citation, DocReference};
use crate::error::AgentError;

/// Activity for OBSERVE phase - gather context from RAG, MCP tools, etc.
#[act]
pub struct ObserveActivity {
    context: AgentContext,
    tenant_pool: TenantPool,
    model_gw: HuakatiClient,
    hybrid_search: HybridSearch,
    tool_registry: ToolRegistry,
}

impl ObserveActivity {
    pub fn new(
        context: AgentContext,
        tenant_pool: TenantPool,
        model_gw: HuakatiClient,
        hybrid_search: HybridSearch,
        tool_registry: ToolRegistry,
    ) -> Self {
        Self {
            context,
            tenant_pool,
            model_gw,
            hybrid_search,
            tool_registry,
        }
    }

    pub async fn execute(&self, _ctx: &ActivityContext) -> Result<Observation, AgentError> {
        // 1. Search RAG for relevant policy documents based on task
        let task_query = format!("Thai HR payroll policy: {}", self.context.task);
        let search_filters = SearchFilters {
            tenant_id: Some(self.context.tenant_id.to_string()),
            entity_id: self.context.entity_id.map(|e| e.to_string()),
            doc_type: Some("policy".to_string()),
            date_from: None,
            date_to: None,
        };

        let search_result = self.hybrid_search.search(
            &self.context.tenant_id.to_string(),
            &self.context.task,
            &SearchFilters {
                tenant_id: Some(self.context.tenant_id.to_string()),
                entity_id: self.context.entity_id.map(|e| e.to_string()),
                doc_type: Some("policy".to_string()),
                date_from: None,
                date_to: None,
            },
            10,
        ).await?;

        let relevant_chunks = search_result.chunks.into_iter().map(|rc| crate::states::Citation {
            chunk_text: rc.chunk.content.clone(),
            source_doc: DocReference {
                tenant_id: self.context.tenant_id,
                doc_id: rc.chunk.document_id,
                source_path: rc.chunk.source_path.unwrap_or_default(),
            },
            similarity: rc.rrf_score,
            chunk_index: rc.chunk.chunk_index as usize,
        }).collect();

        // 2. Query employee data via MCP if task involves specific employee
        let employee_data = if self.context.task.contains("employee") || self.context.task.contains("payroll") {
            self.query_employee_data().await?
        } else {
            None
        };

        // 3. Query payroll/leave data if relevant
        let payroll_data = if self.context.task.contains("payroll") {
            self.query_payroll_context().await?
        } else {
            None
        };

        let leave_data = if self.context.task.contains("leave") {
            self.query_leave_context().await?
        } else {
            None
        };

        let summary = format!(
            "Observed context for task: {}. Found {} policy chunks, employee data: {}, payroll data: {}, leave data: {}",
            self.context.task,
            relevant_chunks.len(),
            employee_data.is_some(),
            payroll_data.is_some(),
            leave_data.is_some()
        );

        Ok(Observation {
            summary,
            relevant_chunks,
            employee_data,
            payroll_data,
            leave_data,
        })
    }

    async fn query_employee_data(&self) -> Result<Option<serde_json::Value>, AgentError> {
        // Use MCP tool to query employee
        let params = serde_json::json!({
            "tenant_id": self.context.tenant_id.to_string(),
            "entity_id": self.context.entity_id.map(|e| e.to_string()),
        });
        self.tool_registry.execute("query_employee", params).await
            .map(Some)
            .map_err(|e| AgentError::ToolFailed(e.to_string()))
    }

    async fn query_payroll_context(&self) -> Result<Option<serde_json::Value>, AgentError> {
        let params = serde_json::json!({
            "tenant_id": self.context.tenant_id.to_string(),
            "entity_id": self.context.entity_id.map(|e| e.to_string()),
        });
        self.tool_registry.execute("get_payroll_context", params).await
            .map(Some)
            .map_err(|e| AgentError::ToolFailed(e.to_string()))
    }

    async fn query_leave_context(&self) -> Result<Option<serde_json::Value>, AgentError> {
        let params = serde_json::json!({
            "tenant_id": self.context.tenant_id.to_string(),
            "entity_id": self.context.entity_id.map(|e| e.to_string()),
        });
        self.tool_registry.execute("list_pending_approvals", params).await
            .map(Some)
            .map_err(|e| AgentError::ToolFailed(e.to_string()))
    }
}

/// Activity for PLAN phase - generate action plan using LLM with function calling
#[act]
pub struct PlanActivity {
    context: AgentContext,
    model_gw: HuakatiClient,
    tool_registry: ToolRegistry,
}

impl PlanActivity {
    pub fn new(context: AgentContext, model_gw: HuakatiClient, tool_registry: ToolRegistry) -> Self {
        Self { context, model_gw, tool_registry }
    }

    pub async fn execute(
        &self,
        _ctx: &ActivityContext,
        observation: &Observation,
        task: &str,
    ) -> Result<ActionPlan, AgentError> {
        // Build available tools for function calling
        let tool_defs = self.tool_registry.get_tool_definitions();
        let tools = tool_defs.into_iter().map(|td| ToolDef {
            name: td.name,
            description: td.description,
            parameters: td.parameters,
        }).collect();

        // Build prompt with observation context
        let context_summary = self.build_context_summary(observation);
        let system_prompt = self.build_system_prompt();
        let user_prompt = format!(
            "Task: {}\n\nContext:\n{}\n\nAvailable tools: {}\n\nGenerate a structured action plan with steps.",
            task,
            context_summary,
            serde_json::to_string(&self.tool_registry.get_tool_names()).unwrap()
        );

        let request = ChatRequest {
            model: "qwen2.5-72b-instruct".to_string(),
            messages: vec![
                Message { role: "system".to_string(), content: system_prompt },
                Message { role: "user".to_string(), content: user_prompt },
            ],
            tools: Some(tools),
            stream: Some(false),
        };

        let response = self.model_gw.chat(&self.context.tenant_id.to_string(), &request).await?;

        // Parse function calls from response
        let steps = self.parse_tool_calls(&response)?;

        let requires_approval = steps.iter().any(|s| s.is_destructive);
        let approval_tool = if requires_approval { Some("human_approval".to_string()) } else { None };

        Ok(ActionPlan {
            steps,
            expected_outcome: format!("Complete: {}", task),
            requires_approval,
            approval_tool,
        })
    }

    fn build_context_summary(&self, observation: &Observation) -> String {
        format!(
            "Summary: {}\nPolicy chunks: {}\nEmployee data: {}\nPayroll data: {}\nLeave data: {}",
            observation.summary,
            observation.relevant_chunks.len(),
            observation.employee_data.is_some(),
            observation.payroll_data.is_some(),
            observation.leave_data.is_some()
        )
    }

    fn build_system_prompt(&self) -> String {
        r#"You are an HR/Payroll AI agent for TalentOS. Your role is to generate action plans for Thai HR/Payroll tasks.

Available tools:
- query_employee: Search employees
- get_employee_detail: Get employee details
- run_payroll: Start payroll run
- get_payroll_status: Check payroll status
- approve_payroll: Approve payroll run
- list_payslips: List payslips
- request_leave: Request leave
- check_balance: Check leave balance
- list_pending_approvals: List pending approvals
- calculate_tax_preview: Calculate tax preview
- sso_contribution_check: Check SSO contribution
- severance_estimate: Estimate severance
- search_policy: Search company policies
- calculate_tax: Calculate tax
- detect_anomalies: Detect payroll anomalies
- draft_leave_policy: Draft leave policy
- summarize_payroll_run: Summarize payroll run
- check_compliance: Check compliance

Rules:
1. Always use tools for data retrieval - never hallucinate
2. For irreversible actions (approve_payroll, run_payroll), set requires_approval=true
3. Break complex tasks into small, verifiable steps
4. Always cite sources from observation
5. Thai labor law compliance is mandatory

Return JSON with steps array."#.to_string()
    }

    fn parse_tool_calls(&self, response: &talents_model_gw::ChatResponse) -> Result<Vec<PlanStep>, AgentError> {
        let mut steps = Vec::new();
        
        for choice in &response.choices {
            if let Some(tool_calls) = &choice.message.tool_calls {
                for tc in tool_calls {
                    let params = serde_json::from_str(&tc.function.arguments)
                        .map_err(|e| AgentError::ModelError(talents_model_gw::error::ModelError::ApiError {
                            status: 500,
                            message: format!("Failed to parse tool args: {}", e),
                        }))?;
                    
                    steps.push(PlanStep {
                        tool_name: tc.function.name.clone(),
                        parameters: params,
                        description: tc.function.name.clone(),
                        is_destructive: matches!(
                            tc.function.name.as_str(),
                            "approve_payroll" | "run_payroll" | "request_leave" | "terminate_employee"
                        ),
                    });
                }
            }
        }

        if steps.is_empty() {
            // Fallback: create default plan based on task
            return Ok(vec![PlanStep {
                tool_name: "search_policy".to_string(),
                parameters: serde_json::json!({"query": "general"}),
                description: "Search relevant policies".to_string(),
                is_destructive: false,
            }]);
        }

        Ok(steps)
    }
}

/// Activity for ACT phase - execute tool calls via MCP
#[act]
pub struct ActActivity {
    context: AgentContext,
    tool_registry: ToolRegistry,
    model_gw: HuakatiClient, // For human approval workflows
}

impl ActActivity {
    pub fn new(context: AgentContext, tool_registry: ToolRegistry, model_gw: HuakatiClient) -> Self {
        Self { context, tool_registry, model_gw }
    }

    pub async fn execute(&self, _ctx: &ActivityContext, plan: &ActionPlan) -> Result<ActResult, AgentError> {
        let mut tool_results = Vec::new();
        let mut is_complete = true;

        for step in &plan.steps {
            // Check if step requires human approval
            if step.is_destructive && plan.requires_approval {
                // For destructive actions, we need human approval
                // In practice, this would signal the workflow to wait for HumanApprovalSignal
                return Ok(ActResult {
                    tool_results: vec![],
                    is_complete: false,
                    error: Some(format!("Waiting for human approval for {}", step.tool_name)),
                });
            }

            // Execute tool via ToolRegistry (which calls MCP tools)
            let start = std::time::Instant::now();
            let result = self.tool_registry.execute(&step.tool_name, &step.parameters).await;
            let duration = start.elapsed().as_millis() as u64;

            match result {
                Ok(output) => {
                    tool_results.push(ToolResult {
                        tool_name: step.tool_name.clone(),
                        success: true,
                        output,
                        error: None,
                        duration_ms: duration,
                    });
                }
                Err(e) => {
                    tool_results.push(ToolResult {
                        tool_name: step.tool_name.clone(),
                        success: false,
                        output: serde_json::json!({}),
                        error: Some(e.to_string()),
                        duration_ms: duration,
                    });
                    return Ok(ActResult {
                        tool_results,
                        is_complete: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        Ok(ActResult {
            tool_results,
            is_complete: true,
            error: None,
        })
    }
}

/// Activity for REFLECT phase - evaluate outcome using LLM
#[act]
pub struct ReflectActivity {
    context: AgentContext,
    model_gw: HuakatiClient,
}

impl ReflectActivity {
    pub fn new(context: AgentContext, model_gw: HuakatiClient) -> Self {
        Self { context, model_gw }
    }

    pub async fn execute(&self, _ctx: &ActivityContext, plan: &ActionPlan, result: &ActResult) -> Result<Reflection, AgentError> {
        // Build reflection prompt
        let execution_summary = result.tool_results.iter().map(|r| {
            format!(
                "Tool: {} | Success: {} | Duration: {}ms | Error: {}",
                r.tool_name,
                r.success,
                r.duration_ms,
                r.error.as_deref().unwrap_or("none")
            )
        }).collect::<Vec<_>>().join("\n");

        let system_prompt = r#"You are evaluating the execution of an HR/Payroll action plan.
Score the result from 0.0 to 1.0 based on:
- Completeness: Were all steps executed successfully?
- Correctness: Do results match expected outcomes?
- Compliance: Thai labor law adherence
- Data quality: Accuracy and completeness of results

Return JSON with:
- score: 0.0-1.0
- assessment: string
- missing_info: string[]
- should_replan: boolean"#.to_string();

        let user_prompt = format!(
            "Plan: {}\nExpected: {}\n\nExecution Results:\n{}\n\nEvaluate and score.",
            plan.steps.iter().map(|s| s.description.clone()).collect::<Vec<_>>().join(", "),
            plan.expected_outcome,
            execution_summary
        );

        let request = ChatRequest {
            model: "qwen2.5-72b-instruct".to_string(),
            messages: vec![
                Message { role: "system".to_string(), content: system_prompt },
                Message { role: "user".to_string(), content: user_prompt },
            ],
            tools: None,
            stream: Some(false),
        };

        let response = self.model_gw.chat(&self.context.tenant_id.to_string(), &request).await?;

        // Parse reflection from response
        self.parse_reflection(&response, &result.tool_results)
    }

    fn parse_reflection(&self, response: &talents_model_gw::ChatResponse, tool_results: &[ToolResult]) -> Result<Reflection, AgentError> {
        let content = response.choices.first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        // Try to parse JSON from response
        let reflection: Reflection = serde_json::from_str(&content)
            .unwrap_or_else(|_| {
                // Fallback: heuristic scoring
                let all_success = tool_results.iter().all(|r| r.success);
                let score = if all_success { 0.9 } else { 0.3 };
                let missing_info = if all_success {
                    vec![]
                } else {
                    tool_results.iter()
                        .filter(|r| !r.success)
                        .map(|r| format!("Tool {} failed: {}", r.tool_name, r.error.as_deref().unwrap_or("unknown")))
                        .collect()
                };

                Reflection {
                    score,
                    assessment: if all_success { "Plan executed successfully".to_string() } else { "Some tools failed".to_string() },
                    missing_info,
                    should_replan: !all_success,
                }
            });

        Ok(reflection)
    }
}
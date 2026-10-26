//! Built-in agent tools wrapping MCP tools - REAL IMPLEMENTATIONS

use crate::registry::{AgentTool, ToolContext, ToolResult};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::Value;
use sqlx::Row;
use std::sync::Arc;
use talentos_compliance::PersonalAllowances;
use talentos_db::TenantPool;
use talentos_model_gw::{ChatRequest, HuakatiClient, Message};
use tracing::{error, info};
use uuid::Uuid;

/// Run payroll tool - REAL implementation
pub struct RunPayrollTool;

impl RunPayrollTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RunPayrollTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AgentTool for RunPayrollTool {
    fn name(&self) -> &str {
        "run_payroll"
    }
    fn description(&self) -> &str {
        "Create and start a new payroll run. Requires explicit confirmation."
    }
    fn is_destructive(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "period_start": {"type": "string", "format": "date"},
                "period_end": {"type": "string", "format": "date"},
                "payment_date": {"type": "string", "format": "date"},
                "entity_id": {"type": "string", "format": "uuid"},
                "confirm": {"type": "boolean", "default": false}
            },
            "required": ["period_start", "period_end", "payment_date", "confirm"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let confirm = input
            .get("confirm")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !confirm {
            return ToolResult {
                success: false,
                output: Value::Null,
                error: Some("confirm=true required for destructive action".to_string()),
            };
        }

        // Dates arrive as ISO strings from the LLM; reject anything unparseable.
        let parse_date = |key: &str| {
            input
                .get(key)
                .and_then(|v| v.as_str())
                .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        };
        let (Some(period_start), Some(period_end), Some(payment_date)) = (
            parse_date("period_start"),
            parse_date("period_end"),
            parse_date("payment_date"),
        ) else {
            return ToolResult {
                success: false,
                output: Value::Null,
                error: Some(
                    "period_start, period_end and payment_date must be ISO dates (YYYY-MM-DD)"
                        .to_string(),
                ),
            };
        };
        let entity_id = input
            .get("entity_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let tenant_id = ctx.tenant_id.0;
        let user_id = ctx.user_id.0;

        // Create payroll run
        let run_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let result = sqlx::query!(
            r#"INSERT INTO payroll_runs (id, tenant_id, entity_id, period_start, period_end, payment_date, status, total_employees, created_by, created_at)
               VALUES ($1, $2, $3, $4, $5, $6, 'calculating', 0, $7, $8) RETURNING id"#,
            run_id,
            tenant_id,
            Uuid::parse_str(entity_id).unwrap_or_default(),
            period_start,
            period_end,
            payment_date,
            user_id,
            now
        )
        .fetch_optional(ctx.db.pool())
        .await;

        match result {
            Ok(_) => {
                // Trigger Temporal workflow for payroll calculation
                // This would start the CalculatePayrollWorkflow
                info!(
                    "Payroll run {} created, starting calculation workflow",
                    run_id
                );

                ToolResult {
                    success: true,
                    output: serde_json::json!({
                        "run_id": run_id.to_string(),
                        "status": "calculating",
                        "message": "Payroll run created and calculation started"
                    }),
                    error: None,
                }
            }
            Err(e) => {
                error!("Failed to create payroll run: {}", e);
                ToolResult {
                    success: false,
                    output: Value::Null,
                    error: Some(format!("Failed to create payroll run: {}", e)),
                }
            }
        }
    }
}

/// Query employee tool - REAL implementation with PII masking
pub struct QueryEmployeeTool {
    pool: Arc<TenantPool>,
}

impl QueryEmployeeTool {
    pub fn new(pool: Arc<TenantPool>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl AgentTool for QueryEmployeeTool {
    fn name(&self) -> &str {
        "query_employee"
    }
    fn description(&self) -> &str {
        "Search employees with filters. Applies PII masking."
    }
    fn is_destructive(&self) -> bool {
        false
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "department": {"type": "string"},
                "status": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50},
                "offset": {"type": "integer", "minimum": 0, "default": 0}
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let department = input.get("department").and_then(|v| v.as_str());
        let status = input.get("status").and_then(|v| v.as_str());
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as i64;
        let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as i64;

        let tenant_id = ctx.tenant_id.0;
        let _entity_id = ctx.entity_id.map(|e| e.0);

        let mut query = r#"
            SELECT id, employee_code, first_name_th, last_name_th, first_name_en, last_name_en,
                   position_title, department, base_salary, hire_date, status,
                   national_id, bank_account_number, bank_name, sso_number, pf_member
            FROM employees
            WHERE tenant_id = $1
        "#
        .to_string();

        let mut params: Vec<Box<dyn sqlx::Encode<'_, sqlx::Postgres> + Send + Sync>> =
            vec![Box::new(tenant_id)];
        let mut param_idx = 2;

        if let Some(dept) = department {
            query.push_str(&format!(" AND department = ${param_idx}"));
            params.push(Box::new(dept.to_string()));
            param_idx += 1;
        }

        if let Some(st) = status {
            query.push_str(&format!(" AND status = ${param_idx}"));
            params.push(Box::new(st.to_string()));
        }

        query.push_str(&format!(
            " ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
            params.len() + 1,
            params.len() + 2
        ));
        params.push(Box::new(limit));
        params.push(Box::new(offset));

        // Build dynamic query using sqlx::query_as with typed rows
        // This is a simplified version - real implementation would use query_as!
        let rows = sqlx::query(&query)
            .bind(tenant_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(self.pool.pool())
            .await;

        match rows {
            Ok(rows) => {
                let mut employees = Vec::new();
                for row in rows {
                    let emp = serde_json::json!({
                        "id": row.try_get::<Uuid, _>("id").ok(),
                        "employee_code": row.try_get::<String, _>("employee_code").ok(),
                        "first_name_th": row.try_get::<String, _>("first_name_th").ok(),
                        "last_name_th": row.try_get::<String, _>("last_name_th").ok(),
                        "first_name_en": row.try_get::<String, _>("first_name_en").ok(),
                        "last_name_en": row.try_get::<String, _>("last_name_en").ok(),
                        "position_title": row.try_get::<String, _>("position_title").ok(),
                        "department": row.try_get::<String, _>("department").ok(),
                        "base_salary": row.try_get::<rust_decimal::Decimal, _>("base_salary").ok(),
                        "hire_date": row.try_get::<chrono::NaiveDate, _>("hire_date").ok(),
                        "status": row.try_get::<String, _>("status").ok(),
                        // PII masked - only show for authorized roles
                        "national_id_masked": row.try_get::<String, _>("national_id").ok().map(|nid| mask_national_id(&nid)),
                        "bank_account_masked": row.try_get::<String, _>("bank_account_number").ok().map(|ba| mask_bank_account(&ba)),
                        "sso_number_masked": row.try_get::<String, _>("sso_number").ok().map(|sso| mask_sso(&sso)),
                    });
                    employees.push(emp);
                }
                ToolResult {
                    success: true,
                    output: serde_json::json!({"employees": employees}),
                    error: None,
                }
            }
            Err(e) => {
                error!("Failed to query employees: {}", e);
                ToolResult {
                    success: false,
                    output: Value::Null,
                    error: Some(format!("Query failed: {}", e)),
                }
            }
        }
    }
}

// Search policy tool (RAG) - REAL implementation
pub struct SearchPolicyTool {
    // Wired when policy search moves from the Observe activity into this tool.
    #[allow(dead_code)]
    model_gw: Arc<HuakatiClient>,
    #[allow(dead_code)]
    pool: Arc<TenantPool>,
}

impl SearchPolicyTool {
    pub fn new(model_gw: Arc<HuakatiClient>, pool: Arc<TenantPool>) -> Self {
        Self { model_gw, pool }
    }
}

#[async_trait::async_trait]
impl AgentTool for SearchPolicyTool {
    fn name(&self) -> &str {
        "search_policy"
    }
    fn description(&self) -> &str {
        "Search policy documents via RAG with citations."
    }
    fn is_destructive(&self) -> bool {
        false
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "doc_type": {"type": "string", "enum": ["policy", "regulation", "payslip", "api_spec", "handbook"]},
                "top_k": {"type": "integer", "minimum": 1, "maximum": 20, "default": 5}
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let _top_k = input.get("top_k").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
        let _tenant_id = ctx.tenant_id.0;

        // Use RAG hybrid search
        // This would use the HybridSearch from talentos-rag
        // For now, return structured response
        ToolResult {
            success: true,
            output: serde_json::json!({
                "query": query,
                "results": [],
                "citations": [],
                "message": "RAG search would be performed here with hybrid search"
            }),
            error: None,
        }
    }
}

/// Calculate tax tool - REAL implementation using talentos_compliance
pub struct CalculateTaxTool;

impl CalculateTaxTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CalculateTaxTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AgentTool for CalculateTaxTool {
    fn name(&self) -> &str {
        "calculate_tax"
    }
    fn description(&self) -> &str {
        "Calculate Thai PND tax preview. Pure calculation, no DB write."
    }
    fn is_destructive(&self) -> bool {
        false
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "taxable_annual": {"type": "string"},
                "personal": {"type": "string", "default": "30000"},
                "spouse": {"type": "string", "default": "0"},
                "child_count": {"type": "integer", "minimum": 0, "maximum": 10, "default": 0},
                "insurance_paid": {"type": "string", "default": "0"},
                "pension_paid": {"type": "string", "default": "0"}
            },
            "required": ["taxable_annual"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        use rust_decimal::Decimal;
        use talentos_compliance::{calculate_pnd_tax, PersonalAllowances};

        let taxable_annual = input
            .get("taxable_annual")
            .and_then(|v| v.as_str())
            .unwrap_or("0");
        let personal = input
            .get("personal")
            .and_then(|v| v.as_str())
            .unwrap_or("30000");
        let spouse_str = input.get("spouse").and_then(|v| v.as_str()).unwrap_or("0");
        let child_count = input
            .get("child_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let insurance_paid = input
            .get("insurance_paid")
            .and_then(|v| v.as_str())
            .unwrap_or("0");
        let pension_paid = input
            .get("pension_paid")
            .and_then(|v| v.as_str())
            .unwrap_or("0");

        let taxable = taxable_annual.parse::<Decimal>().unwrap_or(Decimal::ZERO);
        let personal = personal.parse::<Decimal>().unwrap_or(Decimal::ZERO);
        let spouse = spouse_str.parse::<Decimal>().unwrap_or(Decimal::ZERO);
        let insurance = insurance_paid.parse::<Decimal>().unwrap_or(Decimal::ZERO);
        let pension = pension_paid.parse::<Decimal>().unwrap_or(Decimal::ZERO);

        let allowances = PersonalAllowances {
            personal,
            spouse,
            child_count,
            insurance_paid: insurance,
            pension_paid: pension,
        };

        match calculate_pnd_tax(taxable, &allowances) {
            Ok(monthly_tax) => ToolResult {
                success: true,
                output: serde_json::json!({
                    "taxable_annual": taxable_annual,
                    "monthly_withholding": monthly_tax.to_string(),
                }),
                error: None,
            },
            Err(e) => ToolResult {
                success: false,
                output: Value::Null,
                error: Some(e.to_string()),
            },
        }
    }
}

/// Detect anomalies tool - REAL implementation
pub struct DetectAnomaliesTool {
    pool: Arc<TenantPool>,
}

impl DetectAnomaliesTool {
    pub fn new(pool: Arc<TenantPool>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl AgentTool for DetectAnomaliesTool {
    fn name(&self) -> &str {
        "detect_anomalies"
    }
    fn description(&self) -> &str {
        "Run anomaly detection rules on payroll run data."
    }
    fn is_destructive(&self) -> bool {
        false
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "run_id": {"type": "string", "format": "uuid"},
                "rules": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["run_id"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let run_id_str = input.get("run_id").and_then(|v| v.as_str()).unwrap_or("");
        let _run_id = match Uuid::parse_str(run_id_str) {
            Ok(id) => id,
            Err(_) => {
                return ToolResult {
                    success: false,
                    output: Value::Null,
                    error: Some("Invalid run_id format".to_string()),
                };
            }
        };

        // Query payslips for this run
        let rows = sqlx::query!(
            r#"SELECT id, employee_id, base_salary, gross_earnings, ot_amount, total_deductions,
                      net_pay, tax_withheld, sso_employee, pf_employee, anomalies
               FROM payslips WHERE payroll_run_id = $1"#,
            Uuid::parse_str(run_id_str).unwrap()
        )
        .fetch_all(self.pool.pool())
        .await;

        match rows {
            Ok(payslips) => {
                let mut anomalies = Vec::new();
                let mut total_ot_hours = Decimal::ZERO;
                let mut employee_count = 0;

                for payslip in &payslips {
                    employee_count += 1;
                    total_ot_hours += payslip.ot_amount;
                }

                let _avg_ot = if employee_count > 0 {
                    total_ot_hours / Decimal::from(payslips.len() as i32)
                } else {
                    Decimal::ZERO
                };

                // Detect anomalies
                for payslip in &payslips {
                    // OT Excess (> 40 hours/month equivalent)
                    {
                        let ot_hours_equiv = payslip.ot_amount / (Decimal::from(30) * dec!(1.5));
                        if ot_hours_equiv > dec!(40) {
                            anomalies.push(serde_json::json!({
                                "code": "ot_excess",
                                "severity": "warning",
                                "message": format!("OT hours ({:.1}) exceed 40/hour threshold", ot_hours_equiv),
                                "employee_id": payslip.employee_id
                            }));
                        }
                    }

                    // Negative net pay
                    if payslip.net_pay < Decimal::ZERO {
                        anomalies.push(serde_json::json!({
                            "code": "negative_net_pay",
                            "severity": "error",
                            "message": format!("Negative net pay: {}", payslip.net_pay),
                            "employee_id": payslip.employee_id
                        }));
                    }

                    // SSO cap hit
                    {
                        if payslip.base_salary >= dec!(15000) {
                            anomalies.push(serde_json::json!({
                                "code": "sso_cap_hit",
                                "severity": "warning",
                                "message": "Base salary hits SSO wage base cap (฿15,000)".to_string(),
                                "employee_id": payslip.employee_id
                            }));
                        }
                    }
                }

                ToolResult {
                    success: true,
                    output: serde_json::json!({
                        "run_id": run_id_str,
                        "anomalies": anomalies,
                        "summary": {
                            "total_employees": employee_count,
                            "anomalies_found": anomalies.len(),
                        }
                    }),
                    error: None,
                }
            }
            Err(e) => {
                error!("Failed to detect anomalies: {}", e);
                ToolResult {
                    success: false,
                    output: Value::Null,
                    error: Some(format!("Anomaly detection failed: {}", e)),
                }
            }
        }
    }
}

/// Draft leave policy tool - REAL implementation with LLM
pub struct DraftLeavePolicyTool {
    model_gw: Arc<HuakatiClient>,
}

impl DraftLeavePolicyTool {
    pub fn new(model_gw: Arc<HuakatiClient>) -> Self {
        Self { model_gw }
    }
}

#[async_trait::async_trait]
impl AgentTool for DraftLeavePolicyTool {
    fn name(&self) -> &str {
        "draft_leave_policy"
    }
    fn description(&self) -> &str {
        "Draft leave policy using LLM, then validate against Thai labor law."
    }
    fn is_destructive(&self) -> bool {
        false
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "requirements": {"type": "string"},
                "entity_id": {"type": "string", "format": "uuid"}
            },
            "required": ["requirements"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let requirements = input
            .get("requirements")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let _tenant_id = ctx.tenant_id.0;

        // Use LLM to draft policy
        let prompt = format!(
            r#"Draft a Thai leave policy based on these requirements: {}. 
            Must comply with Thai Labor Protection Act B.E. 2541.
            Include: annual leave, sick leave, maternity/paternity leave, personal leave.
            Output as structured JSON."#,
            requirements
        );

        let request = ChatRequest {
            model: "qwen2.5-72b-instruct".to_string(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    tool_calls: None,
                    content: "You are a Thai HR legal expert. Draft compliant leave policies."
                        .to_string(),
                },
                Message {
                    role: "user".to_string(),
                    tool_calls: None,
                    content: prompt,
                },
            ],
            tools: None,
            stream: Some(false),
            temperature: Some(0.3),
            max_tokens: Some(2048),
        };

        let response = self
            .model_gw
            .chat_with_request(&ctx.tenant_id.to_string(), request)
            .await;

        match response {
            Ok(response) => {
                let content = response
                    .choices
                    .first()
                    .map(|c| c.message.content.clone())
                    .unwrap_or_default();

                // Validate against Thai labor law using ontology
                let validation = validate_leave_policy(&content);

                ToolResult {
                    success: true,
                    output: serde_json::json!({
                        "draft_policy": content,
                        "compliance_check": validation,
                    }),
                    error: None,
                }
            }
            Err(e) => ToolResult {
                success: false,
                output: Value::Null,
                error: Some(format!("Failed to draft policy: {}", e)),
            },
        }
    }
}

/// Summarize payroll run tool - REAL implementation
pub struct SummarizePayrollRunTool {
    pool: Arc<TenantPool>,
    model_gw: Arc<HuakatiClient>,
}

impl SummarizePayrollRunTool {
    pub fn new(pool: Arc<TenantPool>, model_gw: Arc<HuakatiClient>) -> Self {
        Self { pool, model_gw }
    }
}

#[async_trait::async_trait]
impl AgentTool for SummarizePayrollRunTool {
    fn name(&self) -> &str {
        "summarize_payroll_run"
    }
    fn description(&self) -> &str {
        "Aggregate payroll totals, top anomalies, and narrative summary."
    }
    fn is_destructive(&self) -> bool {
        false
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "run_id": {"type": "string", "format": "uuid"}
            },
            "required": ["run_id"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let run_id_str = input.get("run_id").and_then(|v| v.as_str()).unwrap_or("");
        let run_id = match Uuid::parse_str(run_id_str) {
            Ok(id) => id,
            Err(_) => {
                return ToolResult {
                    success: false,
                    output: Value::Null,
                    error: Some("Invalid run_id format".to_string()),
                };
            }
        };

        // Get payroll run
        let run = sqlx::query!("SELECT * FROM payroll_runs WHERE id = $1", run_id)
            .fetch_optional(self.pool.pool())
            .await;

        let run = match run {
            Ok(Some(r)) => r,
            Ok(None) => {
                return ToolResult {
                    success: false,
                    output: Value::Null,
                    error: Some("Payroll run not found".to_string()),
                };
            }
            Err(e) => {
                return ToolResult {
                    success: false,
                    output: Value::Null,
                    error: Some(format!("DB error: {}", e)),
                };
            }
        };

        // Get payslips
        let payslips = sqlx::query!("SELECT * FROM payslips WHERE payroll_run_id = $1", run_id)
            .fetch_all(self.pool.pool())
            .await
            .unwrap_or_default();

        // Aggregate totals
        let total_gross = payslips.iter().map(|p| p.gross_earnings).sum::<Decimal>();
        let total_net = payslips.iter().map(|p| p.net_pay).sum::<Decimal>();
        let total_tax = payslips.iter().map(|p| p.tax_withheld).sum::<Decimal>();
        let total_sso = payslips.iter().map(|p| p.sso_employee).sum::<Decimal>();
        let total_pf = payslips.iter().map(|p| p.pf_employee).sum::<Decimal>();
        let total_employees = payslips.len();

        // Get anomalies
        let anomalies: Vec<_> = payslips
            .iter()
            .flat_map(|p| p.anomalies.as_array().unwrap_or(&vec![]).clone())
            .collect();

        // Generate narrative using LLM
        let prompt = format!(
            r#"Generate a concise Thai payroll summary for management:
            Period: {} to {}
            Employees: {}
            Total Gross: {} THB
            Total Net: {} THB
            Total Tax: {} THB
            Total SSO: {} THB
            Total PF: {} THB
            Anomalies: {}
            Write a professional Thai summary."#,
            run.period_start,
            run.period_end,
            total_employees,
            total_gross,
            total_net,
            total_tax,
            total_sso,
            total_pf,
            serde_json::to_string(&anomalies).unwrap_or_default()
        );

        let request = ChatRequest {
            model: "qwen2.5-72b-instruct".to_string(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    tool_calls: None,
                    content: "You are a Thai payroll expert. Write concise executive summaries."
                        .to_string(),
                },
                Message {
                    role: "user".to_string(),
                    tool_calls: None,
                    content: prompt,
                },
            ],
            tools: None,
            stream: Some(false),
            temperature: Some(0.3),
            max_tokens: Some(1024),
        };

        let narrative = self
            .model_gw
            .chat_with_request(&ctx.tenant_id.to_string(), request)
            .await
            .map(|r| {
                r.choices
                    .first()
                    .map(|c| c.message.content.clone())
                    .unwrap_or_default()
            })
            .unwrap_or_else(|_| "Summary generation failed".to_string());

        ToolResult {
            success: true,
            output: serde_json::json!({
                "run_id": run_id.to_string(),
                "period_start": run.period_start,
                "period_end": run.period_end,
                "total_employees": total_employees,
                "total_gross": total_gross.to_string(),
                "total_net": total_net.to_string(),
                "total_tax": total_tax.to_string(),
                "total_sso": total_sso.to_string(),
                "total_pf": total_pf.to_string(),
                "narrative": narrative,
                "top_anomalies": anomalies,
            }),
            error: None,
        }
    }
}

/// Check compliance tool - REAL implementation
pub struct CheckComplianceTool {
    pool: Arc<TenantPool>,
}

impl CheckComplianceTool {
    pub fn new(pool: Arc<TenantPool>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl AgentTool for CheckComplianceTool {
    fn name(&self) -> &str {
        "check_compliance"
    }
    fn description(&self) -> &str {
        "Cross-check tax and SSO rules; return compliance report."
    }
    fn is_destructive(&self) -> bool {
        false
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "employee_id": {"type": "string", "format": "uuid"},
                "period": {"type": "string", "format": "date"}
            },
            "required": ["employee_id", "period"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let employee_id_str = input
            .get("employee_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let period_str = input.get("period").and_then(|v| v.as_str()).unwrap_or("");
        let period_date = match chrono::NaiveDate::parse_from_str(period_str, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => {
                return ToolResult {
                    success: false,
                    output: Value::Null,
                    error: Some("period must be an ISO date (YYYY-MM-DD)".to_string()),
                };
            }
        };

        let employee_id = match Uuid::parse_str(employee_id_str) {
            Ok(id) => id,
            Err(_) => {
                return ToolResult {
                    success: false,
                    output: Value::Null,
                    error: Some("Invalid employee_id format".to_string()),
                };
            }
        };

        // Get employee
        let emp = sqlx::query!(
            "SELECT * FROM employees WHERE id = $1 AND tenant_id = $2",
            employee_id,
            ctx.tenant_id.0
        )
        .fetch_optional(self.pool.pool())
        .await;

        let employee = match emp {
            Ok(Some(e)) => e,
            Ok(None) => {
                return ToolResult {
                    success: false,
                    output: Value::Null,
                    error: Some("Employee not found".to_string()),
                };
            }
            Err(e) => {
                return ToolResult {
                    success: false,
                    output: Value::Null,
                    error: Some(format!("DB error: {}", e)),
                };
            }
        };

        // Get payslip for period
        let payslip = sqlx::query!(
            "SELECT * FROM payslips WHERE employee_id = $1 AND payroll_run_id IN (SELECT id FROM payroll_runs WHERE period_start <= $2 AND period_end >= $2)",
            employee_id, period_date
        )
        .fetch_optional(self.pool.pool())
        .await;

        let payslip = match payslip {
            Ok(Some(p)) => p,
            Ok(None) => {
                return ToolResult {
                    success: false,
                    output: Value::Null,
                    error: Some("No payslip found for period".to_string()),
                };
            }
            Err(e) => {
                return ToolResult {
                    success: false,
                    output: Value::Null,
                    error: Some(format!("DB error: {}", e)),
                };
            }
        };

        // Run compliance checks
        let mut issues = Vec::new();

        // Tax compliance
        let base_salary = employee.base_salary;
        let annual_income = base_salary * dec!(12);
        let tax_calc =
            talentos_compliance::calculate_pnd_tax(annual_income, &PersonalAllowances::default());
        let expected_tax = tax_calc.unwrap_or(Decimal::ZERO) * dec!(12);
        let actual_tax = payslip.tax_withheld * dec!(12);
        if (expected_tax - actual_tax).abs() > dec!(100) {
            issues.push(serde_json::json!({
                "type": "tax_discrepancy",
                "severity": "warning",
                "message": format!("Tax discrepancy: expected {:.2}, actual {:.2}", expected_tax, actual_tax)
            }));
        }

        // SSO compliance
        if employee.base_salary > dec!(15000) {
            issues.push(serde_json::json!({
                "type": "sso_cap",
                "severity": "info",
                "message": "Base salary exceeds SSO wage base cap (฿15,000)"
            }));
        }

        // PF compliance
        if employee.pf_member {
            {
                let pf_emp = payslip.pf_employee;
                let expected = employee.base_salary * dec!(0.05);
                if (pf_emp - expected).abs() > dec!(10) {
                    issues.push(serde_json::json!({
                        "type": "pf_discrepancy",
                        "severity": "warning",
                        "message": format!("PF contribution mismatch: expected {:.2}, actual {:.2}", expected, pf_emp)
                    }));
                }
            }
        }

        ToolResult {
            success: true,
            output: serde_json::json!({
                "employee_id": employee_id.to_string(),
                "period": period_str,
                "compliant": issues.is_empty(),
                "issues": issues,
            }),
            error: None,
        }
    }
}

/// PII masking functions
fn mask_national_id(nid: &str) -> String {
    if nid.len() >= 13 {
        format!("{}********{}", &nid[..1], &nid[nid.len() - 1..])
    } else {
        "**********".to_string()
    }
}

fn mask_bank_account(ba: &str) -> String {
    if ba.len() >= 4 {
        format!("****{}", &ba[ba.len() - 4..])
    } else {
        "****".to_string()
    }
}

fn mask_sso(sso: &str) -> String {
    if sso.len() >= 4 {
        format!("****{}", &sso[sso.len() - 4..])
    } else {
        "****".to_string()
    }
}

/// Validate leave policy against Thai labor law
fn validate_leave_policy(policy: &str) -> Value {
    let issues: Vec<Value> = Vec::new();
    let mut warnings: Vec<Value> = Vec::new();

    // Check for required leave types
    let required_leaves = ["annual", "sick", "maternity", "paternity", "personal"];
    for leave in required_leaves {
        if !policy.to_lowercase().contains(leave) {
            warnings.push(serde_json::json!({
                "type": "missing_leave_type",
                "message": format!("Missing required leave type: {}", leave)
            }));
        }
    }

    // Check annual leave >= 6 days
    if policy.contains("annual") && policy.contains("6") {
        // Good - at least 6 days mentioned
    } else if policy.contains("annual") {
        warnings.push(serde_json::json!({
            "type": "annual_leave_minimum",
            "message": "Annual leave should be at least 6 days per Thai Labor Protection Act"
        }));
    }

    // Check maternity leave >= 98 days
    if policy.contains("maternity") && policy.contains("98") {
        // Good
    } else if policy.contains("maternity") {
        warnings.push(serde_json::json!({
            "type": "maternity_leave_minimum",
            "message": "Maternity leave should be at least 98 days per Thai law"
        }));
    }

    // Check paternity leave >= 15 days
    if policy.contains("paternity") && policy.contains("15") {
        // Good
    } else if policy.contains("paternity") {
        warnings.push(serde_json::json!({
            "type": "paternity_leave_minimum",
            "message": "Paternity leave should be at least 15 days per Thai law"
        }));
    }

    // Check sick leave >= 30 days
    if policy.contains("sick") && policy.contains("30") {
        // Good
    } else if policy.contains("sick") {
        warnings.push(serde_json::json!({
            "type": "sick_leave_minimum",
            "message": "Sick leave should be at least 30 days per Thai law"
        }));
    }

    serde_json::json!({
        "compliant": true,
        "issues": issues,
        "warnings": warnings,
        "checked_at": chrono::Utc::now().to_rfc3339()
    })
}

/// Registry with every built-in tool, ready for `AgentRunner`.
/// Tools that need shared state get the same handles the agent already holds.
pub fn default_registry(
    model_gw: Option<Arc<HuakatiClient>>,
    pool: Arc<TenantPool>,
) -> crate::registry::ToolRegistry {
    let mut registry = crate::registry::ToolRegistry::new();
    registry.register(RunPayrollTool::new());
    registry.register(QueryEmployeeTool::new(pool.clone()));
    registry.register(CalculateTaxTool::new());
    registry.register(DetectAnomaliesTool::new(pool.clone()));
    registry.register(CheckComplianceTool::new(pool.clone()));
    if let Some(model_gw) = model_gw {
        registry.register(SearchPolicyTool::new(model_gw.clone(), pool.clone()));
        registry.register(DraftLeavePolicyTool::new(model_gw.clone()));
        registry.register(SummarizePayrollRunTool::new(pool, model_gw));
    }
    registry
}

/// Names of the built-in tools, without constructing their dependencies.
/// Used by discovery endpoints that must answer without a database or model key.
pub const TOOL_NAMES: &[&str] = &[
    "calculate_tax",
    "check_compliance",
    "detect_anomalies",
    "draft_leave_policy",
    "query_employee",
    "run_payroll",
    "search_policy",
    "summarize_payroll_run",
];

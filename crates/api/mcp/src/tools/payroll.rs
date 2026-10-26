//! Payroll tools for MCP.

use rmcp::{
    handler::server::tool::ToolCallContext,
    model::{CallToolResult, ContentBlock, ToolAnnotations},
    ErrorData,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use talents_db::TenantPool;
use talents_kernel::TenantContext;
use crate::auth::{extract_from_request_context, to_error_data};
use crate::error::{mcp_error, codes, mcp_error_with_details};
use std::collections::HashMap;
use crate::tools::{tool_def, read_only_annotation, destructive_annotation, route_dyn};

/// Parameters for run_payroll.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunPayrollParams {
    /// Payroll period start date (YYYY-MM-DD)
    pub period_start: chrono::NaiveDate,
    /// Payroll period end date (YYYY-MM-DD)
    pub period_end: chrono::NaiveDate,
    /// Payment date (YYYY-MM-DD)
    pub payment_date: chrono::NaiveDate,
    /// Entity ID to run payroll for (optional, defaults to all entities in tenant)
    #[serde(default)]
    pub entity_id: Option<uuid::Uuid>,
    /// Explicit confirmation required for irreversible action
    pub confirm: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PayrollRunResult {
    pub run_id: uuid::Uuid,
    pub tenant_id: uuid::Uuid,
    pub entity_id: Option<uuid::Uuid>,
    pub period_start: chrono::NaiveDate,
    pub period_end: chrono::NaiveDate,
    pub payment_date: chrono::NaiveDate,
    pub status: String,
    pub total_employees: i32,
    pub total_gross: String,
    pub total_net: String,
    pub total_tax: String,
    pub total_sso: String,
    pub total_pf: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub fn run_payroll() -> crate::tools::ToolRoute<crate::McpServer> {
    let tool = tool_def::<RunPayrollParams>(
        "run_payroll",
        "Create and start a new payroll run for the given period. IRREVERSIBLE — requires explicit confirm=true. Returns the run_id for status polling.",
    ).annotate(destructive_annotation());

    route_dyn(tool, move |context: ToolCallContext<'_, crate::McpServer>| {
        Box::pin(async move {
            let tenant_ctx = match extract_from_request_context(context.request_context()) {
                Ok(ctx) => ctx,
                Err(e) => return Err(to_error_data(e)),
            };

            let args: RunPayrollParams = match context
                .arguments
                .and_then(|a| serde_json::from_value::<RunPayrollParams>(serde_json::Value::Object(a)).ok())
            {
                Some(a) => a,
                None => return Err(mcp_error(codes::VALIDATION_ERROR, "invalid arguments")),
            };

            if !args.confirm {
                return Err(mcp_error_with_details(codes::MISSING_CONFIRMATION, "payroll run requires explicit confirm=true", {
                    let mut m = HashMap::new();
                    m.insert("error_code".to_string(), serde_json::json!("MISSING_CONFIRMATION"));
                    m
                }));
            }

            let pool = &context.service.pool;

            let run = pool
                .with_tenant_tx(&tenant_ctx, |tx| {
                    Box::pin(async move {
                        let row = sqlx::query_as!(
                            PayrollRunRow,
                            r#"
                            INSERT INTO payroll_runs (tenant_id, entity_id, period_start, period_end, payment_date, status, created_by)
                            VALUES ($1, $2, $3, $4, $5, 'draft', $6)
                            RETURNING id, tenant_id, entity_id, period_start, period_end, payment_date, status,
                                      total_employees, total_gross, total_net, total_tax, total_sso, total_pf, created_at
                            "#,
                            tenant_ctx.tenant_id.0,
                            args.entity_id,
                            args.period_start,
                            args.period_end,
                            args.payment_date,
                            tenant_ctx.user_id.0
                        )
                        .fetch_one(&mut *tx)
                        .await?;

                        Ok::<PayrollRunResult, sqlx::Error>(PayrollRunResult {
                            run_id: row.id,
                            tenant_id: row.tenant_id,
                            entity_id: row.entity_id,
                            period_start: row.period_start,
                            period_end: row.period_end,
                            payment_date: row.payment_date,
                            status: row.status,
                            total_employees: row.total_employees,
                            total_gross: row.total_gross,
                            total_net: row.total_net,
                            total_tax: row.total_tax,
                            total_sso: row.total_sso,
                            total_pf: row.total_pf,
                            created_at: row.created_at,
                        })
                    })
                })
                .await
                .map_err(|e| mcp_error(codes::INTERNAL_ERROR, &format!("db: {}", e)))?;

            let json = serde_json::to_value(run).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?;
            Ok(CallToolResult::success(vec![ContentBlock::json(json).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?]).into())
        })
    })
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetPayrollStatusParams {
    pub run_id: uuid::Uuid,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PayrollStatus {
    pub run_id: uuid::Uuid,
    pub tenant_id: uuid::Uuid,
    pub entity_id: Option<uuid::Uuid>,
    pub period_start: chrono::NaiveDate,
    pub period_end: chrono::NaiveDate,
    pub payment_date: chrono::NaiveDate,
    pub status: String,
    pub total_employees: i32,
    pub total_gross: String,
    pub total_net: String,
    pub total_tax: String,
    pub total_sso: String,
    pub total_pf: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub fn get_payroll_status() -> crate::tools::ToolRoute<crate::McpServer> {
    let tool = tool_def::<GetPayrollStatusParams>(
        "get_payroll_status",
        "Get the current status and summary of a payroll run by run_id.",
    ).annotate(read_only_annotation());

    route_dyn(tool, move |context: ToolCallContext<'_, crate::McpServer>| {
        Box::pin(async move {
            let tenant_ctx = match extract_from_request_context(context.request_context()) {
                Ok(ctx) => ctx,
                Err(e) => return Err(to_error_data(e)),
            };

            let args: GetPayrollStatusParams = match context
                .arguments
                .and_then(|a| serde_json::from_value::<GetPayrollStatusParams>(serde_json::Value::Object(a)).ok())
            {
                Some(a) => a,
                None => return Err(mcp_error(codes::VALIDATION_ERROR, "invalid arguments")),
            };

            let pool = &context.service.pool;

            let status = pool
                .with_tenant_tx(&tenant_ctx, |tx| {
                    Box::pin(async move {
                        let row = sqlx::query_as!(
                            PayrollRunRow,
                            r#"
                            SELECT id, tenant_id, entity_id, period_start, period_end, payment_date,
                                   status, total_employees, total_gross, total_net, total_tax,
                                   total_sso, total_pf, created_at, updated_at
                            FROM payroll_runs
                            WHERE id = $1
                            "#,
                            args.run_id
                        )
                        .fetch_optional(&mut *tx)
                        .await?;

                        Ok::<Option<PayrollStatus>, sqlx::Error>(row.map(|r| PayrollStatus {
                            run_id: r.id,
                            tenant_id: r.tenant_id,
                            entity_id: r.entity_id,
                            period_start: r.period_start,
                            period_end: r.period_end,
                            payment_date: r.payment_date,
                            status: r.status,
                            total_employees: r.total_employees,
                            total_gross: r.total_gross,
                            total_net: r.total_net,
                            total_tax: r.total_tax,
                            total_sso: r.total_sso,
                            total_pf: r.total_pf,
                            created_at: r.created_at,
                            updated_at: r.updated_at,
                        }))
                    })
                })
                .await
                .map_err(|e| ErrorData::internal_error(format!("db: {}", e), None))?;

            match status {
                Some(s) => {
                    let json = serde_json::to_value(s).map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                    Ok(CallToolResult::success(vec![ContentBlock::json(json).map_err(|e| ErrorData::internal_error(e.to_string(), None))?]).into())
                }
                None => Err(mcp_error(codes::NOT_FOUND, "payroll run not found")),
            }
        })
    })
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ApprovePayrollParams {
    pub run_id: uuid::Uuid,
    /// Explicit confirmation required for irreversible approval
    pub confirm: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ApprovePayrollResult {
    pub run_id: uuid::Uuid,
    pub status: String,
    pub approved_at: chrono::DateTime<chrono::Utc>,
    pub approved_by: uuid::Uuid,
}

pub fn approve_payroll() -> crate::tools::ToolRoute<crate::McpServer> {
    let tool = tool_def::<ApprovePayrollParams>(
        "approve_payroll",
        "Approve a draft/pending_approval payroll run. IRREVERSIBLE — requires explicit confirm=true.",
    ).annotate(destructive_annotation());

    route_dyn(tool, move |context: ToolCallContext<'_, crate::McpServer>| {
        Box::pin(async move {
            let tenant_ctx = match extract_from_request_context(context.request_context()) {
                Ok(ctx) => ctx,
                Err(e) => return Err(to_error_data(e)),
            };

            let args: ApprovePayrollParams = match context
                .arguments
                .and_then(|a| serde_json::from_value::<ApprovePayrollParams>(serde_json::Value::Object(a)).ok())
            {
                Some(a) => a,
                None => return Err(mcp_error(codes::VALIDATION_ERROR, "invalid arguments")),
            };

            if !args.confirm {
                return Err(mcp_error_with_details(codes::MISSING_CONFIRMATION, "approval requires explicit confirm=true", {
                    let mut m = HashMap::new();
                    m.insert("error_code".to_string(), serde_json::json!("MISSING_CONFIRMATION"));
                    m
                }));
            }

            let pool = &context.service.pool;

            let result = pool
                .with_tenant_tx(&tenant_ctx, |tx| {
                    Box::pin(async move {
                        let row = sqlx::query_as!(
                            ApprovePayrollRow,
                            r#"
                            UPDATE payroll_runs
                            SET status = 'approved', updated_at = now()
                            WHERE id = $1 AND status IN ('draft', 'pending_approval')
                            RETURNING id, status, updated_at
                            "#,
                            args.run_id
                        )
                        .fetch_optional(&mut *tx)
                        .await?;

                        Ok::<Option<ApprovePayrollResult>, sqlx::Error>(row.map(|r| ApprovePayrollResult {
                            run_id: r.id,
                            status: r.status,
                            approved_at: r.updated_at,
                            approved_by: tenant_ctx.user_id.0,
                        }))
                    })
                })
                .await
                .map_err(|e| ErrorData::internal_error(format!("db: {}", e), None))?;

            match result {
                Some(r) => {
                    let json = serde_json::to_value(r).map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                    Ok(CallToolResult::success(vec![ContentBlock::json(json).map_err(|e| ErrorData::internal_error(e.to_string(), None))?]).into())
                }
                None => Err(ErrorData::invalid_params(
                    "payroll run not found or not in approvable state (must be draft or pending_approval)",
                    Some(serde_json::json!({"error_code": "INVALID_STATE"})),
                )),
            }
        })
    })
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ListPayslipsParams {
    pub run_id: uuid::Uuid,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 { 50 }

#[derive(Debug, Serialize, JsonSchema)]
pub struct PayslipSummary {
    pub id: uuid::Uuid,
    pub employee_id: uuid::Uuid,
    pub base_salary: String,
    pub gross_earnings: String,
    pub total_deductions: String,
    pub net_pay: String,
    pub tax_withheld: String,
    pub sso_employee: String,
    pub sso_employer: String,
    pub pf_employee: String,
    pub pf_employer: String,
    pub status: String,
}

pub fn list_payslips() -> crate::tools::ToolRoute<crate::McpServer> {
    let tool = tool_def::<ListPayslipsParams>(
        "list_payslips",
        "List payslips for a given payroll run. Returns paginated summary data.",
    ).annotate(read_only_annotation());

    route_dyn(tool, move |context: ToolCallContext<'_, crate::McpServer>| {
        Box::pin(async move {
            let tenant_ctx = match extract_from_request_context(context.request_context()) {
                Ok(ctx) => ctx,
                Err(e) => return Err(to_error_data(e)),
            };

            let args: ListPayslipsParams = match context
                .arguments
                .and_then(|a| serde_json::from_value::<ListPayslipsParams>(serde_json::Value::Object(a)).ok())
            {
                Some(a) => a,
                None => return Err(mcp_error(codes::VALIDATION_ERROR, "invalid arguments")),
            };

            let limit = args.limit.min(200) as i64;
            let offset = args.offset as i64;

            let pool = &context.service.pool;

            let rows = pool
                .with_tenant_tx(&tenant_ctx, |tx| {
                    Box::pin(async move {
                        let rows = sqlx::query_as!(
                            PayslipRow,
                            r#"
                            SELECT id, employee_id, base_salary, gross_earnings, total_deductions,
                                   net_pay, tax_withheld, sso_employee, sso_employer,
                                   pf_employee, pf_employer, status
                            FROM payslips
                            WHERE payroll_run_id = $1
                            ORDER BY employee_id
                            LIMIT $2 OFFSET $3
                            "#,
                            args.run_id,
                            limit,
                            offset
                        )
                        .fetch_all(&mut *tx)
                        .await?;

                        Ok::<Vec<PayslipSummary>, sqlx::Error>(rows
                            .into_iter()
                            .map(|r| PayslipSummary {
                                id: r.id,
                                employee_id: r.employee_id,
                                base_salary: r.base_salary,
                                gross_earnings: r.gross_earnings,
                                total_deductions: r.total_deductions,
                                net_pay: r.net_pay,
                                tax_withheld: r.tax_withheld,
                                sso_employee: r.sso_employee,
                                sso_employer: r.sso_employer,
                                pf_employee: r.pf_employee,
                                pf_employer: r.pf_employer,
                                status: r.status,
                            })
                            .collect())
                    })
                })
                .await
                .map_err(|e| ErrorData::internal_error(format!("db: {}", e), None))?;

            let json = serde_json::to_value(rows).map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            Ok(CallToolResult::success(vec![ContentBlock::json(json).map_err(|e| ErrorData::internal_error(e.to_string(), None))?]).into())
        })
    })
}

// Row structs for type-safe query_as!
struct PayrollRunRow {
    id: uuid::Uuid,
    tenant_id: uuid::Uuid,
    entity_id: Option<uuid::Uuid>,
    period_start: chrono::NaiveDate,
    period_end: chrono::NaiveDate,
    payment_date: chrono::NaiveDate,
    status: String,
    total_employees: i32,
    total_gross: String,
    total_net: String,
    total_tax: String,
    total_sso: String,
    total_pf: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

struct ApprovePayrollRow {
    id: uuid::Uuid,
    status: String,
    updated_at: chrono::DateTime<chrono::Utc>,
}

struct PayslipRow {
    id: uuid::Uuid,
    employee_id: uuid::Uuid,
    base_salary: String,
    gross_earnings: String,
    total_deductions: String,
    net_pay: String,
    tax_withheld: String,
    sso_employee: String,
    sso_employer: String,
    pf_employee: String,
    pf_employer: String,
    status: String,
}

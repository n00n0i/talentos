//! Employee query tools for MCP.

use rmcp::{
    handler::server::tool::ToolCallContext,
    model::{CallToolResult, ContentBlock, ToolAnnotations},
    service::RequestContext,
    ErrorData,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use talents_db::TenantPool;
use talents_kernel::{TenantContext, TenantId};
use crate::auth::{extract_from_request_context, to_error_data};
use crate::error::{mcp_error, codes};
use crate::tools::{tool_def, read_only_annotation, route_dyn};

/// Parameters for query_employees.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct QueryEmployeesParams {
    /// Filter by department name (partial match)
    #[serde(default)]
    pub department: Option<String>,
    /// Filter by employment status (active, probation, leave, terminated)
    #[serde(default)]
    pub status: Option<String>,
    /// Maximum results to return (default 50, max 200)
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Offset for pagination (default 0)
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 { 50 }

/// Single employee detail returned by get_employee_detail.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EmployeeDetail {
    pub id: uuid::Uuid,
    pub employee_code: String,
    pub first_name_th: String,
    pub last_name_th: String,
    pub first_name_en: Option<String>,
    pub last_name_en: Option<String>,
    /// Masked — only last 3 digits visible
    pub national_id: Option<String>,
    pub birth_date: Option<chrono::NaiveDate>,
    pub hire_date: chrono::NaiveDate,
    pub termination_date: Option<chrono::NaiveDate>,
    pub position_title: String,
    pub department: Option<String>,
    pub base_salary: String, // Money serializes as string
    /// Masked — only last 4 digits visible
    pub bank_account_number: Option<String>,
    pub bank_name: Option<String>,
    pub tax_filing_status: i16,
    /// Masked — only last 3 digits visible
    pub sso_number: Option<String>,
    pub pf_member: bool,
    pub status: String,
    pub entity_id: uuid::Uuid,
}

/// List item for query_employees results.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EmployeeListItem {
    pub id: uuid::Uuid,
    pub employee_code: String,
    pub first_name_th: String,
    pub last_name_th: String,
    pub position_title: String,
    pub department: Option<String>,
    pub base_salary: String,
    pub status: String,
    pub entity_id: uuid::Uuid,
}

// PII masking functions
fn mask_national_id(v: Option<String>) -> Option<String> {
    v.map(|s| {
        if s.len() <= 3 {
            "***".into()
        } else {
            format!("***{}", &s[s.len() - 3..])
        }
    })
}

fn mask_bank_account(v: Option<String>) -> Option<String> {
    v.map(|s| {
        if s.len() <= 4 {
            "****".into()
        } else {
            format!("****{}", &s[s.len() - 4..])
        }
    })
}

fn mask_sso(v: Option<String>) -> Option<String> {
    v.map(|s| {
        if s.len() <= 3 {
            "***".into()
        } else {
            format!("***{}", &s[s.len() - 3..])
        }
    })
}

pub fn query_employees() -> crate::tools::ToolRoute<crate::McpServer> {
    let tool = tool_def::<QueryEmployeesParams>(
        "query_employees",
        "Search employees within the current tenant. Returns paginated results with basic profile info.",
    ).annotate(read_only_annotation());

    route_dyn(tool, move |context: ToolCallContext<'_, crate::McpServer>| {
        Box::pin(async move {
            let tenant_ctx = match extract_from_request_context(context.request_context()) {
                Ok(ctx) => ctx,
                Err(e) => return Err(to_error_data(e)),
            };

            let args: QueryEmployeesParams = match context
                .arguments
                .and_then(|a| serde_json::from_value::<QueryEmployeesParams>(serde_json::Value::Object(a)).ok())
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
                        // Use parameterized query with optional filters
                        let rows = sqlx::query_as!(
                            EmployeeRow,
                            r#"
                            SELECT id, employee_code, first_name_th, last_name_th, position_title,
                                   department, base_salary, status, entity_id
                            FROM employees
                            WHERE ($1::text IS NULL OR department ILIKE '%' || $1 || '%')
                              AND ($2::text IS NULL OR status = $2)
                            ORDER BY hire_date DESC
                            LIMIT $3 OFFSET $4
                            "#,
                            args.department,
                            args.status,
                            limit,
                            offset
                        )
                        .fetch_all(&mut *tx)
                        .await?;

                        Ok::<Vec<EmployeeListItem>, sqlx::Error>(rows
                            .into_iter()
                            .map(|r| EmployeeListItem {
                                id: r.id,
                                employee_code: r.employee_code,
                                first_name_th: r.first_name_th,
                                last_name_th: r.last_name_th,
                                position_title: r.position_title,
                                department: r.department,
                                base_salary: r.base_salary,
                                status: r.status,
                                entity_id: r.entity_id,
                            })
                            .collect())
                    })
                })
                .await
                .map_err(|e| mcp_error(codes::INTERNAL_ERROR, &format!("db: {}", e)))?;

            let json = serde_json::to_value(rows).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?;
            Ok(CallToolResult::success(vec![ContentBlock::json(json).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?]).into())
        })
    })
}

pub fn get_employee_detail() -> crate::tools::ToolRoute<crate::McpServer> {
    #[derive(Debug, Deserialize, Serialize, JsonSchema)]
    struct GetEmployeeDetailParams {
        /// Employee UUID
        pub employee_id: uuid::Uuid,
    }

    let tool = tool_def::<GetEmployeeDetailParams>(
        "get_employee_detail",
        "Get full detail for a single employee by ID. PII fields are masked based on caller role.",
    ).annotate(read_only_annotation());

    route_dyn(tool, move |context: ToolCallContext<'_, crate::McpServer>| {
        Box::pin(async move {
            let tenant_ctx = match extract_from_request_context(context.request_context()) {
                Ok(ctx) => ctx,
                Err(e) => return Err(to_error_data(e)),
            };

            let args: GetEmployeeDetailParams = match context
                .arguments
                .and_then(|a| serde_json::from_value::<GetEmployeeDetailParams>(serde_json::Value::Object(a)).ok())
            {
                Some(a) => a,
                None => return Err(mcp_error(codes::VALIDATION_ERROR, "invalid arguments")),
            };

            let pool = &context.service.pool;

            let employee = pool
                .with_tenant_tx(&tenant_ctx, |tx| {
                    Box::pin(async move {
                        let row = sqlx::query_as!(
                            EmployeeDetailRow,
                            r#"
                            SELECT id, employee_code, first_name_th, last_name_th,
                                   first_name_en, last_name_en, national_id, birth_date,
                                   hire_date, termination_date, position_title, department,
                                   base_salary, bank_account_number, bank_name,
                                   tax_filing_status, sso_number, pf_member, status, entity_id
                            FROM employees
                            WHERE id = $1
                            "#,
                            args.employee_id
                        )
                        .fetch_optional(&mut *tx)
                        .await?;

                        Ok::<Option<EmployeeDetail>, sqlx::Error>(row.map(|r| EmployeeDetail {
                            id: r.id,
                            employee_code: r.employee_code,
                            first_name_th: r.first_name_th,
                            last_name_th: r.last_name_th,
                            first_name_en: r.first_name_en,
                            last_name_en: r.last_name_en,
                            national_id: mask_national_id(r.national_id),
                            birth_date: r.birth_date,
                            hire_date: r.hire_date,
                            termination_date: r.termination_date,
                            position_title: r.position_title,
                            department: r.department,
                            base_salary: r.base_salary,
                            bank_account_number: mask_bank_account(r.bank_account_number),
                            bank_name: r.bank_name,
                            tax_filing_status: r.tax_filing_status as i16,
                            sso_number: mask_sso(r.sso_number),
                            pf_member: r.pf_member,
                            status: r.status,
                            entity_id: r.entity_id,
                        }))
                    })
                })
                .await
                .map_err(|e| mcp_error(codes::INTERNAL_ERROR, &format!("db: {}", e)))?;

            match employee {
                Some(emp) => {
                    let json = serde_json::to_value(emp).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?;
                    Ok(CallToolResult::success(vec![ContentBlock::json(json).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?]).into())
                }
                None => Err(mcp_error(codes::NOT_FOUND, "employee not found")),
            }
        })
    })
}

// Row structs for type-safe query_as!
struct EmployeeRow {
    id: uuid::Uuid,
    employee_code: String,
    first_name_th: String,
    last_name_th: String,
    position_title: String,
    department: Option<String>,
    base_salary: String,
    status: String,
    entity_id: uuid::Uuid,
}

struct EmployeeDetailRow {
    id: uuid::Uuid,
    employee_code: String,
    first_name_th: String,
    last_name_th: String,
    first_name_en: Option<String>,
    last_name_en: Option<String>,
    national_id: Option<String>,
    birth_date: Option<chrono::NaiveDate>,
    hire_date: chrono::NaiveDate,
    termination_date: Option<chrono::NaiveDate>,
    position_title: String,
    department: Option<String>,
    base_salary: String,
    bank_account_number: Option<String>,
    bank_name: Option<String>,
    tax_filing_status: i32,
    sso_number: Option<String>,
    pf_member: bool,
    status: String,
    entity_id: uuid::Uuid,
}

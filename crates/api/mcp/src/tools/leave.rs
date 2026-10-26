//! Leave management tools for MCP.

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
use crate::tools::{tool_def, read_only_annotation, destructive_annotation, route_dyn};
use std::collections::HashMap;

/// Parameters for request_leave.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RequestLeaveParams {
    /// Employee UUID
    pub employee_id: uuid::Uuid,
    /// Leave type (annual, sick, maternity, paternity, unpaid, other)
    pub leave_type: String,
    /// Start date (YYYY-MM-DD)
    pub start_date: chrono::NaiveDate,
    /// End date (YYYY-MM-DD)
    pub end_date: chrono::NaiveDate,
    /// Reason/notes
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LeaveRequestResult {
    pub request_id: uuid::Uuid,
    pub employee_id: uuid::Uuid,
    pub leave_type: String,
    pub start_date: chrono::NaiveDate,
    pub end_date: chrono::NaiveDate,
    pub status: String,
    pub requested_at: chrono::DateTime<chrono::Utc>,
}

pub fn request_leave() -> crate::tools::ToolRoute<crate::McpServer> {
    let tool = tool_def::<RequestLeaveParams>(
        "request_leave",
        "Submit a new leave request for an employee. Creates a pending request for manager approval.",
    ).annotate(destructive_annotation());

    route_dyn(tool, move |context: ToolCallContext<'_, crate::McpServer>| {
        Box::pin(async move {
            let tenant_ctx = match extract_from_request_context(context.request_context()) {
                Ok(ctx) => ctx,
                Err(e) => return Err(to_error_data(e)),
            };

            let args: RequestLeaveParams = match context
                .arguments
                .and_then(|a| serde_json::from_value::<RequestLeaveParams>(serde_json::Value::Object(a)).ok())
            {
                Some(a) => a,
                None => return Err(mcp_error(codes::VALIDATION_ERROR, "invalid arguments")),
            };

            if args.end_date < args.start_date {
                return Err(mcp_error_with_details(codes::INVALID_DATE_RANGE, "end_date must be on or after start_date", {
                    let mut m = HashMap::new();
                    m.insert("error_code".to_string(), serde_json::json!("INVALID_DATE_RANGE"));
                    m
                }));
            }

            let pool = &context.service.pool;

            let result = pool
                .with_tenant_tx(&tenant_ctx, |tx| {
                    Box::pin(async move {
                        // Validate employee exists first
                        let employee = sqlx::query!(
                            "SELECT entity_id FROM employees WHERE id = $1",
                            args.employee_id
                        )
                        .fetch_optional(&mut *tx)
                        .await?;

                        let entity_id = employee
                            .ok_or(sqlx::Error::RowNotFound)?
                            .entity_id;

                        let days = (args.end_date - args.start_date).num_days() as i64 + 1;

                        let row = sqlx::query_as!(
                            LeaveRequestRow,
                            r#"
                            INSERT INTO leave_requests (tenant_id, entity_id, employee_id, leave_type, start_date, end_date, days, reason, status, requested_by)
                            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'pending', $9)
                            RETURNING id, employee_id, leave_type, start_date, end_date, days, reason, status, requested_by, created_at
                            "#,
                            tenant_ctx.tenant_id.0,
                            entity_id,
                            args.employee_id,
                            args.leave_type,
                            args.start_date,
                            args.end_date,
                            days,
                            args.reason,
                            tenant_ctx.user_id.0
                        )
                        .fetch_one(&mut *tx)
                        .await?;

                        Ok::<LeaveRequestResult, sqlx::Error>(LeaveRequestResult {
                            request_id: row.id,
                            employee_id: row.employee_id,
                            leave_type: row.leave_type,
                            start_date: row.start_date,
                            end_date: row.end_date,
                            status: row.status,
                            requested_at: row.created_at,
                        })
                    })
                })
                .await
                .map_err(|e| mcp_error(codes::INTERNAL_ERROR, &format!("db: {}", e)))?;

            let json = serde_json::to_value(result).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?;
            Ok(CallToolResult::success(vec![ContentBlock::json(json).map_err(|e| mcp_error(codes::INTERNAL_ERROR, &e.to_string()))?]).into())
        })
    })
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CheckBalanceParams {
    /// Employee UUID
    pub employee_id: uuid::Uuid,
    /// Leave type to check (annual, sick, etc.) — if omitted, returns all types
    #[serde(default)]
    pub leave_type: Option<String>,
    /// As of date (defaults to today)
    #[serde(default)]
    pub as_of: Option<chrono::NaiveDate>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LeaveBalance {
    pub employee_id: uuid::Uuid,
    pub leave_type: String,
    pub accrued: f64,
    pub used: f64,
    pub balance: f64,
    pub as_of: chrono::NaiveDate,
}

pub fn check_balance() -> crate::tools::ToolRoute<crate::McpServer> {
    let tool = tool_def::<CheckBalanceParams>(
        "check_balance",
        "Check leave balance for an employee. Returns accrued, used, and remaining days by leave type.",
    ).annotate(read_only_annotation());

    route_dyn(tool, move |context: ToolCallContext<'_, crate::McpServer>| {
        Box::pin(async move {
            let tenant_ctx = match extract_from_request_context(context.request_context()) {
                Ok(ctx) => ctx,
                Err(e) => return Err(to_error_data(e)),
            };

            let args: CheckBalanceParams = match context
                .arguments
                .and_then(|a| serde_json::from_value::<CheckBalanceParams>(serde_json::Value::Object(a)).ok())
            {
                Some(a) => a,
                None => return Err(mcp_error(codes::VALIDATION_ERROR, "invalid arguments")),
            };

            let as_of = args.as_of.unwrap_or_else(chrono::Utc::now().date_naive);

            let pool = &context.service.pool;

            let balances = pool
                .with_tenant_tx(&tenant_ctx, |tx| {
                    Box::pin(async move {
                        let rows = sqlx::query_as!(
                            LeaveBalanceRow,
                            r#"
                            SELECT lt.name as leave_type,
                                   COALESCE(SUM(lb.accrued_days), 0) as accrued,
                                   COALESCE(SUM(lb.used_days), 0) as used
                            FROM leave_types lt
                            LEFT JOIN leave_balances lb ON lb.leave_type_id = lt.id AND lb.employee_id = $1
                            WHERE lt.tenant_id = $2
                              AND ($3::text IS NULL OR lt.name = $3)
                            GROUP BY lt.name
                            "#,
                            args.employee_id,
                            tenant_ctx.tenant_id.0,
                            args.leave_type
                        )
                        .fetch_all(&mut *tx)
                        .await?;

                        Ok::<Vec<LeaveBalance>, sqlx::Error>(rows
                            .into_iter()
                            .map(|r| LeaveBalance {
                                employee_id: args.employee_id,
                                leave_type: r.leave_type,
                                accrued: r.accrued,
                                used: r.used,
                                balance: r.accrued - r.used,
                                as_of,
                            })
                            .collect())
                    })
                })
                .await
                .map_err(|e| ErrorData::internal_error(format!("db: {}", e), None))?;

            let json = serde_json::to_value(balances).map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            Ok(CallToolResult::success(vec![ContentBlock::json(json).map_err(|e| ErrorData::internal_error(e.to_string(), None))?]).into())
        })
    })
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ListPendingApprovalsParams {
    /// Filter by department (optional)
    #[serde(default)]
    pub department: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 { 50 }

#[derive(Debug, Serialize, JsonSchema)]
pub struct PendingApproval {
    pub request_id: uuid::Uuid,
    pub employee_id: uuid::Uuid,
    pub employee_name: String,
    pub department: Option<String>,
    pub leave_type: String,
    pub start_date: chrono::NaiveDate,
    pub end_date: chrono::NaiveDate,
    pub days: f64,
    pub reason: Option<String>,
    pub requested_at: chrono::DateTime<chrono::Utc>,
}

pub fn list_pending_approvals() -> crate::tools::ToolRoute<crate::McpServer> {
    let tool = tool_def::<ListPendingApprovalsParams>(
        "list_pending_approvals",
        "List all leave requests pending approval for the current user's team/department.",
    ).annotate(read_only_annotation());

    route_dyn(tool, move |context: ToolCallContext<'_, crate::McpServer>| {
        Box::pin(async move {
            let tenant_ctx = match extract_from_request_context(context.request_context()) {
                Ok(ctx) => ctx,
                Err(e) => return Err(to_error_data(e)),
            };

            // Require entity_id for team-scoped queries
            let entity_id = match tenant_ctx.entity_id {
                Some(eid) => eid,
                None => return Err(mcp_error_with_details(codes::MISSING_ENTITY_ID, "entity_id required for team-scoped query", {
                    let mut m = HashMap::new();
                    m.insert("error_code".to_string(), serde_json::json!("MISSING_ENTITY_ID"));
                    m
                })),
            };

            let args: ListPendingApprovalsParams = match context
                .arguments
                .and_then(|a| serde_json::from_value::<ListPendingApprovalsParams>(serde_json::Value::Object(a)).ok())
            {
                Some(a) => a,
                None => return Err(ErrorData::invalid_params("invalid arguments", None)),
            };

            let limit = args.limit.min(200) as i64;
            let offset = args.offset as i64;

            let pool = &context.service.pool;

            let rows = pool
                .with_tenant_tx(&tenant_ctx, |tx| {
                    Box::pin(async move {
                        let rows = sqlx::query_as!(
                            PendingApprovalRow,
                            r#"
                            SELECT lr.id as request_id, lr.employee_id,
                                   e.first_name_th || ' ' || e.last_name_th as employee_name,
                                   e.department, lr.leave_type, lr.start_date, lr.end_date,
                                   lr.reason, lr.created_at as requested_at
                            FROM leave_requests lr
                            JOIN employees e ON e.id = lr.employee_id
                            WHERE lr.status = 'pending'
                              AND lr.tenant_id = $1
                              AND e.entity_id = $2
                              AND ($3::text IS NULL OR e.department ILIKE '%' || $3 || '%')
                            ORDER BY lr.created_at
                            LIMIT $4 OFFSET $5
                            "#,
                            tenant_ctx.tenant_id.0,
                            entity_id.0,
                            args.department,
                            limit,
                            offset
                        )
                        .fetch_all(&mut *tx)
                        .await?;

                        Ok::<Vec<PendingApproval>, sqlx::Error>(rows
                            .into_iter()
                            .map(|r| {
                                let start: chrono::NaiveDate = r.start_date;
                                let end: chrono::NaiveDate = r.end_date;
                                let days = (end - start).num_days() as f64 + 1.0;
                                PendingApproval {
                                    request_id: r.request_id,
                                    employee_id: r.employee_id,
                                    employee_name: r.employee_name,
                                    department: r.department,
                                    leave_type: r.leave_type,
                                    start_date: start,
                                    end_date: end,
                                    days,
                                    reason: r.reason,
                                    requested_at: r.requested_at,
                                }
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
struct LeaveRequestRow {
    id: uuid::Uuid,
    employee_id: uuid::Uuid,
    leave_type: String,
    start_date: chrono::NaiveDate,
    end_date: chrono::NaiveDate,
    days: i64,
    reason: Option<String>,
    status: String,
    requested_by: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
}

struct LeaveBalanceRow {
    leave_type: String,
    accrued: f64,
    used: f64,
}

struct PendingApprovalRow {
    request_id: uuid::Uuid,
    employee_id: uuid::Uuid,
    employee_name: String,
    department: Option<String>,
    leave_type: String,
    start_date: chrono::NaiveDate,
    end_date: chrono::NaiveDate,
    reason: Option<String>,
    requested_at: chrono::DateTime<chrono::Utc>,
}

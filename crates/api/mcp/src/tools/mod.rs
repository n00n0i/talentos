//! MCP tool registry for TalentOS.
//!
//! Each tool module defines:
//! 1. A strongly-typed parameters struct (with JSON Schema via `schemars`).
//! 2. A `Tool` definition (name, description, input_schema).
//! 3. A `ToolRoute::new_dyn` closure that:
//!    - extracts `TenantContext` from request extensions,
//!    - validates it,
//!    - executes the business logic inside `TenantPool::with_tenant_tx`,
//!    - returns a structured JSON error on failure (never panics).
//!
//! All tools are registered into a single [`ToolRouter`] via [`register_all_tools`].

use rmcp::{
    handler::server::router::tool::ToolRoute,
    model::{Tool, ToolAnnotations},
    RoleServer,
};

use crate::McpServer;

pub mod employee;
pub mod payroll;
pub mod leave;
pub mod compliance;

/// Register every MCP tool into the given router.
///
/// This is the single entry point that the [`McpServer::router`] builder calls.
pub fn register_all_tools(router: rmcp::handler::server::Router<McpServer>) -> rmcp::handler::server::Router<McpServer> {
    router
        .with_route(employee::query_employees())
        .with_route(employee::get_employee_detail())
        .with_route(payroll::run_payroll())
        .with_route(payroll::get_payroll_status())
        .with_route(payroll::approve_payroll())
        .with_route(payroll::list_payslips())
        .with_route(leave::request_leave())
        .with_route(leave::check_balance())
        .with_route(leave::list_pending_approvals())
        .with_route(compliance::calculate_tax_preview())
        .with_route(compliance::sso_contribution_check())
        .with_route(compliance::severance_estimate())
}

/// Build a `Tool` with a name, description, and JSON schema derived from `T`.
pub fn tool_def<T: schemars::JsonSchema + 'static>(
    name: &str,
    description: &str,
) -> Tool {
    let schema = rmcp::handler::server::tool::schema_for_input::<T>()
        .unwrap_or_else(|e| panic!("invalid input schema for tool '{name}': {e}"));
    Tool::new(name, description, schema)
}

/// Build a read-only tool annotation.
pub fn read_only_annotation() -> ToolAnnotations {
    ToolAnnotations::new().read_only(true).destructive(false).idempotent(true)
}

/// Build a write tool annotation (destructive, requires confirmation).
pub fn destructive_annotation() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(false)
        .destructive(true)
        .idempotent(false)
}

/// Build a write tool annotation that is idempotent (safe to retry).
pub fn idempotent_write_annotation() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(false)
        .destructive(true)
        .idempotent(true)
}

/// Helper to create a `ToolRoute` from a closure.
pub fn route_dyn<F>(tool: Tool, call: F) -> ToolRoute<McpServer>
where
    F: for<'a> Fn(
        rmcp::handler::server::tool::ToolCallContext<'a, McpServer>,
    ) -> rmcp::service::MaybeBoxFuture<'a, Result<rmcp::model::CallToolResponse, rmcp::ErrorData>>
        + Send
        + Sync
        + 'static,
{
    ToolRoute::new_dyn(tool, call)
}
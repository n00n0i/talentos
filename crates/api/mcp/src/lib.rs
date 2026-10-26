pub mod auth;
pub mod tools;
pub mod error;

use rmcp::{
    handler::server::{
        ServerHandler,
        router::Router,
    },
    model::{ServerCapabilities, ServerConfig, Implementation},
    service::ServerServiceExt,
    ErrorData,
};
use std::sync::Arc;
use talentos_db::TenantPool;

pub struct McpServer {
    pub pool: Arc<TenantPool>,
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
        .with_server_info(Implementation::new("talentos-mcp", "0.1.0"))
        .with_instructions("TalentOS HR & Payroll Model Context Protocol Server. Use the provided tools to interact with employee records, payroll runs, leave balances, and statutory compliance rules. All requests are strictly isolated by TenantContext RLS.")
    }
}

impl McpServer {
    pub fn new(pool: Arc<TenantPool>) -> Self {
        Self { pool }
    }

    /// Build the full RMCP Router with all HR/Payroll/Compliance/Leave tools registered.
    pub fn router(self) -> Router<Self> {
        let server = self;
        let router = Router::new(server);

        tools::register_all_tools(router)
    }
}

use async_graphql::{ID, SimpleObject, directive};
use chrono::{DateTime, Utc};

#[derive(SimpleObject)]
pub struct User {
    pub id: ID,
    /// Requires hr_admin or payroll_manager role
    #[graphql(directive(requireRole(role = "hr_admin")))]
    #[graphql(directive(requireRole(role = "payroll_manager")))]
    pub email: String,
    pub full_name_th: Option<String>,
    pub full_name_en: Option<String>,
    /// Requires hr_admin or payroll_manager role
    #[graphql(directive(requireRole(role = "hr_admin")))]
    #[graphql(directive(requireRole(role = "payroll_manager")))]
    pub roles: Vec<String>,
    pub is_active: bool,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
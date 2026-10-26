use async_graphql::{ComplexObject, ID, SimpleObject, directive};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::Value as JsonValue;

use crate::context::tenant_ctx;
use crate::types::pagination::PaginationInput;
use crate::types::leave::LeaveBalance;
use crate::types::payroll::Payslip;

#[derive(SimpleObject)]
#[graphql(complex)]
pub struct Employee {
    pub id: ID,
    pub employee_code: String,
    pub first_name_th: String,
    pub last_name_th: String,
    pub first_name_en: Option<String>,
    pub last_name_en: Option<String>,
    /// Masked — only last 3 digits visible. Requires hr_admin or payroll_manager role.
    #[graphql(directive(requireRole(role = "hr_admin")))]
    #[graphql(directive(requireRole(role = "payroll_manager")))]
    pub national_id: Option<String>,
    pub birth_date: Option<NaiveDate>,
    pub hire_date: NaiveDate,
    pub termination_date: Option<NaiveDate>,
    pub position_title: String,
    pub department: Option<String>,
    pub base_salary: Decimal,
    /// Masked — only last 4 digits visible. Requires hr_admin or payroll_manager role.
    #[graphql(directive(requireRole(role = "hr_admin")))]
    #[graphql(directive(requireRole(role = "payroll_manager")))]
    pub bank_account_number: Option<String>,
    pub bank_name: Option<String>,
    pub tax_filing_status: i32,
    /// Masked — only last 3 digits visible. Requires hr_admin or payroll_manager role.
    #[graphql(directive(requireRole(role = "hr_admin")))]
    #[graphql(directive(requireRole(role = "payroll_manager")))]
    pub sso_number: Option<String>,
    pub pf_member: bool,
    pub status: String,
}

#[ComplexObject]
impl Employee {
    async fn entity(&self, ctx: &async_graphql::Context<'_>) -> async_graphql::Result<Entity> {
        let tc = tenant_ctx(ctx)?;
        let pool = crate::context::pool(ctx)?;
        let eid = tc.entity_id.ok_or_else(|| async_graphql::Error::new("no entity context"))?;
        let row = pool
            .with_tenant_tx(tc, |tx| {
                Box::pin(async move {
                    sqlx::query_as!(
                        EntityRow,
                        "SELECT id, name, tax_id, address, is_active FROM entities WHERE id = $1",
                        eid.0
                    )
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(|e| talentos_authz::AuthError::Internal(e.to_string()))
                })
            })
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(row.into())
    }

    async fn payslips(
        &self,
        ctx: &async_graphql::Context<'_>,
        period_start: Option<NaiveDate>,
        period_end: Option<NaiveDate>,
    ) -> async_graphql::Result<Vec<Payslip>> {
        let _tc = tenant_ctx(ctx)?;
        let _ps = period_start;
        let _pe = period_end;
        // shortcut: full query wired when DB migrations land; returns empty for now.
        Ok(vec![])
    }

    async fn leave_balances(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<Vec<LeaveBalance>> {
        let _tc = tenant_ctx(ctx)?;
        Ok(vec![])
    }
}

#[derive(SimpleObject)]
pub struct Entity {
    pub id: ID,
    pub name: String,
    pub tax_id: Option<String>,
    pub address: Option<JsonValue>,
    pub is_active: bool,
}

struct EntityRow {
    id: uuid::Uuid,
    name: String,
    tax_id: Option<String>,
    address: Option<sqlx::types::Json<serde_json::Value>>,
    is_active: bool,
}

impl From<EntityRow> for Entity {
    fn from(r: EntityRow) -> Self {
        Self {
            id: ID::from(r.id.to_string()),
            name: r.name,
            tax_id: r.tax_id,
            address: r.address.map(|j| j.0),
            is_active: r.is_active,
        }
    }
}

#[derive(async_graphql::InputObject, Default)]
pub struct EmployeeFilter {
    pub entity_id: Option<ID>,
    pub status: Option<String>,
    pub department: Option<String>,
    pub search: Option<String>,
}

#[derive(async_graphql::InputObject)]
pub struct CreateEmployeeInput {
    pub entity_id: ID,
    pub employee_code: String,
    #[graphql(validator(max_length = 20))]
    pub first_name_th: String,
    #[graphql(validator(max_length = 20))]
    pub last_name_th: String,
    #[graphql(validator(max_length = 50))]
    pub first_name_en: Option<String>,
    #[graphql(validator(max_length = 50))]
    pub last_name_en: Option<String>,
    #[graphql(validator(pattern = "^[0-9]{13}$"))]
    pub national_id: Option<String>,
    pub birth_date: Option<NaiveDate>,
    pub hire_date: NaiveDate,
    #[graphql(validator(max_length = 100))]
    pub position_title: String,
    #[graphql(validator(max_length = 100))]
    pub department: Option<String>,
    #[graphql(validator(range(min = "0")))]
    pub base_salary: Decimal,
    #[graphql(validator(pattern = "^[0-9]{10,17}$"))]
    pub bank_account_number: Option<String>,
    #[graphql(validator(max_length = 100))]
    pub bank_name: Option<String>,
    #[graphql(validator(range(min = "1", max = "5")))]
    pub tax_filing_status: i32,
    #[graphql(validator(pattern = "^[0-9]{13}$"))]
    pub sso_number: Option<String>,
    pub pf_member: Option<bool>,
}

#[derive(async_graphql::InputObject)]
pub struct UpdateEmployeeInput {
    #[graphql(validator(max_length = 20))]
    pub first_name_th: Option<String>,
    #[graphql(validator(max_length = 20))]
    pub last_name_th: Option<String>,
    #[graphql(validator(max_length = 50))]
    pub first_name_en: Option<String>,
    #[graphql(validator(max_length = 50))]
    pub last_name_en: Option<String>,
    #[graphql(validator(max_length = 100))]
    pub position_title: Option<String>,
    #[graphql(validator(max_length = 100))]
    pub department: Option<String>,
    #[graphql(validator(range(min = "0")))]
    pub base_salary: Option<Decimal>,
    #[graphql(validator(pattern = "^[0-9]{10,17}$"))]
    pub bank_account_number: Option<String>,
    #[graphql(validator(max_length = 100))]
    pub bank_name: Option<String>,
    #[graphql(validator(range(min = "1", max = "5")))]
    pub tax_filing_status: Option<i32>,
    #[graphql(validator(pattern = "^[0-9]{13}$"))]
    pub sso_number: Option<String>,
    pub pf_member: Option<bool>,
}

// Paginated wrapper — declared here to keep co-located with its node type.
#[derive(SimpleObject)]
pub struct PaginatedEmployees {
    pub nodes: Vec<Employee>,
    pub page_info: crate::types::pagination::PageInfo,
    pub total_count: i32,
}
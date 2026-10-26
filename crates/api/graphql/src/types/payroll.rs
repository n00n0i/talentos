use async_graphql::{ComplexObject, Enum, ID, SimpleObject};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;

use crate::types::employee::Employee;
use crate::types::pagination::{PageInfo, PaginationInput};
use crate::types::User;

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum PayrollRunStatus {
    Draft,
    Calculating,
    PendingApproval,
    Approved,
    Rejected,
    Disbursing,
    Completed,
    Failed,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum PayslipStatus {
    Calculated,
    Approved,
    Disbursed,
    Rejected,
}

#[derive(SimpleObject)]
#[graphql(complex)]
pub struct PayrollRun {
    pub id: ID,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub payment_date: NaiveDate,
    pub status: PayrollRunStatus,
    pub total_employees: i32,
    pub total_gross: Option<Decimal>,
    pub total_net: Option<Decimal>,
    pub total_tax: Option<Decimal>,
    pub total_sso: Option<Decimal>,
    pub total_pf: Option<Decimal>,
    pub workflow_id: Option<String>,
    pub created_by: User,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl PayrollRun {
    async fn payslips(
        &self,
        ctx: &async_graphql::Context<'_>,
        filter: Option<PayslipFilter>,
        pagination: Option<PaginationInput>,
    ) -> async_graphql::Result<PaginatedPayslips> {
        let _tc = crate::context::tenant_ctx(ctx)?;
        let _ = (filter, pagination);
        Ok(PaginatedPayslips {
            nodes: vec![],
            page_info: PageInfo {
                has_next_page: false,
                has_previous_page: false,
                start_cursor: None,
                end_cursor: None,
            },
            total_count: 0,
        })
    }
}

#[derive(SimpleObject)]
#[graphql(complex)]
pub struct Payslip {
    pub id: ID,
    pub employee: Employee,
    pub base_salary: Decimal,
    pub gross_earnings: Decimal,
    pub total_deductions: Decimal,
    pub net_pay: Decimal,
    pub tax_withheld: Decimal,
    pub sso_employee: Decimal,
    pub sso_employer: Decimal,
    pub pf_employee: Decimal,
    pub pf_employer: Decimal,
    pub anomalies: Vec<Anomaly>,
    pub status: PayslipStatus,
    pub created_at: DateTime<Utc>,
}

#[ComplexObject]
impl Payslip {
    async fn payroll_run(
        &self,
        ctx: &async_graphql::Context<'_>,
    ) -> async_graphql::Result<PayrollRun> {
        let _tc = crate::context::tenant_ctx(ctx)?;
        // shortcut: resolve via dataloader once wiring lands.
        Err(async_graphql::Error::new("not implemented: payslip.payrollRun"))
    }
}

#[derive(SimpleObject)]
pub struct Anomaly {
    pub code: String,
    pub severity: String,
    pub message: String,
}

#[derive(async_graphql::InputObject, Default)]
pub struct PayrollRunFilter {
    pub entity_id: Option<ID>,
    pub status: Option<PayrollRunStatus>,
    pub period_start: Option<NaiveDate>,
    pub period_end: Option<NaiveDate>,
}

#[derive(async_graphql::InputObject, Default)]
pub struct PayslipFilter {
    pub employee_id: Option<ID>,
    pub status: Option<PayslipStatus>,
}

#[derive(async_graphql::InputObject)]
pub struct StartPayrollRunInput {
    pub entity_id: ID,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub payment_date: NaiveDate,
}

#[derive(SimpleObject)]
pub struct PaginatedPayrollRuns {
    pub nodes: Vec<PayrollRun>,
    pub page_info: PageInfo,
    pub total_count: i32,
}

#[derive(SimpleObject)]
pub struct PaginatedPayslips {
    pub nodes: Vec<Payslip>,
    pub page_info: PageInfo,
    pub total_count: i32,
}
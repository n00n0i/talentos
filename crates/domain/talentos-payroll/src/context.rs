use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use talentos_kernel::{EntityId, Money, TenantId, UserId};

/// Immutable per-employee input to the payroll DAG.
/// All quantities are `Decimal` — payroll input never uses floats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmployeePayrollInput {
    pub employee_id: uuid::Uuid,
    pub base_salary: Money,
    pub allowances: Vec<Money>,
    pub ot_hours_weekday: Decimal,
    pub ot_hours_weekend: Decimal,
    pub ot_hours_holiday: Decimal,
    pub days_worked: Decimal,
    /// Approved unpaid leave days inside the pay period. Gross is reduced by
    /// base_salary / 30 per day (Thai monthly-salary practice).
    pub unpaid_leave_days: Decimal,
    pub is_married: bool,
    pub child_count: u32,
    pub insurance_paid: Decimal,
    pub pf_rate: Decimal,
    /// Social Security Act §33 coverage. Independent of the provident fund.
    pub is_sso_enrolled: bool,
    pub is_pf_member: bool,
}

/// Immutable snapshot of a payroll run. The DAG is a pure function of this.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayrollContext {
    pub run_id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub entity_id: EntityId,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub payment_date: NaiveDate,
    pub employees: Vec<EmployeePayrollInput>,
    pub created_by: UserId,
}

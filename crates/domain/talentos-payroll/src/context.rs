use chrono::NaiveDate;
use rust_decimal::Decimal;
use talentos_kernel::{EntityId, TenantId, UserId, Money};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmployeePayrollInput {
    pub employee_id: uuid::Uuid,
    pub base_salary: Money,
    pub allowances: Vec<Money>,
    pub ot_hours_weekday: f64,
    pub ot_hours_weekend: f64,
    pub ot_hours_holiday: f64,
    pub days_worked: f64,
    pub is_married: bool,
    pub child_count: u32,
    pub insurance_paid: Decimal,
    pub pf_rate: Decimal,
    pub is_sso_enrolled: bool,
}

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
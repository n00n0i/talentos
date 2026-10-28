use super::dag::StepResult;
use crate::anomaly::Anomaly;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use talentos_kernel::{Money, TenantId, UserId};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmployeePayrollOutput {
    pub employee_id: Uuid,
    pub base_salary: Money,
    pub gross_earnings: Money,
    pub ot_amount: Money,
    pub total_deductions: Money,
    pub sso_employee: Money,
    pub tax_withheld: Money,
    pub pf_employee: Money,
    pub loan_deduction: Money,
    pub reimbursement: Money,
    pub net_pay: Money,
    pub anomalies: Vec<Anomaly>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PayrollResult {
    pub run_id: Uuid,
    pub tenant_id: TenantId,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub payments_date: NaiveDate,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
    pub employee_outputs: Vec<EmployeePayrollOutput>,
    pub step_traces: Vec<Vec<StepResult>>,
    pub total_gross: Decimal,
    pub total_tax: Decimal,
    pub total_sso: Decimal,
    pub total_pf: Decimal,
    pub total_net: Decimal,
    pub total_ot: Decimal,
    pub errors: Vec<String>,
}

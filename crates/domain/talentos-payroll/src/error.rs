use thiserror::Error;

#[derive(Debug, Error)]
pub enum PayrollError {
    #[error("employee not found: {employee_id}")]
    EmployeeNotFound { employee_id: String },

    #[error("invalid payroll period: {0}")]
    InvalidPeriod(String),

    #[error("negative net pay for employee {employee_id}: gross={gross}, deductions={deductions}")]
    NegativeNetPay {
        employee_id: String,
        gross: String,
        deductions: String,
    },

    #[error("concurrent payroll run already exists for period {period_start}..{period_end}")]
    ConcurrentRun {
        period_start: String,
        period_end: String,
    },

    #[error("compliance calculation failed: {0}")]
    Compliance(#[from] talentos_compliance::ComplianceError),

    #[error("missing required field: {field}")]
    MissingField { field: String },

    #[error("internal error: {0}")]
    Internal(String),
}
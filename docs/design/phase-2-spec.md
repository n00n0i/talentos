# TalentOS Phase 2 — Payroll & Compliance Engine Specification

> **Scope**: Pure calculation DAG engine (`talentos-payroll`), Thai compliance rulebook (`talentos-compliance`), Temporal workflow orchestration (`payroll-worker`), database schema & migrations (`20261026000001_payroll_core.sql`, `20261026000002_payroll_constants.sql`), and API contracts for payroll execution, approval, payslips, and anomaly detection.
> **Dependency**: Phase 1 (AuthZ + Gateway + RLS) must be verified and passing.

---

## 1. Module Boundaries

### 1.1 `crates/domain/talentos-payroll/`
Pure calculation engine. Zero framework dependencies, zero database access, zero async runtimes.
- **Responsibility**: Implements the Directed Acyclic Graph (DAG) of pure functions for salary, tax, SSO, PF, OT, severance, and net pay computation.
- **Public API surface**:
  - `PayrollContext`: Immutable snapshot of company parameters, run period, and active employee profiles.
  - `PayrollDag`: Executes nodes in topological order (`gross → allowances → ot → tax → sso → pf → net → payslip`).
  - `PayrollResult`: Final computed payslips and aggregate totals with full audit trace (`Vec<PayrollStep>`).

### 1.2 `crates/domain/talentos-compliance/`
Thai labor and tax compliance rulebook. Hard-coded statutory constants (PND 2567, SSO caps, PF limits, LPA severance tiers, OT multipliers) hot-swappable via configuration.
- **Responsibility**: Provides pure lookup functions and calculation helpers conforming to Thai Revenue Department, Social Security Office, and Ministry of Labour regulations.

### 1.3 `crates/services/payroll-worker/`
Temporal worker binary crate. Owns Temporal activities and workflows for asynchronous, durable payroll batch execution.
- **Responsibility**: Orchestrates parallel per-employee calculations via Temporal, handles human-in-the-loop signals (`ApprovePayroll`, `RejectPayroll`), and persists final results to PostgreSQL via tenant-scoped transactions.

### 1.4 `crates/infra/db/` (Updates)
- Adds migration scripts for payroll core tables (`payroll_runs`, `payslips`, `payslip_lines`, `payroll_events`) and compliance configuration tables (`tax_brackets`, `sso_rules`, `pf_rules`, `severance_rules`, `ot_rules`) with strict Row Level Security (RLS) enforcement.

---

## 2. Domain Model & PostgreSQL Schema

### 2.1 Rust Structs (`crates/domain/talentos-payroll/src/models.rs`)

```rust
use rust_decimal::Decimal;
use uuid::Uuid;
use chrono::{NaiveDate, DateTime, Utc};
use talentos_kernel::{TenantId, EntityId, UserId, Money};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayrollRun {
    pub id: Uuid,
    pub tenant_id: TenantId,
    pub entity_id: EntityId,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub payment_date: NaiveDate,
    pub status: PayrollRunStatus,
    pub total_employees: i32,
    pub total_gross: Money,
    pub total_net: Money,
    pub total_tax: Money,
    pub total_sso: Money,
    pub total_pf: Money,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payslip {
    pub id: Uuid,
    pub payroll_run_id: Uuid,
    pub tenant_id: TenantId,
    pub employee_id: Uuid,
    pub base_salary: Money,
    pub gross_earnings: Money,
    pub total_deductions: Money,
    pub net_pay: Money,
    pub tax_withheld: Money,
    pub sso_employee: Money,
    pub sso_employer: Money,
    pub pf_employee: Money,
    pub pf_employer: Money,
    pub lines: Vec<PayslipLine>,
    pub anomalies: Vec<PayrollAnomaly>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineType {
    Earning,
    Deduction,
    Tax,
    Contribution,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayslipLine {
    pub code: String,
    pub name: String,
    pub line_type: LineType,
    pub amount: Money,
    pub taxable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayrollAnomaly {
    pub code: String,
    pub severity: String, // "warning", "error"
    pub message: String,
}
```

### 2.2 PostgreSQL Schema Migration (`migrations/20261026000001_payroll_core.sql`)

```sql
CREATE TYPE payroll_status AS ENUM (
    'draft', 'calculating', 'pending_approval', 'approved', 'rejected', 'disbursing', 'completed', 'failed'
);

CREATE TABLE payroll_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    entity_id UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    period_start DATE NOT NULL,
    period_end DATE NOT NULL,
    payment_date DATE NOT NULL,
    status payroll_status NOT NULL DEFAULT 'draft',
    total_employees INT NOT NULL DEFAULT 0,
    total_gross NUMERIC(14,2) NOT NULL DEFAULT 0.00,
    total_net NUMERIC(14,2) NOT NULL DEFAULT 0.00,
    total_tax NUMERIC(14,2) NOT NULL DEFAULT 0.00,
    total_sso NUMERIC(14,2) NOT NULL DEFAULT 0.00,
    total_pf NUMERIC(14,2) NOT NULL DEFAULT 0.00,
    created_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(entity_id, period_start, period_end)
);

CREATE INDEX idx_payroll_runs_tenant ON payroll_runs(tenant_id, status);

CREATE TABLE payslips (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    payroll_run_id UUID NOT NULL REFERENCES payroll_runs(id) ON DELETE CASCADE,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE RESTRICT,
    base_salary NUMERIC(12,2) NOT NULL,
    gross_earnings NUMERIC(12,2) NOT NULL,
    total_deductions NUMERIC(12,2) NOT NULL,
    net_pay NUMERIC(12,2) NOT NULL,
    tax_withheld NUMERIC(12,2) NOT NULL,
    sso_employee NUMERIC(12,2) NOT NULL,
    sso_employer NUMERIC(12,2) NOT NULL,
    pf_employee NUMERIC(12,2) NOT NULL,
    pf_employer NUMERIC(12,2) NOT NULL,
    lines JSONB NOT NULL DEFAULT '[]',
    anomalies JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(payroll_run_id, employee_id)
);

CREATE INDEX idx_payslips_employee ON payslips(tenant_id, employee_id);

CREATE TABLE payroll_events (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    payroll_run_id UUID REFERENCES payroll_runs(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    actor TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_payroll_events_run ON payroll_events(payroll_run_id, created_at);

-- RLS
ALTER TABLE payroll_runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE payroll_runs FORCE ROW LEVEL SECURITY;
CREATE POLICY payroll_runs_isolation ON payroll_runs USING (tenant_id = current_tenant_id());

ALTER TABLE payslips ENABLE ROW LEVEL SECURITY;
ALTER TABLE payslips FORCE ROW LEVEL SECURITY;
CREATE POLICY payslips_isolation ON payslips USING (tenant_id = current_tenant_id());

ALTER TABLE payroll_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE payroll_events FORCE ROW LEVEL SECURITY;
CREATE POLICY payroll_events_isolation ON payroll_events USING (tenant_id = current_tenant_id());
```

### 2.3 PostgreSQL Schema Migration (`migrations/20261026000002_payroll_constants.sql`)

```sql
CREATE TABLE tax_brackets (
    id SERIAL PRIMARY KEY,
    tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE, -- NULL = global Thai default
    tax_year INT NOT NULL DEFAULT 2567,
    lower_bound NUMERIC(12,2) NOT NULL,
    upper_bound NUMERIC(12,2), -- NULL = infinity
    rate NUMERIC(5,4) NOT NULL  -- e.g., 0.0500 for 5%
);

CREATE TABLE sso_rules (
    id SERIAL PRIMARY KEY,
    tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE,
    effective_from DATE NOT NULL,
    employee_rate NUMERIC(5,4) NOT NULL DEFAULT 0.0500,
    employer_rate NUMERIC(5,4) NOT NULL DEFAULT 0.0500,
    min_wage_base NUMERIC(10,2) NOT NULL DEFAULT 1650.00,
    max_wage_base NUMERIC(10,2) NOT NULL DEFAULT 15000.00
);

CREATE TABLE pf_rules (
    id SERIAL PRIMARY KEY,
    tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE,
    min_employee_rate NUMERIC(5,4) NOT NULL DEFAULT 0.0200,
    max_employee_rate NUMERIC(5,4) NOT NULL DEFAULT 0.1500,
    employer_match_rate NUMERIC(5,4) NOT NULL DEFAULT 0.0500
);

CREATE TABLE severance_rules (
    id SERIAL PRIMARY KEY,
    min_months_worked INT NOT NULL,
    max_months_worked INT,
    severance_days INT NOT NULL
);

-- Seed Thai statutory constants (2567)
INSERT INTO tax_brackets (tax_year, lower_bound, upper_bound, rate) VALUES
(2567, 0.00, 150000.00, 0.0000),
(2567, 150001.00, 300000.00, 0.0500),
(2567, 300001.00, 500000.00, 0.1000),
(2567, 500001.00, 750000.00, 0.1500),
(2567, 750001.00, 1000000.00, 0.2000),
(2567, 1000001.00, 2000000.00, 0.2500),
(2567, 2000001.00, 5000000.00, 0.3000),
(2567, 5000001.00, NULL, 0.3500);

INSERT INTO sso_rules (effective_from, employee_rate, employer_rate, min_wage_base, max_wage_base) VALUES
('2024-01-01', 0.0500, 0.0500, 1650.00, 15000.00);

INSERT INTO severance_rules (min_months_worked, max_months_worked, severance_days) VALUES
(120, 360, 30),   -- 120 days to < 3 years: 30 days
(36, 72, 90),     -- 3 years to < 6 years: 90 days
(72, 120, 180),   -- 6 years to < 10 years: 180 days
(120, 240, 240),  -- 10 years to < 20 years: 240 days
(240, NULL, 400); -- 20 years or more: 400 days
```

---

## 3. Payroll DAG Engine (`talentos-payroll`)

The calculation engine is structured as a Directed Acyclic Graph (DAG) of pure functions. 

```
[PayrollContext] + [EmployeeInput]
       │
       ▼
 1. calculate_gross (base salary + recurring earnings)
       │
       ▼
 2. calculate_allowances (fixed / variable stipends)
       │
       ▼
 3. calculate_ot (weekday 1.5x, weekend 2x/3x, max 36 hrs cap)
       │
       ▼
 4. calculate_sso (5% capped at ฿15,000)
       │
       ▼
 5. calculate_pf (Provident Fund 2%-15% + employer match)
       │
       ▼
 6. calculate_tax (PND progressive tax on annualised net)
       │
       ▼
 7. calculate_net (gross + earnings - deductions - tax - sso - pf)
       │
       ▼
 8. generate_payslip (produces final Payslip + audit step trail)
```

### Determinism Invariants
- **No Side Effects**: Functions take immutable references/values and return new computed results.
- **No Randomness**: No UUID generation or RNG calls inside calculation nodes.
- **No Clock Dependency**: All date calculations use fixed period dates passed via `PayrollContext`.
- **Decimal Only**: All currency arithmetic uses `rust_decimal::Decimal` with banker's rounding (`round_dp(2)`).

---

## 4. Thai Compliance Rules (`talentos-compliance`)

### 4.1 PND Progressive Tax (2567)
- **Personal Allowance**: ฿60,000 per year.
- **SSO Deduction**: Actual employee SSO contributed during the year (max ฿9,000/year).
- **Provident Fund Deduction**: Actual PF contribution up to ฿10,000 exempt, remaining up to 15% of wage exempt up to ฿490,000 total cap.
- **Calculation Formula**: Annualized Assessable Income = `(Gross - SSO - PF) * 12`. Net Taxable Income = `Annualized Income - Personal Allowance - Deductions`. Apply progressive brackets to find annual tax, then divide by 12 for monthly withholding tax.

### 4.2 Social Security (SSO)
- Rate: 5% employee + 5% employer.
- Wage Base: Min ฿1,650, Max ฿15,000.
- Max monthly contribution: `15000 * 0.05 = ฿750` employee, `฿750` employer. Salaries above ฿15,000 contribute exactly ฿750.

### 4.3 Provident Fund (PF)
- Employee Contribution: Configurable between 2% and 15% of base salary.
- Employer Match: Configurable fixed or tiered rate (default 5%).

### 4.4 LPA Severance & Overtime
- **Severance Pay**: Based on length of service (30 to 400 days of final wage rate). Formula: `(Final Daily Wage * Severance Days)`. Final Daily Wage = `Monthly Base Salary / 30`.
- **Overtime (OT)**:
  - Weekday (1.5×): `Hourly Rate * 1.5 * Hours`
  - Rest Day / Weekend (2.0×): `Hourly Rate * 2.0 * Hours`
  - Holiday (3.0×): `Hourly Rate * 3.0 * Hours`
  - Max OT cap: 36 hours per week.

---

## 5. Temporal Workflow Design (`payroll-worker`)

### 5.1 Workflow Definition
- `CalculatePayrollWorkflow(run_id)`:
  1. Loads payroll run metadata and active employees for the entity.
  2. Spawns parallel `CalculateEmployeePayslipActivity(run_id, employee_id)` for each employee via Temporal futures.
  3. Awaits completion of all activities.
  4. Aggregates totals (gross, net, tax, sso, pf).
  5. Transitions run status to `PendingApproval`.
  6. Waits for external signal (`ApprovePayroll` or `RejectPayroll`).
  7. If approved: executes disbursement activity and marks run `Completed`. If rejected: marks run `Rejected`.

### 5.2 Signals & Queries
- **Signals**:
  - `ApprovePayroll { approver_user_id: Uuid }`
  - `RejectPayroll { approver_user_id: Uuid, reason: String }`
- **Queries**:
  - `GetProgress() -> (completed_count: usize, total_count: usize)`
  - `GetAnomalies() -> Vec<EmployeeAnomalySummary>`

---

## 6. API Contracts (`crates/api/gateway/`)

| Endpoint | Method | Permission Required | Description |
|---|---|---|---|
| `/api/v1/payroll/runs` | `POST` | `payroll_run.calculate` | Start new payroll run for period |
| `/api/v1/payroll/runs/:id` | `GET` | `payroll_run.calculate` | Get run status, progress, and aggregate totals |
| `/api/v1/payroll/runs/:id/approve` | `POST` | `payroll_run.approve` | Signal approval for payroll run |
| `/api/v1/payroll/runs/:id/payslips` | `GET` | `payslip.view_all` | List generated payslips with filters |
| `/api/v1/payroll/payslips/:id/pdf` | `GET` | `payslip.view_self` | Download payslip PDF via MinIO signed URL |

---

## 7. Anomaly Detection Rules (Pure Functions)

Executed post-calculation per employee:
1. **OT Excess**: OT hours > 40 hrs/month → Warning.
2. **Salary Spike**: Base salary change > 20% compared to previous month without promotion record → Error.
3. **SSO Cap Hit**: Base salary ≥ ฿15,000 → Note SSO cap applied (informational).
4. **Negative Net Pay**: Net pay < 0 → Error (blocks approval).
5. **Missing PF**: Employee eligible but PF member = false → Warning.
6. **Duplicate Bank Account**: Same bank account number shared across multiple active employees in entity → Error.

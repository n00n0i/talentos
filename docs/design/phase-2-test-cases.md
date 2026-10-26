# TalentOS Phase 2 — Integration Test Cases

> **Scope**: 30 integration-level test cases covering payroll calculation DAG, Thai compliance (PND tax, SSO, PF, LPA severance, OT), Temporal workflow orchestration, anomaly detection, audit trail, error handling, and cross-tenant RLS isolation.
> **Environment**: Live integration test suite running against PostgreSQL, Redis, Kafka, and Temporal via `docker-compose`.

---

## Test Case Table

| TC-ID | Category | Description | Given | When | Then | Priority |
|---|---|---|---|---|---|---|
| **TC-26** | Happy Path | Single employee payroll run computes correctly | Tenant A, active employee Somchai with base salary ฿50,000, 1 dependent, SSO active, no PF | `POST /api/v1/payroll/runs` with period 2026-10 | `201 Created`; run ID returned; worker calculates payslip; gross = ฿50,000; SSO = ฿750 (capped); Tax calculated per PND 2567 progressive brackets; Net pay = Gross - SSO - Tax; audit events recorded | P0 |
| **TC-27** | Happy Path | Multi-employee company run processes in parallel | Tenant A with 50 active employees across various salary tiers | `POST /api/v1/payroll/runs` | `201 Created`; Temporal workflow spawns 50 parallel activities; completes within SLA (<5s); aggregate `total_gross` and `total_net` match sum of individual payslips exactly | P0 |
| **TC-28** | Happy Path | Full approval workflow cycle (Calculate → Pending → Approve → Completed) | Payroll run in `pending_approval` state | `POST /api/v1/payroll/runs/:id/approve` with `approver_user_id` | Status transitions to `approved`, then `completed`; audit log records approval action with actor ID; event published to Kafka | P0 |
| **TC-29** | Happy Path | Payslip list and filtering by department | Completed payroll run with 50 payslips | `GET /api/v1/payroll/runs/:id/payslips?department=Engineering` | `200 OK`; returns only payslips belonging to engineering employees; pagination works correctly | P1 |
| **TC-30** | Happy Path | Payslip PDF download returns valid MinIO signed URL | Generated payslip ID for employee Somchai | `GET /api/v1/payroll/payslips/:id/pdf` | `200 OK`; returns JSON with `download_url` pointing to MinIO pre-signed URL with 15-minute expiry | P1 |
| **TC-31** | Boundary Values | Salary exactly at tax bracket edge (฿300,000 annual) | Employee with annual assessable income exactly ฿300,000 | Run payroll calculation | Tax calculated using 5% bracket correctly without off-by-one errors; no floating point drift | P0 |
| **TC-32** | Boundary Values | SSO wage cap enforcement at exactly ฿15,000 vs ฿50,000 | Employee E1 at ฿15,000 base salary; Employee E2 at ฿50,000 base salary | Run payroll calculation | E1 SSO employee = ฿750; E2 SSO employee = ฿750 (cap strictly enforced); employer contributions match | P0 |
| **TC-33** | Boundary Values | Zero days worked / unpaid leave reduction | Employee with 0 days worked in period due to unpaid leave | Run payroll calculation | Base salary pro-rated to ฿0.00; gross earnings = allowances only; SSO = ฿0; net pay = ฿0 or allowances minus deductions | P1 |
| **TC-34** | Boundary Values | Leap year Feb 29 daily wage calculation for severance | Employee terminated on leap year Feb 29 | Calculate LPA severance daily wage | Daily wage divisor correctly uses 366 days for annual rate or 30 days for monthly rate without panic | P2 |
| **TC-35** | Thai Compliance | PND Progressive tax calculation correctness | Employee with annual net taxable income ฿500,000 | Run tax calculation node | Tax computed as: (150k @ 0%) + (150k @ 5% = 7,500) + (200k @ 10% = 20,000) = ฿27,500 annual / 12 = ฿2,291.67 monthly | P0 |
| **TC-36** | Thai Compliance | SSO minimum wage floor enforcement | Employee with part-time base salary ฿1,200 (below min ฿1,650) | Run SSO calculation node | SSO wage base floored at ฿1,650; employee SSO contribution = `1650 * 0.05 = ฿82.50` | P1 |
| **TC-37** | Thai Compliance | Provident Fund (PF) employee deduction & employer match | Employee enrolled in PF with 5% employee contribution & 5% employer match | Run payroll calculation | PF employee deduction = 5% of base salary; PF employer contribution = 5% recorded on payslip | P1 |
| **TC-38** | Thai Compliance | LPA Severance calculation for 12 years of service | Employee terminated after 144 months (12 years) of service, base salary ฿60,000 | Calculate severance rule | Service falls in 10-20 years bracket (240 days). Daily wage = 60,000 / 30 = 2,000. Severance = `2,000 * 240 = ฿480,000` | P0 |
| **TC-39** | Overtime (OT) | Weekday, weekend, and holiday OT calculations | Employee with base salary ฿30,000 (hourly rate = `30000 / 30 / 8 = ฿125`) | Calculate OT with 10 hrs weekday (1.5x), 5 hrs weekend (2x), 4 hrs holiday (3x) | Weekday OT = `125 * 1.5 * 10 = ฿1,875`; Weekend = `125 * 2.0 * 5 = ฿1,250`; Holiday = `125 * 3.0 * 4 = ฿1,500`; Total OT = ฿4,625 | P0 |
| **TC-40** | Overtime (OT) | Max OT hours enforcement (36 hours/week cap) | Employee submitted for 45 hours of OT in a single week | Run payroll calculation / validation | Anomaly generated (`ot_exceeds_max_limit`, severity: warning); calculation proceeds but flags compliance review | P1 |
| **TC-41** | Anomaly Detection | OT > 40 hrs/month triggers anomaly | Employee with 45 OT hours logged in monthly cycle | Run anomaly detection step | Payslip contains anomaly object with code `excessive_ot`, severity `warning` | P1 |
| **TC-42** | Anomaly Detection | Salary change > 20% without promotion record triggers error | Employee base salary increased from ฿50,000 to ฿70,000 (+40%) with no promotion record | Run anomaly detection step | Payslip contains anomaly with code `unauthorized_salary_spike`, severity `error` | P0 |
| **TC-43** | Anomaly Detection | Negative net pay blocks approval | Allowances / deductions result in negative net pay | Run calculation & attempt approval | Anomaly `negative_net_pay` triggered (severity `error`); run approval rejected with 422 validation error | P0 |
| **TC-44** | Anomaly Detection | Duplicate bank account detection across active employees | Two active employees assigned identical bank account number | Run anomaly detection across entity | Anomaly `duplicate_bank_account` flagged for both employees | P1 |
| **TC-45** | Temporal Workflow | Workflow replay determinism | Completed payroll run workflow execution | Replay workflow history in Temporal test harness | Replayed execution produces identical states, identical calculations, and zero state divergence | P0 |
| **TC-46** | Temporal Workflow | Signal handling during active calculation | Payroll run in `calculating` state | Send `RejectPayroll` signal while activities are running | Workflow aborts active calculation activities, transitions state to `rejected`, records reason in audit log | P1 |
| **TC-47** | Temporal Workflow | Query consistency during run progress | Payroll run calculating 100 employees (20 completed) | Query workflow via `GetProgress()` | Returns `(20, 100)` accurately without locking database tables | P1 |
| **TC-48** | Audit Trail | Every calculation step logged immutably | Run payroll calculation | Inspect `payroll_events` and `audit_log` tables | Every DAG step (`calculate_gross`, `calculate_tax`, etc.) has corresponding immutable audit event with correlation ID | P0 |
| **TC-49** | Error Cases | Invalid period (end date before start date) rejected | Start date: 2026-10-31, End date: 2026-10-01 | `POST /api/v1/payroll/runs` | `422 Unprocessable Entity`; validation error returned specifying period bounds | P1 |
| **TC-50** | Error Cases | Missing employee profile data prevents run | Employee record missing mandatory `base_salary` or `tax_filing_status` | Execute payroll run | Activity catches validation error, marks employee payslip status as failed, run captures partial failure without panicking | P0 |
| **TC-51** | Error Cases | Concurrent payroll run prevention for same entity & period | Existing active payroll run for Entity E1 in period Oct 2026 | Attempt second `POST /api/v1/payroll/runs` for same entity and period | `409 Conflict`; duplicate run prevented by unique database constraint and application check | P0 |
| **TC-52** | Cross-Tenant Isolation | Payroll run never leaks data across tenants | Tenant A payroll run ID known by Tenant B user | Tenant B user calls `GET /api/v1/payroll/runs/:id` | `404 Not Found` (enforced by RLS `tenant_id = current_tenant_id()`) | P0 |
| **TC-53** | Cross-Tenant Isolation | Payslips scoped strictly to tenant | Tenant B queries payslips endpoint with Tenant A run ID | `GET /api/v1/payroll/runs/:id/payslips` under Tenant B token | `404 Not Found` or empty array; zero data leakage | P0 |
| **TC-54** | Cross-Tenant Isolation | Global tax constants accessible, tenant constants override correctly | Tenant A has custom tax bracket override in `tax_brackets` table | Run payroll for Tenant A vs Tenant B | Tenant A uses custom bracket; Tenant B falls back to default Thai 2567 constants | P1 |
| **TC-55** | Integration E2E | End-to-end payroll lifecycle from HTTP request to MinIO PDF | Auth token with `payroll_run.calculate` role | `POST /runs` → Wait for workflow → `POST /approve` → `GET /payslips/:id/pdf` | Complete sequence returns `200 OK` at each step; final PDF download URL successfully generated | P0 |

---

## Coverage Summary

| Category | Count | TC-IDs |
|---|---|---|
| Happy Path | 5 | TC-26, TC-27, TC-28, TC-29, TC-30 |
| Boundary Values | 4 | TC-31, TC-32, TC-33, TC-34 |
| Thai Compliance | 5 | TC-35, TC-36, TC-37, TC-38, TC-39 |
| Overtime & Caps | 2 | TC-40, TC-41 |
| Anomaly Detection | 4 | TC-42, TC-43, TC-44, TC-45 |
| Temporal Workflow | 3 | TC-46, TC-47, TC-48 |
| Audit & Error Cases | 5 | TC-49, TC-50, TC-51, TC-52, TC-53 |
| Cross-Tenant Isolation | 3 | TC-54, TC-55, TC-56 (mapped to TC-52, TC-53, TC-54) |
| **Total** | **30** | |

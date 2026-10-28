# Provident Fund Remittance Implementation Plan

**Goal:** Export a deterministic PF remittance CSV for an approved payroll run (employee + employer contributions), using `pf_member` on the employee form as membership management.

**Files:** `crates/services/talentos-hr-runner/src/reports.rs`, `crates/api/gateway/src/handlers/reports.rs`, `router.rs`, `web/app/(app)/payroll/page.tsx`, tests.

**Interface:** `GET /api/v1/payroll/runs/:id/pf-remittance.csv` (`payroll_run.export_report`), approved runs only, header `employee_code,name,employee_amount,employer_amount,total`, one row per PF-member payslip, totals row, Decimal 2dp, CSV-injection safe, stable order, filename with run id.

**Tests:** DB test with two PF members/one non-member; sums match payslips; deterministic bytes; cross-tenant 404.

**UI:** “ดาวน์โหลด PF (CSV)” link next to GL on approved runs.

**Out of scope:** fund provider API, historical rate changes (PF rate stays 5%), member enrollment beyond the existing employee `pf_member` toggle.

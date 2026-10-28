# Final Settlement Implementation Plan

**Goal:** HR computes and stores a draft final settlement for a departing employee: last-month pay, unused leave, statutory severance, and notice pay.

**Architecture:** A pure calculator in `talentos-hr-runner` reuses `talentos_compliance::calculate_severance`; a `final_settlements` table stores the snapshot and breakdown. A small `/final-settlement` page selects an employee, takes the termination date/unused days/notice days, and shows the result. Tax/SSO withholding and the official 50 ทวิ/SSO forms stay with payroll/Phase 0.

**Files:** migration `20261110000001_final_settlements.sql`; `hr-runner/src/settlement.rs`; gateway handler/route; `web/app/(app)/final-settlement/page.tsx`; nav; tests.

**Interface:**
`POST /api/v1/employees/:id/final-settlement` (`employee.update`) `{termination_date, unused_leave_days, notice_days, include_severance}` → draft snapshot with:
`last_month_pay = base_salary/30 × min(day_of_month,30)`,
`leave_pay = base_salary/30 × unused_days`,
`notice_pay = base_salary/30 × notice_days`,
`severance = compliance::calculate_severance(base_salary, years)` when included,
`total_gross`; `years_of_service`.
`GET /api/v1/final-settlements?employee_id=` lists drafts.

**Constraints:** Decimal money; own tenant/employee only; 0 ≤ days; termination_date ≥ hire_date; no payroll run/journal change; no production unwrap/expect; Thai UI; tests + build + Playwright 20/20.

**Tests:** final-month proration, unused leave, severance boundary/years, notice, totals, tenant isolation; E2E: HR computes for E2E001 and sees the total.

**Out of scope:** tax withholding, 50 ทวิ, SSO termination forms, final payroll run integration.

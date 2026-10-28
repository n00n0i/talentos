# Claims Reimbursement Through Payroll Plan

**Goal:** Approved expense claims are paid with the next payroll run as a non-taxable reimbursement, marked paid once, with the payslip and GL reflecting it.

**Files:** migration; `talentos-payroll` input/output/DAG/render; payroll runner; claims/gateway; reports GL; tests; claims UI label.

## Rules
- `payslips.reimbursement` and `EmployeePayrollInput.reimbursement` (default 0).
- Net pay = gross − SSO − tax − PF − loan + reimbursement; tax/SSO/PF base unchanged; negative-net guard applies to the final net.
- Runner sums approved claims with `expense_date` in the run period and `payroll_run_id IS NULL OR payroll_run_id = this_run`, passes per employee, then marks those claims `paid` with the run id (idempotent on re-run).
- Claims status gains `paid`; only approved claims are picked.
- GL: debit `5200 ค่าใช้จ่ายพนักงาน` = reimbursement; net credit already includes it, so the journal stays balanced.
- Payslip renders a “เบิกค่าใช้จ่าย” line when > 0.
- Failed payslips do not mark claims paid.
- No change to tax/SSO/PF math; no double payment on rerun.

## Tests
- Domain net with reimbursement.
- Runner: approved claim in period → payslip reimbursement/net and claim `paid`; rerun same run no double; claim outside period ignored; rejected claim ignored; failed payslip leaves claim approved.
- GL balanced with reimbursement.

## Gates
fmt/clippy/tests/build/Playwright 21/21. No new UI flow (claims page shows “จ่ายแล้ว”).

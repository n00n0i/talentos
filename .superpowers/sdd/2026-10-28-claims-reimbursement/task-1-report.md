# Claims Reimbursement Through Payroll — Task 1 Report

## Status
Completed.

## Commit
`67cff83` — `feat(claims): reimburse approved expense claims through payroll`

## Changes

- `migrations/20261113000001_claims_payroll.sql`
  - `payslips.reimbursement NUMERIC(12,2) NOT NULL DEFAULT 0`.
  - `expense_claims.payroll_run_id UUID REFERENCES payroll_runs ON DELETE SET NULL`, `paid_at TIMESTAMPTZ`.
  - Widen claims status CHECK to include `paid`; add lookup indexes.
- `crates/domain/talentos-payroll/`
  - `EmployeePayrollInput.reimbursement` (serde default zero) and `EmployeePayrollOutput.reimbursement`.
  - DAG: net = gross − sso − tax − pf − loan + reimbursement; new `reimbursement` trace step; payslip output carries it.
  - Render: payslip shows a “เบิกค่าใช้จ่าย” line when > 0.
- `crates/services/talentos-payroll-runner/src/lib.rs`
  - `load_approved_claims` sums approved claims with `expense_date` in the run period and (`payroll_run_id IS NULL OR payroll_run_id = run_id`).
  - Reimbursement passed per employee into the DAG input; written to payslips.
  - `mark_claims_paid` sets `status='paid'`, `payroll_run_id`, `paid_at` in the same tx after a successful payslip write (idempotent).
  - Failed payslips leave claims approved; re-run includes claims already tied to the run (no double-mark).
- `crates/services/talentos-hr-runner/src/reports.rs`
  - GL export adds debit `5200 ค่าใช้จ่ายพนักงาน` = reimbursement per department; net credit already includes it, journal stays balanced.
- `crates/services/talentos-hr-runner/src/claims.rs`
  - `VALID_STATUSES` filter allowlist gains `paid`.
- `web/app/(app)/claims/page.tsx`
  - Claims status badge `paid` → “จ่ายแล้ว”.

## Tests
- `talentos-payroll` domain: net with reimbursement, trace has reimbursement step, render line.
- `crates/services/talentos-payroll-runner/tests/claims_payroll.rs` (5 tests): approved claim in period → payslip reimbursement/net + claim paid; rerun no double; outside-period ignored; rejected ignored; failed payslip leaves claim approved.
- `crates/services/talentos-hr-runner/tests/reports.rs`: `gl_export_balances_with_reimbursement` asserts `5200 ค่าใช้จ่ายพนักงาน` present and journal balances.

## Test Summary
```
cargo fmt --all                                      ok
cargo clippy -p talentos-payroll -p talentos-payroll-runner -p talentos-hr-runner --all-targets   ok
cargo clippy -p talentos-gateway                     ok
cargo test -p talentos-payroll                       23 passed, 0 failed
cargo test -p talentos-payroll-runner --test claims_payroll   5 passed, 0 failed
cargo test -p talentos-hr-runner --test reports gl_export       5 passed, 0 failed
npm run build (web/)                                 ok
Playwright e2e (21 tests, gateway 3139 / web 18309)  21 passed, 0 failed
```

## Concerns
- No `query!` macros changed (all queries are runtime `sqlx::query`), so `cargo sqlx prepare` was not needed.
- Migration applied via psql to `loans-postgres-1`; the `derived_ot_minutes` column from `20261112000001` was already present (applied out-of-band), so no re-migrate was needed.
- Playwright 2FA test requires `PII_ENCRYPTION_KEY` (64 hex chars); run with a generated key as in prior tasks.

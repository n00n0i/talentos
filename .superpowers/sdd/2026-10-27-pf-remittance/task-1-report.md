# PF Remittance Slice — Task 1 Report

## Status
Completed.

## Commit
`7b011ce` — `feat(pf): PF remittance CSV export slice`

## Changes
- `crates/services/talentos-hr-runner/src/reports.rs`
  - Added `export_pf_remittance_csv(conn, run_id)`: approved runs only, 404 otherwise.
  - Rows for payslips with `pf_employee > 0 OR pf_employer > 0`.
  - Header: `employee_code,name,employee_amount,employer_amount,total`.
  - Totals row; 2dp Decimal; stable order by `employee_code`; CSV-escapes formula-leading values.
  - Filename: `pf-remittance-<run_id>-<start>-<end>.csv`.
  - Also added explicit `tenant_id = current_tenant_id()` filters to all report queries so tests are deterministic when the DB role bypasses RLS.
- `crates/api/gateway/src/handlers/reports.rs`
  - Added `export_pf_remittance_csv` handler (`payroll_run.export_report`).
  - Extracted shared `csv_response(filename, body)` helper.
- `crates/api/gateway/src/router.rs`
  - Added route `GET /api/v1/payroll/runs/:id/pf-remittance.csv`.
- `web/app/(app)/payroll/page.tsx`
  - Added "ดาวน์โหลด PF (CSV)" link next to the GL link for approved runs.
- `crates/services/talentos-hr-runner/tests/reports.rs`
  - Added `pf_member` parameter to `seed_employee` helper.
  - Added `pf_remittance_csv_includes_only_pf_members_and_totals`: two PF members + one non-member, amounts and totals verified, deterministic bytes.
  - Added `pf_remittance_csv_rejects_draft_run`: returns `ReportError::NotFound`.

## Test Summary
```
cargo fmt --all                                      ok
cargo clippy -p talentos-hr-runner -p talentos-gateway --all-targets -- -D warnings  ok
cargo test -p talentos-hr-runner --test reports      10 passed, 0 failed
npm run build (web/)                                 ok
```

## Concerns
- I had to add explicit `tenant_id = current_tenant_id()` filters to existing report queries (`headcount`, `payroll_cost`, `ot_trend`, `export_gl_csv`) because the test DB role is a superuser with `BYPASSRLS`, so RLS policies are ignored. Without these filters, parallel tests leaked across tenants and several existing tests failed. This is a correctness improvement but slightly widens the change surface beyond the PF slice.
- The shared `cleanup` helper still does not delete `payroll_runs`/`payslips`; the new tenant filters make this harmless for the reports tests.
- No Playwright run executed; the only UI change is a static link next to the existing GL link.

# PF Effective-Dated Rates — Task 1 Report

**Status:** Completed
**Commit:** `bdb8554` (`feat/pf-rates`)
**Report path:** `.superpowers/sdd/2026-10-29-pf-rates/task-1-report.md`

## What changed

- `migrations/20261116000001_pf_rate_history.sql`
  - `pf_rate_history(id BIGSERIAL, tenant_id, employee_id, effective_from DATE, rate NUMERIC(5,4) CHECK (rate BETWEEN 0 AND 0.15), created_by, created_at)`.
  - `UNIQUE(tenant_id, employee_id, effective_from)`, `ENABLE` + `FORCE` RLS, `tenant_id = current_tenant_id()` policy, effective-lookup index.
  - Applied via psql to `loans-postgres-1` and recorded in `_sqlx_migrations` (checksum = SHA-384 of the file, matching `sqlx::migrate`).
- `crates/services/talentos-hr-runner/src/pf.rs` (new)
  - `rate_for(employee, as_of)` → latest `effective_from <= as_of` else `DEFAULT_PF_RATE` (0.05); `list_rates`; `upsert_rate` (bounds 0..0.15, cross-tenant employee → `EmployeeNotFound`, `ON CONFLICT` same-date replace).
  - Exported from `lib.rs`.
- `crates/services/talentos-payroll-runner/src/lib.rs`
  - `load_pf_rates` in the run tx: `DISTINCT ON (employee_id)` latest rate ≤ `period_end`.
  - `EmployeePayrollInput.pf_rate` set from that map when `pf_member`, else 0 (default 0.05 on no row). Existing PF math untouched.
- `crates/api/gateway/src/handlers/employees.rs` + `router.rs`
  - `GET /api/v1/employees/:id/pf-rates` (`employee.read_all`), `POST .../pf-rates` (`employee.update`) upsert `{effective_from, rate}`; rejects rate <0 or >0.15; 404 for cross-tenant employee (verified via `fetch_one` first).
- `web/app/(app)/employees/page.tsx`
  - Form fields `อัตรา PF (%)` (`employee-pf-rate`) and `มีผลตั้งแต่` (`employee-pf-rate-from`), prefilled from the latest rate on edit; on save, posts to the PF-rate endpoint only when `pf_member`, then reloads.

## Tests

- `crates/services/talentos-hr-runner/tests/pf.rs` (6 DB tests): default 5%, latest ≤ as_of wins, future rate ignored, same-date upsert replaces, bounds rejected, tenant scope (cross-tenant fallback + `EmployeeNotFound`).
- `crates/services/talentos-payroll-runner/tests/execute_run.rs`: `pf_rate_uses_period_end_effective_rate` (3% history → payslip pf 900, not 1,500).

## Gate results

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p talentos-hr-runner -p talentos-payroll-runner -p talentos-gateway --all-targets -- -D warnings` | clean |
| `cargo test -p talentos-hr-runner` (DB-backed) | all pass (incl. 6 pf) |
| `cargo test -p talentos-payroll-runner` | 10 passed, 1 ignored (load), 0 failed |
| `cargo test -p talentos-gateway` | 54 passed, 0 failed |
| `npm run build` (web) | built |
| Playwright (21 tests; gateway 18339 / web 3142, re-seeded `seed-e2e.sql`) | 21 passed, 0 failed |

## Concerns

- No `query!`/`query_as!` macros changed (all runtime `sqlx::query`/`query_scalar`), so `cargo sqlx prepare` was not needed.
- Migration applied manually via psql (owner) + `_sqlx_migrations` row; re-running `sqlx migrate` should be a no-op since the checksum matches the file's SHA-384.
- Payroll uses the rate effective at `period_end` only; a rate change landing mid-period does not retroactively split the period (matches the plan's "period end" rule).
- Gateway/web e2e servers were stopped after the run.

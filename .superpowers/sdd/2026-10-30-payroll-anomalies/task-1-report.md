# Payroll Anomaly Review — Report

**Status:** Completed
**Commit:** `6e00562` (feat/anomaly)
**Report path:** `.superpowers/sdd/2026-10-30-payroll-anomalies/task-1-report.md`

## What changed

- `migrations/20261127000001_payroll_anomaly_review.sql`
  - `payroll_runs.anomaly_reviewed_at TIMESTAMPTZ`, `anomaly_reviewed_by UUID REFERENCES users(id)`.
  - Applied via psql to `talentos` and recorded in `_sqlx_migrations` (sha384 matches
    `sqlx::migrate!`); also applied to a fresh `talentos_e2e` DB via gateway `migrate`.
- `crates/services/talentos-payroll-runner/src/lib.rs`
  - The `pending_approval` transition now clears `anomaly_reviewed_at/by` so a
    recalculation forces re-review.
- `crates/api/gateway/src/handlers/payroll.rs` + `router.rs`
  - `GET /api/v1/payroll/runs/:id/anomalies` (`payroll_run.approve`): any status; flattens
    `payslips.anomalies` JSONB into rows `{employee_code, employee_name, code, severity,
    message}`, counts `{error, warning}`, plus `reviewed_at`/`reviewed_by`.
  - `POST /api/v1/payroll/runs/:id/anomalies/review` (`payroll_run.approve`): only
    `pending_approval`; sets reviewer/time; idempotent; 409 otherwise.
  - `approve_payroll_run`: if any payslip carries anomalies and `anomaly_reviewed_at IS NULL`
    → 409 `{error:"anomalies_not_reviewed", …}`; otherwise proceeds as before.
  - Tenant-scoped throughout (`WHERE … AND tenant_id = $2` + RLS); no payslip/money change.
- `web/lib/api.ts` + `web/app/(app)/payroll/page.tsx`
  - “ตรวจความผิดปกติ” card in the run detail: severity badges, counts, employee/code/message
    rows, and a `review-anomalies` button. Approve is disabled (with a hint) for pending runs
    that have unreviewed anomalies. Reloads anomalies after review.
- `web/e2e/app.spec.ts`: payroll test now reviews the anomaly (SSO cap) before approving; the
  already-expanded payslip assertion no longer re-toggles the row.

## Verification

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | ✅ clean |
| `cargo clippy -p talentos-payroll-runner -p talentos-hr-runner -p talentos-gateway --all-targets -- -D warnings` | ✅ clean |
| Focused tests (`talentos-payroll-runner::tests::anomaly_review` + gateway `parse_anomaly_array`) | ✅ 5/5 (recalc clears review, sso_cap recorded, tenant scope, flatten/default severity, null/empty) |
| `talentos-payroll-runner` full suite | ✅ 20 passed (execute_run/loans) |
| `talentos-gateway` unit tests | ✅ 62 passed |
| `npm run build` | ✅ built |
| Playwright e2e | ✅ 44/44 passed |

Test env: DB `loans-postgres-1` (fresh `talentos_e2e`, migrated via gateway `migrate`), gateway
`:18469`, web `:3155`, `PII_ENCRYPTION_KEY` set, `SETUP_TOKEN=e2e-setup-token`, empty DB + `/setup`
(no seed). Servers killed after run.

## Concerns / risks

- No `query!`/`query_as!` changes, so `cargo sqlx prepare` was not required; all new SQL uses
  dynamic `sqlx::query`/`query_scalar`.
- The approve button is only disabled once the anomalies have been loaded (row expanded); until
  then it is enabled but the backend still 409s on approve, so the gate is enforced server-side.
- The review endpoint requires `payroll_run.approve`, matching the plan; GET anomalies uses the
  same permission (not `calculate`).
- `anomaly_reviewed_by` is a nullable FK to `users(id)` (like `created_by`); a deleted reviewer
  would need an explicit decision (users are deactivated, not deleted, today).

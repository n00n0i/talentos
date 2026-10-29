# Approval SLA Escalation — Task 1 Report

**Status:** Completed
**Commit:** `150ac70` (`feat/sla`)
**Report path:** `.superpowers/sdd/2026-10-29-approval-sla/task-1-report.md`

## What changed

- `migrations/20261118000001_approval_sla.sql`
  - `tenant_payroll_settings.approval_sla_hours INT NOT NULL DEFAULT 48 CHECK (0..720)`.
  - `leave_approval_steps.escalated_at TIMESTAMPTZ NULL` + escalation lookup index.
  - Applied via psql to `loans-postgres-1` and recorded in `_sqlx_migrations` (checksum = SHA-384 of the file, matching `sqlx::migrate`).
- `crates/domain/talentos-payroll/src/settings.rs`
  - `PayrollSettings.approval_sla_hours` (`serde` default 48), included in `standard()`, `is_valid()` (0..720), and a unit test for bounds.
- `crates/services/talentos-hr-runner/src/escalation.rs` (new)
  - `escalate_overdue(pool, tenant, limit)`: reads SLA (default 48, 0 = no-op); selects pending steps older than SLA with `escalated_at IS NULL`, joined to leave_requests/employees/leave_types (label_th resolved tenant-first then global); enqueues outbox rows (email always, LINE when `line_user_id`) for active `hr_admin` users; stamps `escalated_at = now()` in the same tx. Idempotent per step.
  - Reuses `notify::insert_notification` (made `pub(crate)`).
- `crates/api/gateway/src/main.rs`
  - `run_escalation_sweep` + a 15-min ticker: lists tenants via `auth_pool`, sweeps each tenant, logs counts, no panic.
- `crates/services/talentos-payroll-runner/src/lib.rs`
  - `execute_run` settings query now reads `approval_sla_hours` (no chain/balance change).
- `crates/api/gateway/src/handlers/settings.rs`
  - GET/PUT payroll include `approval_sla_hours`; PUT validation message updated.
- `web/app/(app)/settings/page.tsx`
  - Numeric input `approval-sla-hours` (0..720) with Thai hint “0 = ปิด”; wired into DEFAULTS, load, and save.

## Test summary

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p talentos-hr-runner -p talentos-payroll-runner -p talentos-gateway -p talentos-payroll --all-targets -- -D warnings` | clean |
| `cargo test -p talentos-hr-runner` (DB-backed, incl. 6 escalation) | all pass |
| `cargo test -p talentos-payroll` (settings bounds) | 5 passed |
| `cargo test -p talentos-gateway` | 54 passed |
| `cargo test -p talentos-payroll-runner` | all pass (see note) |
| `npm run build` (web) | built |
| Playwright e2e (21 tests; gateway 18359 / web 3144, re-seeded `seed-e2e.sql`) | 21 passed |

Escalation DB tests: overdue step → one notification per active HR admin (email×2 + LINE×1) and `escalated_at` set; second run no duplicate; fresh/decided step ignored; SLA 0 disables; tenant scope; DB CHECK rejects 721.

## Concerns

- No `query!`/`query_as!` macros added (all runtime `sqlx::query`/`query_scalar`), so `cargo sqlx prepare` was not required.
- `talentos-payroll-runner`'s `stuck_run_is_recovered_and_completed` fails if `DATABASE_AUTH_URL` is unset (recovery sweep falls back to the RLS-enforcing app role and finds nothing). With the usual env (`DATABASE_AUTH_URL=talentos_auth`) it passes — an environment prerequisite, not a regression.
- E2E requires `PII_ENCRYPTION_KEY` set (existing 2FA test); re-ran with a generated key.
- Gateway/web e2e servers were stopped after the run.

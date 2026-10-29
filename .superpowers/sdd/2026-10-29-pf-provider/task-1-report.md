# PF Remittance Provider Integration — Report

**Status:** Completed
**Commit:** (see git)
**Report path:** `.superpowers/sdd/2026-10-29-pf-provider/task-1-report.md`

## What changed

- `migrations/20261123000001_pf_remittances.sql`
  - `pf_remittances(id BIGSERIAL, tenant_id, payroll_run_id, status pending/sent/failed, attempts INT, last_error TEXT, payload JSONB, sent_at, created_at)`, `UNIQUE (tenant_id, payroll_run_id)`, RLS FORCE with `tenant_id = current_tenant_id()` isolation policy.
  - Applied via psql (owner) and recorded in `_sqlx_migrations` (sha384 `a86688ef…`).
- `crates/services/talentos-hr-runner/src/pf.rs`
  - Added `PfError::{NotApproved, RunNotFound}` and `PfRemittancePayload`/`PfRemittanceMember`.
  - `build_remittance_payload(conn, run_id)`: only `approved` runs; includes only payslips with `pf_employee > 0 OR pf_employer > 0` (code, name, employee/employer/total); period + totals.
- `crates/api/gateway/src/handlers/pf.rs` (new)
  - `POST /api/v1/payroll/runs/:id/pf-remittance/send` (`payroll_run.export_report`).
  - `PF_API_BASE`+`PF_API_TOKEN` required → else 503 `pf_provider_not_configured`.
  - 409 when a `sent` remittance exists; retry allowed after `failed`.
  - Upserts `pf_remittances` (pending, attempts+1), POSTs `{base}/remittances` (bearer + JSON payload, 8s timeout); marks sent/failed, `last_error` truncated to 500 chars.
  - Returns `{status, attempts, sent_at, last_error}`.
- `crates/api/gateway/src/router.rs`, `handlers/mod.rs`, `Cargo.toml` (added `thiserror` workspace dep): route + module wiring.
- `web/app/(app)/payroll/page.tsx`: “ส่ง PF” button (`data-testid="send-pf"`) on approved runs + status badge (`data-testid="pf-status"`); 503/error surfaced via existing `handleApiError`.
- `.env.example` + `docker-compose.yml`: documented `PF_API_BASE`/`PF_API_TOKEN`.

## Verification

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | ✅ clean |
| `cargo clippy -p talentos-gateway -p talentos-hr-runner --all-targets -- -D warnings` | ✅ clean |
| Focused tests (`handlers::pf`, hr-runner `pf::tests`) | ✅ 6 passed (path/header/body, sent, failure+retry, 409 sent, non-approved, tenant scope) |
| 503 unconfigured (live gateway curl) | ✅ `pf_provider_not_configured` |
| `npm run build` | ✅ built |
| Playwright e2e (`E2E_BASE_URL=http://127.0.0.1:3149`) | ✅ 21/21 passed |

Test environment: DB `loans-postgres-1`; gateway `:18409`; web `:3149`; `PII_ENCRYPTION_KEY` set; `seed-e2e.sql` re-seeded. Servers killed after run.

## Concerns / risks

- Provider field mapping (fund codes, member IDs) is intentionally deferred to a future per-provider adapter; the payload is provider-agnostic JSON (amounts serialized as strings via `rust_decimal` `serde-with-str`).
- No `query!` macro changes were introduced, so `cargo sqlx prepare` was not needed.
- The “ส่ง PF” UI is additive (no dedicated Playwright test added, keeping the suite at 21); its 503/status behaviour is covered by the gateway stub/DB tests and a live curl of the 503 path.
- `reqwest::Client` is constructed per request in the handler (matches the single-shot nature of the endpoint); a shared client is a trivial future optimization.

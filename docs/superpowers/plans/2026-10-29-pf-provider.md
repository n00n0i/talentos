# PF Remittance Provider Integration Plan

**Goal:** Send an approved run's PF remittance to a configured provider endpoint, recording success/failure; provider-agnostic JSON with a stub-testable adapter.

## Schema
`pf_remittances(id BIGSERIAL, tenant_id, payroll_run_id UNIQUE, status pending/sent/failed, attempts INT, last_error TEXT, payload JSONB, sent_at, created_at)`, RLS FORCE.

## Config
`PF_API_BASE` + `PF_API_TOKEN`; missing → 503 `pf_provider_not_configured`. POST `{base}/remittances` with bearer token and JSON payload; 8s timeout.

## Behaviour
- `POST /api/v1/payroll/runs/:id/pf-remittance/send` (`payroll_run.export_report`): approved runs only; build payload from payslips with `pf_employee/pf_employer > 0` (employee code, name, employee/employer/total, period); insert/update `pf_remittances`; send; status sent/failed + attempts + truncated error.
- Re-send allowed only after `failed` (409 when sent); tenant scoped.
- No payroll math change; provider field mapping is a future per-provider adapter (documented).
- `/payroll` approved run: “ส่ง PF” button + status; 503 message when unconfigured.

## Tests
- Unit/stub HTTP: request path/header/body; sent status; failure records last_error and allows retry; 409 when already sent; unconfigured 503.
- DB: payload only PF members/approved run; tenant scope.

## Gates
fmt/clippy/tests/build/Playwright 21/21.

# Payroll Anomaly Review Plan

**Goal:** Block approval until HR reviews a run’s payroll anomalies (warnings/errors) and records who reviewed them.

## Schema
`payroll_runs.anomaly_reviewed_at TIMESTAMPTZ`, `anomaly_reviewed_by UUID`.

## Behaviour
- `GET /api/v1/payroll/runs/:id/anomalies` (`payroll_run.approve`): flatten `payslips.anomalies` JSON into rows `{employee_code, employee_name, code, severity, message}`, plus counts by severity.
- `POST /api/v1/payroll/runs/:id/anomalies/review` (`payroll_run.approve`): only `pending_approval`; set reviewer/time; idempotent; 409 otherwise.
- `POST /payroll/runs/:id/approve`: if any anomaly rows exist and `anomaly_reviewed_at IS NULL` → 409 `anomalies_not_reviewed`; otherwise approve as today (re-review required if the run is recalculated after review? clear review on recalculation).
- Payroll runner: when a run transitions to `pending_approval` (recalc), clear `anomaly_reviewed_at/by`.
- No payslip/money change.

## UI
`/payroll` run detail: “ตรวจความผิดปกติ” card listing rows (severity badges), counts, review button (`review-anomalies`); approve button disabled/hinted until reviewed when anomalies exist.

## Tests
Flatten/count, review state, approve blocked without review, allowed after, 409 on draft, review cleared on recalculation, tenant scope.

## Gates
fmt/clippy/tests/build/Playwright 44/44 (anomaly UI may be empty; keep suite green).

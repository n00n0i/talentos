# Bank Payout Export & Mark Paid Plan

**Goal:** Generate a configurable bank payout CSV for an approved payroll run, then mark the run paid/completed and record the date/reference.

## Schema/Templates
- `payroll_runs.paid_at TIMESTAMPTZ`, `payment_reference TEXT`.
- `form_templates.form_type` adds `bank_payout`; global default config:
  columns `employee_code, employee_name, bank_name, bank_account, amount` with sources `employee_code/employee_name/bank_name/bank_account/net`.
- Allowed sources add `bank_name`, `bank_account`, `net`; `bank_account` decrypts via `PiiCipher` (legacy plaintext passes through).

## Endpoints
- `GET /api/v1/payroll/runs/:id/bank-payout.csv` (`payroll_run.export_report`): approved runs only; columns/order from the active `bank_payout` template; CSV-safe; deterministic; filename includes run id.
- `POST /api/v1/payroll/runs/:id/mark-paid` (`payroll_run.approve`) `{reference?}`: `approved → completed` only; set `paid_at=now()`, reference; 409 otherwise. Additive response.
- `/forms` supports `bank_payout` editing.
- `/payroll` approved run: “ดาวน์โหลดไฟล์โอน” + “ทำเครื่องหมายจ่ายแล้ว” button with optional reference; completed runs show paid date/reference.

## Rules
- Missing bank account emits an empty cell (CSV stays parseable); the UI/README warns to verify before uploading to a bank.
- No bank-specific layout is hardcoded; tenants adjust the template (the real bank format is Phase 0 input).
- Mark-paid does not change payslips/totals.
- Existing state transitions unaffected; GL/PF exports remain available after completion? Keep exports allowed for approved/completed (document).

## Tests
Template validation; approved-only; custom columns/order; bank account decrypt; mark-paid state/409/paid_at; tenant scope.

## Gates
fmt/clippy/tests/build/Playwright 45/45.

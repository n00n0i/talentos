# Bank Payout Export & Mark Paid — Report

**Status:** Completed
**Commit:** `2454178` (feat/bank)
**Report path:** `.superpowers/sdd/2026-10-30-bank-payout/task-1-report.md`

## What changed

- `migrations/20261126000001_bank_payout.sql`
  - `payroll_runs.paid_at TIMESTAMPTZ`, `payment_reference TEXT`.
  - `form_templates.form_type` CHECK extended with `bank_payout`; global default template
    (columns `employee_code/employee_name/bank_name/bank_account/amount`, sources
    `employee_code/employee_name/bank_name/bank_account/net`).
  - Applied via psql and recorded in `_sqlx_migrations` (sha384 `9d566277…`).
- `crates/services/talentos-hr-runner/src/forms.rs`
  - `FORM_TYPE_BANK_PAYOUT`; `bank_name`/`bank_account`/`net` added to `ALLOWED_SOURCES`;
    `bank_payout` validated as a column form.
  - `export_bank_payout_csv(conn, run_id, pii)`: approved-only, columns/order from the
    active template, deterministic CSV-safe, filename `bank-payout-<run>-<start>-<end>.csv`,
    no metadata/totals rows (banks parse directly), `bank_account` decrypted via `PiiCipher`
    (legacy plaintext passes through), missing account → empty cell.
- `crates/api/gateway`
  - `GET /api/v1/payroll/runs/:id/bank-payout.csv` (`payroll_run.export_report`).
  - `POST /api/v1/payroll/runs/:id/mark-paid` (`payroll_run.approve`) `{reference?}`:
    `approved → completed` only (sets `paid_at=now()`, `payment_reference`); 409 otherwise;
    response `{status, paid_at, reference}`.
- `crates/api/graphql/src/lib.rs`: `PayrollRun` gains `paidAt`/`paymentReference`
  (active `lib.rs` QueryRoot; `sqlx::query`, so no `sqlx prepare` needed).
- `web`: `/payroll` approved run shows “ดาวน์โหลดไฟล์โอน” + “ทำเครื่องหมายจ่ายแล้ว”
  (reference prompt); completed runs show paid date/reference. `/forms` adds the
  `bank_payout` option + `bank_name`/`bank_account`/`net` sources.
- `web/e2e/app.spec.ts` test 3 extended: bank-payout CSV body (headers `employee_code`/`amount`)
  + mark-paid → “จ่ายแล้ว” + reference.

## Verification

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | ✅ clean |
| `cargo clippy -p talentos-hr-runner -p talentos-gateway -p talentos-graphql --all-targets -- -D warnings` | ✅ clean |
| Focused DB tests (`talentos-hr-runner::tests::forms`) | ✅ 15/15 (bank payout columns/order, decrypt, legacy plaintext, approved-only, mark-paid/409) |
| `npm run build` | ✅ built |
| Playwright e2e (44 tests) | ✅ 44/44 passed |

Test env: DB `loans-postgres-1` (fresh `talentos_e2e` database, migrated via gateway
`migrate`), gateway `:18459`, web `:3154`, `PII_ENCRYPTION_KEY` set, `SETUP_TOKEN=e2e-setup-token`,
empty DB + `/setup` (no seed). Servers killed after run.

## Concerns / risks

- The e2e “download” assertion reads the CSV body via fetch rather than a browser download
  event: the Next `/api/gw` proxy forwards `content-disposition` only for PDF, not CSV, so a
  browser click renders the CSV inline instead of downloading.
- Bank account column is `employees.bank_account_number` (source name is `bank_account`);
  the global default labels are the literal English headers (`employee_code`, `amount`, …)
  as the plan specified — tenants adjust the template for their bank's real format.
- No `query!`/`query_as!` changes, so `cargo sqlx prepare` was not required.
- `mark-paid` leaves payslips/totals and GL/PF exports untouched (exports remain available for
  `approved`; a `completed` run no longer matches the `approved`-only export guard).

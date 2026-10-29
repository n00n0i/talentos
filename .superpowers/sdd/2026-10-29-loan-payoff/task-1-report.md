# Loan Payoff — Task 1 Report

**Status:** Completed
**Branch:** `feat/loan-payoff`
**Report path:** `.superpowers/sdd/2026-10-29-loan-payoff/task-1-report.md`

## What changed

- `migrations/20261120000001_loan_payments.sql`
  - `loan_payments(id BIGSERIAL, tenant_id, loan_id FK→employee_loans ON DELETE CASCADE, amount NUMERIC(12,2) CHECK (amount > 0), paid_at TIMESTAMPTZ DEFAULT now(), note, created_by FK→users)`.
  - `ENABLE` + `FORCE` RLS with `tenant_id = current_tenant_id()` policy; index by `loan_id`.
  - Applied via `sqlx migrate run` (owner) to `loans-postgres-1`; recorded in `_sqlx_migrations`.
- `crates/services/talentos-hr-runner/src/loans.rs` (new)
  - `payoff(pool, tenant, caller, loan_id, amount, note)` → `Result<(), PayoffError>`: rejects `amount <= 0` and `amount > balance` (`InvalidAmount`), non-active loans (`NotActive`), missing loan (`NotFound`); locks the row `FOR UPDATE`, inserts the payment, reduces `balance`, closes at zero (`CASE WHEN balance <= $1 THEN 'closed'`). Tenant scoped via `begin_tenant`.
  - `list_payments(pool, tenant, loan_id)` → newest-first `PaymentView` rows.
  - Exported from `lib.rs`.
- `crates/api/gateway/src/handlers/loans.rs` + `router.rs`
  - `POST /api/v1/loans/:id/payoff` (`employee.update`) — maps `NotFound`→404, `NotActive`/`InvalidAmount`→409, then re-fetches the updated loan.
  - `GET /api/v1/loans/:id/payments` (`employee.read_all`) additive.
  - Added `paid_amount` to `LoanDto` (all list/me/fetch responses), computed as `principal − balance` (total repaid, incl. payroll installments — no extra query).

## UI

- `web/app/(app)/loans/page.tsx`
  - “จ่ายปิด/จ่ายบางส่วน” button (`data-testid="payoff-loan"`) per active row → `prompt` amount (defaults to balance), posts payoff, success notice, reload.
  - Added “ชำระแล้ว” column showing `paid_amount`.

## Out of scope (documented)

- Interest/amortization.
- Reversing a payroll installment (needs a payroll storno design to avoid stale payslips).

## Tests

- `crates/services/talentos-hr-runner/tests/loans.rs` (6 DB tests): partial payoff reduces balance + records payment, full payoff closes, over-balance rejected (balance never negative), zero rejected, closed loan rejected, tenant scope blocks cross-tenant payoff (and hides payments).

## Gate results

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p talentos-hr-runner -p talentos-gateway --all-targets -- -D warnings` | clean |
| `cargo test -p talentos-hr-runner` (DB-backed, app role) | all pass (incl. 6 loans) |
| `cargo test -p talentos-gateway` | 54 passed, 0 failed |
| `npm run build` (web) | built |
| Playwright (21 tests; gateway 18379 / web 3146, re-seeded `seed-e2e.sql`) | 21 passed, 0 failed |

## Concerns

- No `query!`/`query_as!` macros changed (all runtime `sqlx::query`), so `cargo sqlx prepare` was not needed.
- `paid_amount` is total repaid (`principal − balance`), which includes payroll installments — not just manual payoffs. This is the cheapest correct value and matches the plan's "paid amount/remaining" intent; change to `SUM(loan_payments)` only if manual-only reporting is wanted later.
- 2FA e2e test requires `PII_ENCRYPTION_KEY` set on the gateway or it fails (pre-existing, unrelated). Ran the suite with a fresh 64-hex key.
- Gateway (18379) and web (3146) e2e servers were stopped after the run.

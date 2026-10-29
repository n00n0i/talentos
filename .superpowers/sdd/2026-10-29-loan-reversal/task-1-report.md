# Loan Installment Reversal — Task 1 Report

**Status:** Completed
**Branch:** `feat/loan-rev`
**Commit:** `bbf7ea5`
**Report path:** `.superpowers/sdd/2026-10-29-loan-reversal/task-1-report.md`

## What changed

- `migrations/20261121000001_loan_reversal.sql`
  - `loan_installments.reversed_at TIMESTAMPTZ NULL`, `reversed_by UUID NULL` (plain UUID, no FK so deleting a user never cascades into audit history).
  - Drops the unconditional `UNIQUE(loan_id, payroll_run_id)` constraint and adds a partial unique index `loan_installments_active_unique (loan_id, payroll_run_id) WHERE reversed_at IS NULL`.
  - Applied to `loans-postgres-1` via psql (as the plan instructed), not `sqlx migrate run`.

- `crates/services/talentos-hr-runner/src/loans.rs` (+ `lib.rs` exports)
  - `list_installments(pool, tenant, loan_id)` → newest-first `InstallmentView` (amount, created_at, reversed_at, run status, period).
  - `reverse_installment(pool, tenant, caller, loan_id, run_id)` → `Result<(), ReverseError>`: locks the loan and installment `FOR UPDATE`; rejects missing loan/installment (`NotFound`), already-reversed (`AlreadyReversed`), and non-editable run states (`InvalidRunState`; only `draft|calculating|pending_approval` allowed); sets `reversed_at/by`, restores `balance = LEAST(balance + amount, principal)`, reopens `closed → active`.

- `crates/api/gateway/src/handlers/loans.rs` + `router.rs`
  - `GET /api/v1/loans/:id/installments` (`employee.read_all`).
  - `POST /api/v1/loans/:id/installments/:run_id/reverse` (`employee.update`) — maps `NotFound`→404, `AlreadyReversed`→409, `InvalidRunState`→409, then re-fetches the loan.

- `crates/services/talentos-payroll-runner/src/lib.rs`
  - The `ON CONFLICT (loan_id, payroll_run_id) DO NOTHING` target was updated to `ON CONFLICT (loan_id, payroll_run_id) WHERE reversed_at IS NULL DO NOTHING`. **This one-line change is required** (see Concerns): with a partial arbiter index, Postgres rejects a bare column-list `ON CONFLICT` with "no unique or exclusion constraint matching the ON CONFLICT specification".

- `crates/services/talentos-hr-runner/tests/common/mod.rs`
  - Added `DELETE FROM payroll_runs` to `cleanup`. The new reverse tests create `payroll_runs`; without this the `DELETE FROM users`/`entities` would hit FK violations, abort the cleanup transaction, and leak tenants/installments (pre-existing latent bug, triggered by the new fixtures).

## UI

- `web/app/(app)/loans/page.tsx`
  - Per-row "ดูงวด/ซ่อนงวด" toggle (`data-testid="toggle-installments"`) → expandable installments table (period, run id, amount, run status).
  - "กลับรายการ" (`data-testid="reverse-installment"`) shown only for non-reversed rows whose run status is `draft|calculating|pending_approval`; confirm prompt; success notice "กลับรายการแล้ว — ต้องคำนวณรอบเงินเดือนใหม่".

## Tests

- `crates/services/talentos-hr-runner/tests/loans.rs` (5 new DB tests): reverse pending restores balance + reopens closed loan (and marks installment reversed), approved run → `InvalidRunState`, duplicate → `AlreadyReversed`, cap at principal, tenant scope → `NotFound`.
- `crates/services/talentos-payroll-runner/tests/loans.rs` (1 new DB test): recalc after reversal creates a fresh installment and deducts exactly once (2 rows: 1 reversed + 1 fresh; net balance unchanged).

## Gate results

| Gate | Result |
|------|--------|
| `cargo fmt -p …-hr-runner -p …-payroll-runner -p …-gateway -- --check` | clean |
| `cargo clippy` (3 crates, `--all-targets -- -D warnings`) | clean |
| `cargo test -p talentos-hr-runner --test loans` (DB, app role) | 11 passed |
| `cargo test -p talentos-payroll-runner --test loans` (DB, app role) | 5 passed |
| `cargo test -p talentos-gateway` | 54 passed |
| `npm run build` (web) | built |
| Playwright (21 tests; gateway 18389 / web 3147, re-seeded `seed-e2e.sql`) | 21 passed |

## Concerns

- The plan said "no payroll-runner code change", but that is incorrect: the partial unique index forces the `ON CONFLICT` target to carry the `WHERE reversed_at IS NULL` predicate, otherwise every loan deduction errors. I made that one-line change and documented the recalc flow in the doc comment.
- Migration applied via `psql` and therefore **not recorded in `_sqlx_migrations`**. A later `sqlx migrate run` will try to re-apply `20261121000001` and fail (duplicate column). Recommend recording it (or re-running via `sqlx migrate run` against a clean DB) before any future migrate. Flagging per the plan's explicit "apply via psql".
- `reversed_by` is a plain UUID (no FK), deviating from the `created_by REFERENCES users(id)` pattern, so user deletion can't break reversal history.
- No `query!`/`query_as!` macros touched (all runtime `sqlx::query`), so `cargo sqlx prepare` was not needed.
- Gateway (18389) and web (3147) e2e servers were stopped after the run.

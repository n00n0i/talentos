# Loan Interest / Amortization — Task 1 Report

**Status:** Completed
**Branch:** `feat/loan-interest` (merged to `master` as `bfb3523`)
**Commit:** `8aad43e` — `feat(loans): interest amortization and GL split`
**Report path:** `.superpowers/sdd/2026-10-29-loan-interest/task-1-report.md`

## What changed

### Schema — `migrations/20261122000001_loan_interest.sql`
- `employee_loans.interest_rate NUMERIC(5,4) NOT NULL DEFAULT 0 CHECK (0..0.10)`.
- `loan_installments.principal_amount`, `interest_amount NUMERIC(12,2) NOT NULL DEFAULT 0`.
- `payslips.loan_principal`, `loan_interest NUMERIC(12,2) NOT NULL DEFAULT 0` (kept `loan_deduction` = total).
- Applied to `loans-postgres-1` via psql **and** recorded in `_sqlx_migrations` with the correct sha384 checksum, so `sqlx migrate` stays consistent (unlike the earlier loan-reversal migration, which was applied psql-only).

### Payroll runner — `crates/services/talentos-payroll-runner/src/lib.rs`
- `ActiveLoan` now carries `principal`, `interest`, and `total` (was a single `amount`).
- New pure `amortize(balance, rate, installment) -> (principal, interest)` implementing the plan's math: interest `min(round(balance*rate,2), installment)`; final payoff when `balance + interest <= installment` (principal = full balance); principal capped at balance. Zero rate degenerates to the old `min(installment, balance)` behavior.
- Payslip write persists `loan_principal`/`loan_interest` alongside `loan_deduction`; DAG net still uses the total.
- `apply_loan_deduction` reduces `balance` by **principal** only and stores `principal_amount`/`interest_amount` on the installment.

### Domain — `talentos-payroll`
- `EmployeePayrollInput`/`EmployeePayrollOutput` gained `loan_principal`/`loan_interest` (serde-default zero).
- `render.rs` renders “หักเงินกู้ (เงินต้น/ดอกเบี้ย)” with a principal/interest breakdown when interest is present; plain “หักเงินกู้” for zero-rate loans.

### Reversal — `crates/services/talentos-hr-runner/src/loans.rs`
- `reverse_installment` restores **principal only** (interest written off), via the installment's `principal_amount`. Manual payoff unchanged (reduces balance only). `InstallmentView` now exposes `principal_amount`/`interest_amount`.

### GL — `crates/services/talentos-hr-runner/src/reports.rs`
- Credit `2230` (configured `gl_loan_account`) = `loan_principal`, credit hardcoded `4900 รายได้ดอกเบี้ย` = `loan_interest`. Total credits unchanged vs. `loan_deduction`, so the journal stays balanced.

### Gateway — `crates/api/gateway/src/handlers/loans.rs`
- `LoanBody.interest_rate` (`#[serde(default)]`), validated `0..0.10`; creation rejects a non-amortizing installment (`installment <= principal * rate`) with **422**. `LoanDto` + installments DTO expose `interest_rate`/`principal_amount`/`interest_amount`.

### UI — `web/app/(app)/loans/page.tsx`
- Interest-rate (%) field `data-testid="loan-interest-rate"` (posted as fraction); table shows rate; installments sub-table shows principal/interest split.

## Tests

- `talentos-payroll` (unit): 27 passed (incl. new render split tests).
- `talentos-payroll-runner --lib` (unit): 4 passed (`amortize` zero-rate / split / payoff / no-negative).
- `talentos-payroll-runner --test loans` (DB): 8 passed (new interest split, final payoff with interest, multi-run amortization).
- `talentos-hr-runner --test loans` (DB): 12 passed (new reversal-restores-principal-only).
- `talentos-hr-runner --test reports` (DB): 14 passed (new GL principal/interest split).
- `cargo check --workspace --all-targets`: clean.

## Gate results

| Gate | Result |
|------|--------|
| `cargo fmt --all` | clean |
| `cargo clippy` (payroll / payroll-runner / hr-runner / gateway, `--all-targets -- -D warnings`) | clean |
| focused tests (payroll, payroll-runner, hr-runner; app role) | all pass |
| `cargo sqlx prepare --workspace -- --all-targets` | refreshed (two `SELECT * FROM payslips` `query!` caches) |
| `npm run build` (web) | built |
| Playwright (21 tests; gateway 18399 / web 3148, re-seeded `seed-e2e.sql`) | **21 passed** |

## Concerns

- The GL interest income account `4900` is hardcoded (not tenant-configurable), matching the plan's “credit 4900 รายได้ดอกเบี้ย”. If tenants later need to override it, add a `gl_interest_account` setting alongside the existing seven `gl_*_account` columns.
- Reversal restores principal only; the accrued interest is written off and never added back to the balance (as decided in the plan).
- `cargo sqlx prepare` was required because the `SELECT * FROM payslips` `query!` macros in `talentos-tools` pick up the two new payslip columns (offline cache refreshed).
- Playwright is flaky when the web is served with `next start` (the build uses `output: "standalone"`, and `next start` warns it does not support standalone). A clean 21/21 requires `next dev` (used for the final run). Unrelated to this change.

## Out of scope

- No interest recalculation on manual payoff (documented: payoff reduces balance only).
- No amortization schedule / interest disclosure to the employee beyond the payslip split.

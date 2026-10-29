# Loan Installment Reversal Plan

**Goal:** HR can reverse a payroll loan installment before the run is approved, restoring the loan balance; recalculating the run then re-deducts correctly.

## Schema
- `loan_installments.reversed_at TIMESTAMPTZ NULL`, `reversed_by UUID NULL`.
- Replace the unconditional unique `(loan_id, payroll_run_id)` with a partial unique index `WHERE reversed_at IS NULL`, so a recalculated run can create a fresh installment after a reversal.

## Rules
- `POST /api/v1/loans/:id/installments/:run_id/reverse` (`employee.update`): installment must belong to the tenant/loan, not reversed; payroll run status must be `draft|calculating|pending_approval` (never `approved|completed`).
- Lock the loan and installment; set `reversed_at/by`; add the amount back to `balance` (never above principal); reopen a `closed` loan (`active`).
- The payslip for that run keeps the old deduction until HR recalculates; the API response/UI must state “ต้องคำนวณรอบเงินเดือนใหม่”.
- Reversing an approved/completed run → 409. Duplicate reversal → 409.
- `GET /api/v1/loans/:id/installments` lists run id, amount, date, reversed status.

## UI
`/loans`: expandable installments list with “กลับรายการ” for non-reversed, non-approved-run rows; confirm; success notice with recalc hint.

## Tests
Reverse pending run restores balance/reopens; approved run 409; duplicate 409; amount cap; recalc after reversal creates a new installment and deducts once; tenant scope.

## Gates
fmt/clippy/tests/build/Playwright 21/21.

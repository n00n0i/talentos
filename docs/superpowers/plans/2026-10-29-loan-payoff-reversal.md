# Loan Payoff Plan

**Goal:** HR can record an early or partial loan payoff; the balance reduces and closes at zero.

## Schema
`loan_payments(id BIGSERIAL, tenant_id, loan_id, amount NUMERIC(12,2) >0, paid_at, note, created_by)`, RLS FORCE.

## Rules
- `POST /api/v1/loans/:id/payoff {amount, note?}` (employee.update): active loan only; `0 < amount <= balance`; reduce balance, insert payment, close at zero; 409 otherwise; tenant scoped.
- No payroll change; future payroll installments use the reduced balance.
- List payments on `GET /loans` (additive) or a `GET /loans/:id/payments`.
- UI `/loans`: “จ่ายปิด/จ่ายบางส่วน” button with amount prompt; show paid amount/remaining.

## Out of scope (documented)
- Interest/amortization.
- Reversing an installment after payroll: it needs a payroll adjustment design (storno) to avoid stale payslips; deferred rather than half-built.

## Tests
Partial/full/over-balance/zero; closed loan rejects; tenant scope; balance/payment rows.

## Gates
fmt/clippy/tests/build/Playwright 21/21.

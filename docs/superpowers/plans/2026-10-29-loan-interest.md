# Loan Interest / Amortization Plan

**Goal:** A loan can carry a monthly interest rate; each payroll installment splits into interest and principal, the balance amortizes, and the GL separates principal repayment from interest income.

## Schema
- `employee_loans.interest_rate NUMERIC(5,4) NOT NULL DEFAULT 0` (monthly fraction, 0–0.10).
- `loan_installments.principal_amount`, `interest_amount` NUMERIC(12,2) NOT NULL DEFAULT 0.
- `payslips.loan_principal`, `loan_interest` NUMERIC(12,2) NOT NULL DEFAULT 0 (keep `loan_deduction` = total for compatibility).

## Math
- Per run: `interest = min(round(balance * rate, 2), installment)`; `principal = installment - interest`; if `balance + interest <= installment`, pay off: `interest = round(balance*rate,2)`, `principal = balance`, total = balance + interest.
- Cap principal at balance; balance never negative; total deduction = principal + interest.
- New loan validation: `installment > principal * rate` (so it amortizes), rate 0–0.10.
- Manual payoff continues to reduce balance only (no interest recalculation); documented.
- Reversal restores principal only (interest is not refunded? decide: restore full installment total? Keep consistent with payslip: restore both principal and interest to the loan balance? Interest isn't principal; for reversal before approval, the whole deduction never happened, so restore principal to balance and write off the interest. Document.)
- DAG net = gross - sso - tax - pf - loan_deduction + reimbursement.

## GL
- Credit `2230` = principal_amount; credit `4900 รายได้ดอกเบี้ย` = interest_amount per department; total credits unchanged vs. loan_deduction.

## Tests
Zero rate unchanged; interest split; final payoff; balance amortizes; no negative; creation rejects non-amortizing installment; GL split balances; reversal restores principal.

## Gates
fmt/clippy/tests/build/Playwright 21/21.

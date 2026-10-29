-- Loan interest / amortization.
-- A loan carries a monthly interest rate; each payroll installment splits into
-- interest and principal, the balance amortizes, and the GL separates principal
-- repayment from interest income.

ALTER TABLE employee_loans
    ADD COLUMN interest_rate NUMERIC(5,4) NOT NULL DEFAULT 0
    CHECK (interest_rate >= 0 AND interest_rate <= 0.10);

ALTER TABLE loan_installments
    ADD COLUMN principal_amount NUMERIC(12,2) NOT NULL DEFAULT 0,
    ADD COLUMN interest_amount NUMERIC(12,2) NOT NULL DEFAULT 0;

ALTER TABLE payslips
    ADD COLUMN loan_principal NUMERIC(12,2) NOT NULL DEFAULT 0,
    ADD COLUMN loan_interest NUMERIC(12,2) NOT NULL DEFAULT 0;

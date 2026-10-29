-- Manual loan payoffs: HR records an early or partial repayment that reduces
-- the loan balance (and closes it at zero). Interest/amortization and the
-- reversal of payroll installments are out of scope.

CREATE TABLE loan_payments (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL,
    loan_id UUID NOT NULL REFERENCES employee_loans(id) ON DELETE CASCADE,
    amount NUMERIC(12,2) NOT NULL CHECK (amount > 0),
    paid_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    note TEXT,
    created_by UUID NOT NULL REFERENCES users(id)
);

CREATE INDEX idx_loan_payments_loan ON loan_payments(loan_id);

ALTER TABLE loan_payments ENABLE ROW LEVEL SECURITY;
ALTER TABLE loan_payments FORCE ROW LEVEL SECURITY;

CREATE POLICY loan_payments_isolation ON loan_payments USING (tenant_id = current_tenant_id());

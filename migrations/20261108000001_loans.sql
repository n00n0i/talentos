-- Employee loans / salary advances.
-- One active loan per employee; each payroll run deducts one installment.

CREATE TABLE employee_loans (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    principal NUMERIC(12,2) NOT NULL CHECK (principal > 0),
    balance NUMERIC(12,2) NOT NULL CHECK (balance >= 0),
    installment_amount NUMERIC(12,2) NOT NULL CHECK (installment_amount > 0),
    start_date DATE NOT NULL,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','closed','cancelled')),
    note TEXT,
    created_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_employee_loans_one_active ON employee_loans(employee_id) WHERE status = 'active';
CREATE INDEX idx_employee_loans_tenant_employee ON employee_loans(tenant_id, employee_id);

CREATE TABLE loan_installments (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL,
    loan_id UUID NOT NULL REFERENCES employee_loans(id) ON DELETE CASCADE,
    payroll_run_id UUID NOT NULL REFERENCES payroll_runs(id) ON DELETE CASCADE,
    amount NUMERIC(12,2) NOT NULL CHECK (amount > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(loan_id, payroll_run_id)
);

CREATE INDEX idx_loan_installments_loan ON loan_installments(loan_id);
CREATE INDEX idx_loan_installments_run ON loan_installments(payroll_run_id);

ALTER TABLE payslips ADD COLUMN loan_deduction NUMERIC(12,2) NOT NULL DEFAULT 0;

ALTER TABLE employee_loans ENABLE ROW LEVEL SECURITY;
ALTER TABLE employee_loans FORCE ROW LEVEL SECURITY;
ALTER TABLE loan_installments ENABLE ROW LEVEL SECURITY;
ALTER TABLE loan_installments FORCE ROW LEVEL SECURITY;

CREATE POLICY employee_loans_isolation ON employee_loans USING (tenant_id = current_tenant_id());
CREATE POLICY loan_installments_isolation ON loan_installments USING (tenant_id = current_tenant_id());

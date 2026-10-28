-- Reimburse approved expense claims through the next payroll run.

ALTER TABLE payslips
    ADD COLUMN reimbursement NUMERIC(12,2) NOT NULL DEFAULT 0;

ALTER TABLE expense_claims
    ADD COLUMN payroll_run_id UUID REFERENCES payroll_runs(id) ON DELETE SET NULL,
    ADD COLUMN paid_at TIMESTAMPTZ;

-- Widen the status allowlist to include 'paid' once a claim is settled via payroll.
ALTER TABLE expense_claims
    DROP CONSTRAINT expense_claims_status_check,
    ADD CONSTRAINT expense_claims_status_check
        CHECK (status IN ('pending', 'approved', 'rejected', 'cancelled', 'paid'));

CREATE INDEX idx_expense_claims_payroll_run ON expense_claims(payroll_run_id);
CREATE INDEX idx_expense_claims_paid_lookup
    ON expense_claims(tenant_id, employee_id, expense_date, status, payroll_run_id)
    WHERE status = 'approved';

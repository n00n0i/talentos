CREATE TABLE payroll_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    entity_id UUID NOT NULL REFERENCES entities(id) ON DELETE RESTRICT,
    period_start DATE NOT NULL,
    period_end DATE NOT NULL,
    payment_date DATE NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','calculating','pending_approval','approved','rejected','disbursing','completed','failed')),
    total_employees INT NOT NULL DEFAULT 0,
    total_gross NUMERIC(14,2) NOT NULL DEFAULT 0,
    total_net NUMERIC(14,2) NOT NULL DEFAULT 0,
    total_tax NUMERIC(14,2) NOT NULL DEFAULT 0,
    total_sso NUMERIC(14,2) NOT NULL DEFAULT 0,
    total_pf NUMERIC(14,2) NOT NULL DEFAULT 0,
    workflow_id TEXT,
    created_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(tenant_id, entity_id, period_start, period_end)
);

CREATE TABLE payslips (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    payroll_run_id UUID NOT NULL REFERENCES payroll_runs(id) ON DELETE CASCADE,
    tenant_id UUID NOT NULL,
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    base_salary NUMERIC(12,2) NOT NULL,
    gross_earnings NUMERIC(12,2) NOT NULL,
    total_deductions NUMERIC(12,2) NOT NULL DEFAULT 0,
    net_pay NUMERIC(12,2) NOT NULL,
    tax_withheld NUMERIC(12,2) NOT NULL DEFAULT 0,
    sso_employee NUMERIC(12,2) NOT NULL DEFAULT 0,
    sso_employer NUMERIC(12,2) NOT NULL DEFAULT 0,
    pf_employee NUMERIC(12,2) NOT NULL DEFAULT 0,
    pf_employer NUMERIC(12,2) NOT NULL DEFAULT 0,
    anomalies JSONB NOT NULL DEFAULT '[]',
    status TEXT NOT NULL DEFAULT 'calculated' CHECK (status IN ('calculated','approved','disbursed','rejected')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE payslip_lines (
    id BIGSERIAL PRIMARY KEY,
    payslip_id UUID NOT NULL REFERENCES payslips(id) ON DELETE CASCADE,
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    line_type TEXT NOT NULL CHECK (line_type IN ('earning','deduction','tax','contribution')),
    amount NUMERIC(12,2) NOT NULL,
    taxable BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE TABLE payroll_events (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL,
    payroll_run_id UUID NOT NULL REFERENCES payroll_runs(id) ON DELETE CASCADE,
    step_name TEXT NOT NULL,
    employee_id UUID,
    input JSONB NOT NULL,
    output JSONB NOT NULL,
    error JSONB,
    correlation_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_payslips_run ON payslips(payroll_run_id);
CREATE INDEX idx_payslips_employee ON payslips(employee_id);
CREATE INDEX idx_payslip_lines_payslip ON payslip_lines(payslip_id);
CREATE INDEX idx_payroll_events_run ON payroll_events(payroll_run_id);
CREATE INDEX idx_payroll_runs_tenant ON payroll_runs(tenant_id, period_start DESC);

ALTER TABLE payroll_runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE payroll_runs FORCE ROW LEVEL SECURITY;
ALTER TABLE payslips ENABLE ROW LEVEL SECURITY;
ALTER TABLE payslips FORCE ROW LEVEL SECURITY;
ALTER TABLE payslip_lines ENABLE ROW LEVEL SECURITY;
ALTER TABLE payslip_lines FORCE ROW LEVEL SECURITY;
ALTER TABLE payroll_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE payroll_events FORCE ROW LEVEL SECURITY;

CREATE POLICY payroll_runs_isolation ON payroll_runs USING (tenant_id = current_tenant_id());
CREATE POLICY payslips_isolation ON payslips USING (tenant_id = current_tenant_id());
CREATE POLICY payslip_lines_isolation ON payslip_lines USING (EXISTS (SELECT 1 FROM payslips p WHERE p.id = payslip_lines.payslip_id AND p.tenant_id = current_tenant_id()));
CREATE POLICY payroll_events_isolation ON payroll_events USING (tenant_id = current_tenant_id());
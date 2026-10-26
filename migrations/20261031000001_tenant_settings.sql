-- Tenant-editable payroll policy. Statutory rates (SSO, PND) stay in code: they
-- are law, not preference. These are the company's own policy numbers.
CREATE TABLE tenant_payroll_settings (
    tenant_id UUID PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    unpaid_leave_divisor NUMERIC(5,2) NOT NULL DEFAULT 30
        CHECK (unpaid_leave_divisor BETWEEN 1 AND 31),
    ot_weekday_rate NUMERIC(4,2) NOT NULL DEFAULT 1.50
        CHECK (ot_weekday_rate BETWEEN 1 AND 5),
    ot_weekend_rate NUMERIC(4,2) NOT NULL DEFAULT 2.00
        CHECK (ot_weekend_rate BETWEEN 1 AND 5),
    ot_holiday_rate NUMERIC(4,2) NOT NULL DEFAULT 3.00
        CHECK (ot_holiday_rate BETWEEN 1 AND 5),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL
);

ALTER TABLE tenant_payroll_settings ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenant_payroll_settings FORCE ROW LEVEL SECURITY;

CREATE POLICY tenant_payroll_settings_isolation ON tenant_payroll_settings
    USING (tenant_id = current_tenant_id());

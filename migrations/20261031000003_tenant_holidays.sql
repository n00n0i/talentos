-- Company holiday calendar. Dates are tenant-managed because the Thai cabinet
-- announces public holidays yearly; nothing is hardcoded in the payroll code.
CREATE TABLE tenant_holidays (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    holiday_date DATE NOT NULL,
    name_th TEXT NOT NULL,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, holiday_date)
);

CREATE INDEX idx_tenant_holidays_date ON tenant_holidays(tenant_id, holiday_date);

ALTER TABLE tenant_holidays ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenant_holidays FORCE ROW LEVEL SECURITY;

CREATE POLICY tenant_holidays_isolation ON tenant_holidays
    USING (tenant_id = current_tenant_id());

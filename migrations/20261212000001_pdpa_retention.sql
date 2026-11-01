-- PDPA retention/anonymisation policy per tenant.
-- Defaults follow the practical policy: keep payroll/GL-related PII for 7 years
-- (legal obligation, never purged), purge operational data 12 months after
-- termination.
CREATE TABLE tenant_pdpa_policy (
    tenant_id UUID PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    payroll_retention_years INT NOT NULL DEFAULT 7
        CHECK (payroll_retention_years >= 0 AND payroll_retention_years <= 50),
    ops_retention_months INT NOT NULL DEFAULT 12
        CHECK (ops_retention_months >= 0 AND ops_retention_months <= 120),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE tenant_pdpa_policy ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenant_pdpa_policy FORCE ROW LEVEL SECURITY;

CREATE POLICY tenant_pdpa_policy_isolation ON tenant_pdpa_policy
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

-- Flag on employees whose PII has been anonymised/deleted (DSAR).
-- Payroll rows are retained by law; this records that identity data was wiped.
ALTER TABLE employees
    ADD COLUMN IF NOT EXISTS pii_purged_at TIMESTAMPTZ;

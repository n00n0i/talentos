-- Conditional leave approval rules: route a leave request to a specific
-- approval chain by leave type and/or minimum days. Falls back to the tenant
-- default chain (tenant_payroll_settings.leave_approval_chain) when no rule
-- matches. Rules are snapshotted into leave_approval_steps at request creation.
CREATE TABLE leave_approval_rules (
    id         BIGSERIAL PRIMARY KEY,
    tenant_id  UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    leave_type TEXT,
    min_days   NUMERIC(6,2),
    chain      TEXT[] NOT NULL,
    priority   INT NOT NULL DEFAULT 0,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (leave_type IS NULL OR leave_type IN ('annual', 'sick', 'personal', 'maternity', 'paternity', 'bereavement', 'unpaid')),
    CHECK (min_days IS NULL OR (min_days > 0 AND min_days <= 365))
);

-- Lookups in create_request order by type-specific, min_days-specific,
-- priority desc, id.
CREATE INDEX idx_leave_approval_rules_lookup
    ON leave_approval_rules (tenant_id, leave_type, min_days, priority DESC, id);

ALTER TABLE leave_approval_rules ENABLE ROW LEVEL SECURITY;
ALTER TABLE leave_approval_rules FORCE ROW LEVEL SECURITY;

CREATE POLICY leave_approval_rules_isolation ON leave_approval_rules
    USING (tenant_id = current_tenant_id());

-- PF remittance delivery records. One row per payroll run; a failed send can be
-- retried in place, so the UNIQUE key keeps the row (and its attempts) stable.
CREATE TABLE pf_remittances (
    id             BIGSERIAL PRIMARY KEY,
    tenant_id      UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    payroll_run_id UUID NOT NULL REFERENCES payroll_runs(id) ON DELETE CASCADE,
    status         TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','sent','failed')),
    attempts       INT NOT NULL DEFAULT 0,
    last_error     TEXT,
    payload        JSONB NOT NULL,
    sent_at        TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, payroll_run_id)
);

CREATE INDEX idx_pf_remittances_tenant ON pf_remittances(tenant_id, created_at DESC);

ALTER TABLE pf_remittances ENABLE ROW LEVEL SECURITY;
ALTER TABLE pf_remittances FORCE ROW LEVEL SECURITY;

CREATE POLICY pf_remittances_isolation ON pf_remittances
    USING (tenant_id = current_tenant_id());

-- Provident fund contribution rates, effective-dated per employee.
-- Payroll picks the latest rate whose effective_from <= the run's period_end;
-- a missing row means the statutory default 5% (0.05).
CREATE TABLE pf_rate_history (
    id             BIGSERIAL PRIMARY KEY,
    tenant_id      UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id    UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    effective_from DATE NOT NULL,
    rate           NUMERIC(5,4) NOT NULL CHECK (rate BETWEEN 0 AND 0.15),
    created_by     UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, employee_id, effective_from)
);

CREATE INDEX idx_pf_rate_history_employee ON pf_rate_history(employee_id, effective_from DESC);

ALTER TABLE pf_rate_history ENABLE ROW LEVEL SECURITY;
ALTER TABLE pf_rate_history FORCE ROW LEVEL SECURITY;

CREATE POLICY pf_rate_history_isolation ON pf_rate_history
    USING (tenant_id = current_tenant_id());

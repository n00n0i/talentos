-- Per-employee performance goals, optionally tied to a review cycle.
CREATE TABLE performance_goals (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    cycle_id    UUID REFERENCES performance_cycles(id) ON DELETE SET NULL,
    title       TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    description TEXT CHECK (description IS NULL OR char_length(description) <= 1000),
    target      TEXT CHECK (target IS NULL OR char_length(target) <= 200),
    progress    INT NOT NULL DEFAULT 0 CHECK (progress BETWEEN 0 AND 100),
    status      TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','achieved','cancelled')),
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_goals_employee ON performance_goals(tenant_id, employee_id, status);
CREATE INDEX idx_goals_cycle ON performance_goals(tenant_id, cycle_id);

ALTER TABLE performance_goals ENABLE ROW LEVEL SECURITY;
ALTER TABLE performance_goals FORCE ROW LEVEL SECURITY;

CREATE POLICY performance_goals_isolation ON performance_goals
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

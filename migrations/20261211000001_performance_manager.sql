-- Manager workflow: reporting line on employees, goal check-ins, calibrations.
ALTER TABLE employees
    ADD COLUMN manager_id UUID REFERENCES employees(id) ON DELETE SET NULL;

CREATE INDEX idx_employees_manager ON employees(tenant_id, manager_id);

CREATE TABLE performance_goal_check_ins (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id  UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    goal_id    UUID NOT NULL REFERENCES performance_goals(id) ON DELETE CASCADE,
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    manager_id UUID REFERENCES employees(id) ON DELETE SET NULL,
    note       TEXT NOT NULL CHECK (char_length(note) BETWEEN 1 AND 2000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_goal_check_ins_goal ON performance_goal_check_ins(tenant_id, goal_id, created_at);

CREATE TABLE performance_calibrations (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id        UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    cycle_id         UUID NOT NULL REFERENCES performance_cycles(id) ON DELETE CASCADE,
    employee_id      UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    manager_id       UUID REFERENCES employees(id) ON DELETE SET NULL,
    calibrated_rating INT NOT NULL CHECK (calibrated_rating BETWEEN 1 AND 5),
    notes            TEXT CHECK (notes IS NULL OR char_length(notes) <= 2000),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, cycle_id, employee_id)
);

CREATE INDEX idx_calibrations_cycle ON performance_calibrations(tenant_id, cycle_id);
CREATE INDEX idx_calibrations_manager ON performance_calibrations(tenant_id, manager_id);

ALTER TABLE performance_goal_check_ins ENABLE ROW LEVEL SECURITY;
ALTER TABLE performance_goal_check_ins FORCE ROW LEVEL SECURITY;
ALTER TABLE performance_calibrations ENABLE ROW LEVEL SECURITY;
ALTER TABLE performance_calibrations FORCE ROW LEVEL SECURITY;

CREATE POLICY performance_goal_check_ins_isolation ON performance_goal_check_ins
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());
CREATE POLICY performance_calibrations_isolation ON performance_calibrations
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

-- Per-employee onboarding checklist. The standard set is inserted by the app;
-- HR can add ad-hoc tasks. One task per title keeps the standard run idempotent.
CREATE TABLE employee_onboarding_tasks (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id  UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    title        TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    notes        TEXT CHECK (notes IS NULL OR char_length(notes) <= 500),
    due_date     DATE,
    status       TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','done')),
    completed_at TIMESTAMPTZ,
    completed_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, employee_id, title)
);

CREATE INDEX idx_onboarding_employee ON employee_onboarding_tasks(tenant_id, employee_id, status);

ALTER TABLE employee_onboarding_tasks ENABLE ROW LEVEL SECURITY;
ALTER TABLE employee_onboarding_tasks FORCE ROW LEVEL SECURITY;

CREATE POLICY onboarding_isolation ON employee_onboarding_tasks
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

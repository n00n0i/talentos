-- Employee training history with optional certificate expiry.
CREATE TABLE employee_training_records (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id  UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    title        TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    provider     TEXT CHECK (provider IS NULL OR char_length(provider) <= 200),
    completed_on DATE,
    expires_on   DATE,
    created_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (completed_on IS NULL OR expires_on IS NULL OR expires_on >= completed_on)
);

CREATE INDEX idx_training_employee ON employee_training_records(tenant_id, employee_id);
CREATE INDEX idx_training_expiry ON employee_training_records(tenant_id, expires_on)
    WHERE expires_on IS NOT NULL;

ALTER TABLE employee_training_records ENABLE ROW LEVEL SECURITY;
ALTER TABLE employee_training_records FORCE ROW LEVEL SECURITY;

CREATE POLICY training_isolation ON employee_training_records
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

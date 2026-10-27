-- OT (overtime) self-service requests.
CREATE TABLE ot_requests (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    entity_id    UUID NOT NULL REFERENCES entities(id) ON DELETE RESTRICT,
    employee_id  UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    work_date    DATE NOT NULL,
    minutes      INT NOT NULL CHECK (minutes BETWEEN 1 AND 720),
    reason       TEXT,
    status       TEXT NOT NULL DEFAULT 'pending'
                 CHECK (status IN ('pending', 'approved', 'rejected', 'cancelled')),
    requested_by UUID REFERENCES users(id) ON DELETE SET NULL,
    decided_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    decided_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_ot_requests_tenant_status ON ot_requests(tenant_id, status, created_at DESC);
CREATE INDEX idx_ot_requests_employee       ON ot_requests(employee_id, work_date DESC);

-- Only one live request per employee per work date: pending or approved.
CREATE UNIQUE INDEX idx_ot_requests_unique_live_date
    ON ot_requests (tenant_id, employee_id, work_date)
    WHERE status IN ('pending', 'approved');

ALTER TABLE ot_requests ENABLE ROW LEVEL SECURITY;
ALTER TABLE ot_requests FORCE ROW LEVEL SECURITY;

CREATE POLICY ot_requests_isolation ON ot_requests
    USING (tenant_id = current_tenant_id());

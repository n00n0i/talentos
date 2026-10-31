-- Immutable audit trail of candidate stage moves.
CREATE TABLE candidate_stage_events (
    id           BIGSERIAL PRIMARY KEY,
    tenant_id    UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    candidate_id UUID NOT NULL REFERENCES candidates(id) ON DELETE CASCADE,
    from_stage   TEXT,
    to_stage     TEXT NOT NULL,
    changed_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    changed_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_candidate_stage_events ON candidate_stage_events(tenant_id, candidate_id, changed_at);

ALTER TABLE candidate_stage_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE candidate_stage_events FORCE ROW LEVEL SECURITY;

CREATE POLICY candidate_stage_events_isolation ON candidate_stage_events
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

-- Candidate interview scheduling.
CREATE TABLE candidate_interviews (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id           UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    candidate_id        UUID NOT NULL REFERENCES candidates(id) ON DELETE CASCADE,
    scheduled_at        TIMESTAMPTZ NOT NULL,
    mode                TEXT NOT NULL CHECK (mode IN ('onsite','online','phone')),
    location            TEXT CHECK (location IS NULL OR char_length(location) <= 500),
    interviewer_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    status              TEXT NOT NULL DEFAULT 'scheduled'
                        CHECK (status IN ('scheduled','completed','cancelled')),
    notes               TEXT CHECK (notes IS NULL OR char_length(notes) <= 1000),
    created_by          UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_candidate_interviews_time ON candidate_interviews(tenant_id, scheduled_at);
CREATE INDEX idx_candidate_interviews_candidate ON candidate_interviews(tenant_id, candidate_id);

ALTER TABLE candidate_interviews ENABLE ROW LEVEL SECURITY;
ALTER TABLE candidate_interviews FORCE ROW LEVEL SECURITY;

CREATE POLICY candidate_interviews_isolation ON candidate_interviews
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

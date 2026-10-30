-- PDPA: record acknowledgement of the privacy notice per user and version.
-- Append-only history: a new notice version creates a new row.
CREATE TABLE privacy_consents (
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    version     TEXT NOT NULL,
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, user_id, version)
);

CREATE INDEX idx_privacy_consents_user ON privacy_consents(tenant_id, user_id);

ALTER TABLE privacy_consents ENABLE ROW LEVEL SECURITY;
ALTER TABLE privacy_consents FORCE ROW LEVEL SECURITY;

CREATE POLICY privacy_consents_isolation ON privacy_consents
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

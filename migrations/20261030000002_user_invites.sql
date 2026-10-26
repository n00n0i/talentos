-- Invitations. The token is stored hashed and is single-use; no email service is
-- required on-prem — HR copies the link to the new user.
CREATE TABLE user_invites (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    email TEXT NOT NULL,
    full_name_th TEXT,
    role TEXT NOT NULL,
    entity_id UUID REFERENCES entities(id) ON DELETE SET NULL,
    token_hash BYTEA NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_user_invites_tenant ON user_invites(tenant_id, created_at DESC);
CREATE INDEX idx_user_invites_open ON user_invites(expires_at) WHERE accepted_at IS NULL;

ALTER TABLE user_invites ENABLE ROW LEVEL SECURITY;
ALTER TABLE user_invites FORCE ROW LEVEL SECURITY;

CREATE POLICY user_invites_isolation ON user_invites
    USING (tenant_id = current_tenant_id());

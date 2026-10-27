-- HR-issued password reset links. Hashed, single-use, 1 hour. No email service
-- is required on-prem: the admin copies the link to the user.
CREATE TABLE password_resets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash BYTEA NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_password_resets_user ON password_resets(user_id, created_at DESC);
CREATE INDEX idx_password_resets_open ON password_resets(expires_at) WHERE used_at IS NULL;

ALTER TABLE password_resets ENABLE ROW LEVEL SECURITY;
ALTER TABLE password_resets FORCE ROW LEVEL SECURITY;

CREATE POLICY password_resets_isolation ON password_resets
    USING (tenant_id = current_tenant_id());

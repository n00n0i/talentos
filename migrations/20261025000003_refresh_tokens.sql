-- Refresh token store. BYTEA because handlers bind SHA-256 digests (32 bytes), not hex strings.
-- parent_hash enables family-walk revocation on reuse detection (spec §2, TC-18).
CREATE TABLE refresh_tokens (
    id BIGSERIAL PRIMARY KEY,
    token_hash BYTEA NOT NULL UNIQUE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    entity_id UUID REFERENCES entities(id) ON DELETE SET NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    parent_hash BYTEA REFERENCES refresh_tokens(token_hash),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ip_address INET,
    user_agent TEXT
);

CREATE INDEX idx_refresh_tokens_hash ON refresh_tokens(token_hash);
CREATE INDEX idx_refresh_tokens_user ON refresh_tokens(user_id) WHERE revoked_at IS NULL;
CREATE INDEX idx_refresh_tokens_expiry ON refresh_tokens(expires_at) WHERE revoked_at IS NULL;

ALTER TABLE refresh_tokens ENABLE ROW LEVEL SECURITY;
ALTER TABLE refresh_tokens FORCE ROW LEVEL SECURITY;

-- Bootstrap/login reads happen via BYPASSRLS role; normal traffic is tenant-scoped.
CREATE POLICY refresh_token_isolation ON refresh_tokens
    USING (tenant_id = current_tenant_id());
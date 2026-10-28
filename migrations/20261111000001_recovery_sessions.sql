-- Recovery codes for TOTP 2FA and active session listing from refresh_tokens.
CREATE TABLE IF NOT EXISTS totp_recovery_codes (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash BYTEA NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, code_hash)
);

CREATE INDEX IF NOT EXISTS idx_totp_recovery_codes_user_hash
    ON totp_recovery_codes(user_id, code_hash) WHERE used_at IS NULL;

ALTER TABLE totp_recovery_codes ENABLE ROW LEVEL SECURITY;
ALTER TABLE totp_recovery_codes FORCE ROW LEVEL SECURITY;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_policies
        WHERE policyname = 'totp_recovery_code_isolation'
          AND tablename = 'totp_recovery_codes'
    ) THEN
        CREATE POLICY totp_recovery_code_isolation ON totp_recovery_codes
            USING (tenant_id = current_tenant_id());
    END IF;
END
$$;

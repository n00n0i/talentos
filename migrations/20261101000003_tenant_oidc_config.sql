-- Per-tenant generic OIDC provider settings (SSO skeleton, handoff §8.1).
-- client_secret is encrypted at the app layer via PiiCipher (v1: hex blob).
-- Inert until an operator registers a client with a real provider.
CREATE TABLE tenant_oidc_config (
    tenant_id UUID PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    issuer TEXT NOT NULL,
    client_id TEXT NOT NULL,
    client_secret TEXT NOT NULL,
    allowed_domains TEXT[] NOT NULL DEFAULT '{}',
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE tenant_oidc_config ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenant_oidc_config FORCE ROW LEVEL SECURITY;

CREATE POLICY tenant_oidc_config_isolation ON tenant_oidc_config
    USING (tenant_id = current_tenant_id());

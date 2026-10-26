-- Tenant WASM plugins. The module bytes live with the tenant row so an on-prem
-- install needs no object store to run a plugin.
CREATE TABLE tenant_plugins (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    wasm         BYTEA NOT NULL CHECK (octet_length(wasm) > 8),
    capabilities TEXT[] NOT NULL DEFAULT '{}',
    enabled      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, name)
);

CREATE INDEX idx_tenant_plugins_tenant ON tenant_plugins(tenant_id) WHERE enabled;

ALTER TABLE tenant_plugins ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenant_plugins FORCE ROW LEVEL SECURITY;

CREATE POLICY tenant_plugins_isolation ON tenant_plugins
    USING (tenant_id = current_tenant_id());

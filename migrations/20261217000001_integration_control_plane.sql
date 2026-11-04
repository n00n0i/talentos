-- Integration Control Plane, slice 1: connector registry + connections.
--
-- Connector versions are content-immutable once published. The service also
-- refuses draft edits on published rows (409); this trigger is the database
-- backstop, since a superuser or a future writer could bypass the service.
CREATE TABLE integration_connectors (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    connector_id  TEXT NOT NULL,
    name          TEXT NOT NULL,
    version       TEXT NOT NULL,
    protocol      TEXT NOT NULL,
    manifest      JSONB NOT NULL,
    risk          TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'draft'
                  CHECK (status IN ('draft', 'published', 'disabled')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, connector_id, version)
);

CREATE INDEX idx_integration_connectors_tenant_status
    ON integration_connectors(tenant_id, status);

CREATE TABLE integration_connections (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id         UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    connector_id      TEXT NOT NULL,
    connector_version TEXT NOT NULL,
    name              TEXT NOT NULL,
    endpoint          TEXT NOT NULL,
    -- OpenBao path only; secret values never live in Postgres (ADR-003).
    secret_ref        TEXT,
    state             TEXT NOT NULL DEFAULT 'active'
                      CHECK (state IN ('active', 'disabled')),
    last_test         JSONB,
    last_verified_at  TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, name),
    FOREIGN KEY (tenant_id, connector_id, connector_version)
        REFERENCES integration_connectors(tenant_id, connector_id, version)
        ON DELETE RESTRICT
);

CREATE INDEX idx_integration_connections_connector
    ON integration_connections(tenant_id, connector_id, connector_version);

CREATE OR REPLACE FUNCTION integration_connectors_published_immutable()
RETURNS TRIGGER AS $$
BEGIN
    IF OLD.status = 'published' AND (
           NEW.connector_id IS DISTINCT FROM OLD.connector_id
        OR NEW.name         IS DISTINCT FROM OLD.name
        OR NEW.version      IS DISTINCT FROM OLD.version
        OR NEW.protocol     IS DISTINCT FROM OLD.protocol
        OR NEW.manifest     IS DISTINCT FROM OLD.manifest
        OR NEW.risk         IS DISTINCT FROM OLD.risk
    ) THEN
        RAISE EXCEPTION 'published connector versions are immutable'
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER integration_connectors_immutable
    BEFORE UPDATE ON integration_connectors
    FOR EACH ROW EXECUTE FUNCTION integration_connectors_published_immutable();

ALTER TABLE integration_connectors ENABLE ROW LEVEL SECURITY;
ALTER TABLE integration_connectors FORCE ROW LEVEL SECURITY;
CREATE POLICY integration_connectors_isolation ON integration_connectors
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

ALTER TABLE integration_connections ENABLE ROW LEVEL SECURITY;
ALTER TABLE integration_connections FORCE ROW LEVEL SECURITY;
CREATE POLICY integration_connections_isolation ON integration_connections
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

-- Organizational knowledge graph + AI settings.
--
-- Ruling: `tenant_api_keys` (phase 4) stays unused; `ai_settings` supersedes it
-- because it also holds provider/model/base_url/skills.
--
-- Nodes are unique per (tenant, class, key) so a rebuild is idempotent.
-- Edges are unique per (tenant, from, relation, to).
CREATE TABLE kg_nodes (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    class       TEXT NOT NULL,
    key         TEXT NOT NULL,
    label_th    TEXT NOT NULL,
    label_en    TEXT,
    properties  JSONB NOT NULL DEFAULT '{}',
    embedding   vector(1536),
    source      TEXT NOT NULL DEFAULT 'sync',
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, class, key)
);

CREATE TABLE kg_edges (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    from_id     UUID NOT NULL REFERENCES kg_nodes(id) ON DELETE CASCADE,
    to_id       UUID NOT NULL REFERENCES kg_nodes(id) ON DELETE CASCADE,
    relation    TEXT NOT NULL,
    properties  JSONB NOT NULL DEFAULT '{}',
    UNIQUE (tenant_id, from_id, relation, to_id)
);

CREATE TABLE ai_settings (
    tenant_id    UUID PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    provider     TEXT NOT NULL DEFAULT 'huakati',
    base_url     TEXT,
    model        TEXT NOT NULL DEFAULT 'qwen2.5-72b-instruct',
    api_key_enc  BYTEA,
    mcp_enabled  BOOLEAN NOT NULL DEFAULT true,
    skills       JSONB NOT NULL DEFAULT '[]',
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_kg_nodes_tenant_class ON kg_nodes(tenant_id, class);
CREATE INDEX idx_kg_nodes_properties ON kg_nodes USING gin (properties);
CREATE INDEX idx_kg_nodes_embedding ON kg_nodes USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);
CREATE INDEX idx_kg_edges_tenant_relation ON kg_edges(tenant_id, relation);
CREATE INDEX idx_kg_edges_properties ON kg_edges USING gin (properties);

ALTER TABLE kg_nodes ENABLE ROW LEVEL SECURITY;
ALTER TABLE kg_nodes FORCE ROW LEVEL SECURITY;
CREATE POLICY kg_nodes_isolation ON kg_nodes
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

ALTER TABLE kg_edges ENABLE ROW LEVEL SECURITY;
ALTER TABLE kg_edges FORCE ROW LEVEL SECURITY;
CREATE POLICY kg_edges_isolation ON kg_edges
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

ALTER TABLE ai_settings ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_settings FORCE ROW LEVEL SECURITY;
CREATE POLICY ai_settings_isolation ON ai_settings
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

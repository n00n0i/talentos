-- Agent memory, pgvector-backed. One table, four behavioural layers:
--   working    short-lived task context (TTL default applied by the app)
--   episodic   what happened: runs, decisions, outcomes
--   semantic   durable facts about the tenant (policies, constants)
--   procedural how-to: runbooks and learned procedures
-- Layer is a tag, not a separate table: one retrieval path means ranking and
-- RLS cannot drift between layers.
CREATE TABLE ai_memory (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    entity_id UUID REFERENCES entities(id) ON DELETE SET NULL,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    layer TEXT NOT NULL CHECK (layer IN ('working', 'episodic', 'semantic', 'procedural')),
    -- Conversation / task / agent id for working memory; '' for durable layers.
    scope_key TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL CHECK (char_length(content) BETWEEN 1 AND 8000),
    metadata JSONB NOT NULL DEFAULT '{}',
    importance NUMERIC(3,2) NOT NULL DEFAULT 0.50 CHECK (importance BETWEEN 0 AND 1),
    embedding vector(1536),
    -- shortcut: 'simple' config, not real Thai tokenization; swap when Thai keyword recall matters
    tsv tsvector GENERATED ALWAYS AS (to_tsvector('simple', content)) STORED,
    access_count INT NOT NULL DEFAULT 0,
    last_accessed TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_ai_memory_tenant_layer ON ai_memory(tenant_id, layer, created_at DESC);
CREATE INDEX idx_ai_memory_scope ON ai_memory(tenant_id, scope_key) WHERE scope_key <> '';
CREATE INDEX idx_ai_memory_embedding ON ai_memory USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);
CREATE INDEX idx_ai_memory_tsv ON ai_memory USING GIN (tsv);
CREATE INDEX idx_ai_memory_expiry ON ai_memory(expires_at) WHERE expires_at IS NOT NULL;

ALTER TABLE ai_memory ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_memory FORCE ROW LEVEL SECURITY;

CREATE POLICY ai_memory_isolation ON ai_memory
    USING (tenant_id = current_tenant_id());

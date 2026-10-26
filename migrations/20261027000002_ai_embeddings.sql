-- AI Embeddings Schema
-- pgvector extension + document store + chunk embeddings

-- Enable pgvector extension
CREATE EXTENSION IF NOT EXISTS vector;

-- RAG document store
CREATE TABLE ai_documents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    entity_id UUID REFERENCES entities(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL,
    doc_type TEXT NOT NULL CHECK (doc_type IN ('policy', 'regulation', 'payslip', 'api_spec', 'handbook')),
    source_path TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(tenant_id, content_hash)
);

-- Document chunks with embeddings
CREATE TABLE ai_document_chunks (
    id BIGSERIAL PRIMARY KEY,
    document_id UUID NOT NULL REFERENCES ai_documents(id) ON DELETE CASCADE,
    chunk_index INT NOT NULL,
    content TEXT NOT NULL,
    embedding vector(1536),
    word_count INT NOT NULL,
    tsv tsvector GENERATED ALWAYS AS (to_tsvector('thai', content)) STORED,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(document_id, chunk_index)
);

-- Indexes for search
CREATE INDEX idx_ai_chunks_embedding ON ai_document_chunks USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);
CREATE INDEX idx_ai_chunks_doc ON ai_document_chunks(document_id);
CREATE INDEX idx_ai_chunks_tsv ON ai_document_chunks USING GIN (tsv);
CREATE INDEX idx_ai_documents_tenant_type ON ai_documents(tenant_id, doc_type);
CREATE INDEX idx_ai_documents_hash ON ai_documents(content_hash);
CREATE INDEX idx_ai_documents_entity ON ai_documents(entity_id) WHERE entity_id IS NOT NULL;

-- Enable RLS
ALTER TABLE ai_documents ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_documents FORCE ROW LEVEL SECURITY;
ALTER TABLE ai_document_chunks ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_document_chunks FORCE ROW LEVEL SECURITY;

-- RLS policies
CREATE POLICY ai_documents_isolation ON ai_documents USING (tenant_id = current_tenant_id());
CREATE POLICY ai_document_chunks_isolation ON ai_document_chunks USING (EXISTS (
    SELECT 1 FROM ai_documents d WHERE d.id = ai_document_chunks.document_id AND d.tenant_id = current_tenant_id()
));
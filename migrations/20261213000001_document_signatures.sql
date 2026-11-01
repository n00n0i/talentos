-- Digital acknowledgement signatures: internal hash-of-doc + typed name + time.
-- An external provider can be plugged in later via E_SIGNATURE_PROVIDER.
CREATE TABLE document_signatures (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    document_type   TEXT NOT NULL CHECK (document_type IN ('payslip','offer')),
    document_id     UUID NOT NULL,
    signer_user_id  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    typed_name      TEXT NOT NULL CHECK (char_length(typed_name) BETWEEN 1 AND 200),
    doc_hash_sha256 TEXT NOT NULL CHECK (char_length(doc_hash_sha256) = 64),
    signed_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    provider        TEXT NOT NULL DEFAULT 'internal',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, document_type, document_id, signer_user_id)
);

CREATE INDEX idx_document_signatures_doc
    ON document_signatures (tenant_id, document_type, document_id, signed_at DESC);
CREATE INDEX idx_document_signatures_tenant ON document_signatures (tenant_id);

ALTER TABLE document_signatures ENABLE ROW LEVEL SECURITY;
ALTER TABLE document_signatures FORCE ROW LEVEL SECURITY;
CREATE POLICY document_signatures_isolation ON document_signatures
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

-- Link payslips to their signature (optional: null when unsigned).
ALTER TABLE payslips
    ADD COLUMN signature_id UUID REFERENCES document_signatures(id) ON DELETE SET NULL;

-- Link offer-letter records to their signature (optional: null when unsigned).
ALTER TABLE candidate_offers
    ADD COLUMN signature_id UUID REFERENCES document_signatures(id) ON DELETE SET NULL;

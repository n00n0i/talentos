-- Employee documents (contracts, permits, visas) with optional expiry tracking
-- and one stored file per document.
CREATE TABLE employee_documents (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id  UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    doc_type     TEXT NOT NULL CHECK (doc_type IN
                     ('contract','work_permit','visa','id_card','certificate','other')),
    title        TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    doc_number   TEXT CHECK (doc_number IS NULL OR char_length(doc_number) <= 100),
    issued_date  DATE,
    expiry_date  DATE,
    file_path    TEXT,
    file_type    TEXT,
    uploaded_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (issued_date IS NULL OR expiry_date IS NULL OR expiry_date >= issued_date)
);

CREATE INDEX idx_employee_documents_employee ON employee_documents(tenant_id, employee_id);
CREATE INDEX idx_employee_documents_expiry ON employee_documents(tenant_id, expiry_date)
    WHERE expiry_date IS NOT NULL;

ALTER TABLE employee_documents ENABLE ROW LEVEL SECURITY;
ALTER TABLE employee_documents FORCE ROW LEVEL SECURITY;

CREATE POLICY employee_documents_isolation ON employee_documents
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

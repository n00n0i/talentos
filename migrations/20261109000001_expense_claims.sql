-- Expense claims with receipt storage.
-- Reimbursement through payroll is deferred to a future slice.

CREATE TABLE expense_claims (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    entity_id     UUID NOT NULL REFERENCES entities(id) ON DELETE RESTRICT,
    employee_id   UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    category      TEXT NOT NULL CHECK (category IN ('travel', 'meal', 'supplies', 'other')),
    amount        NUMERIC(12,2) NOT NULL CHECK (amount > 0),
    expense_date  DATE NOT NULL,
    description   TEXT CHECK (description IS NULL OR char_length(description) <= 500),
    receipt_path  TEXT,
    receipt_type  TEXT,
    status        TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'approved', 'rejected', 'cancelled')),
    decided_by    UUID REFERENCES users(id) ON DELETE SET NULL,
    decided_at    TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_expense_claims_tenant_status ON expense_claims(tenant_id, status, created_at DESC);
CREATE INDEX idx_expense_claims_employee      ON expense_claims(employee_id, expense_date DESC);

ALTER TABLE expense_claims ENABLE ROW LEVEL SECURITY;
ALTER TABLE expense_claims FORCE ROW LEVEL SECURITY;

CREATE POLICY expense_claims_isolation ON expense_claims
    USING (tenant_id = current_tenant_id());

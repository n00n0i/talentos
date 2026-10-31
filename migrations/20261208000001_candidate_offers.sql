-- Job offers per candidate with a generated PDF letter.
CREATE TABLE candidate_offers (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id      UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    candidate_id   UUID NOT NULL REFERENCES candidates(id) ON DELETE CASCADE,
    position_title TEXT NOT NULL CHECK (char_length(position_title) BETWEEN 1 AND 200),
    salary         NUMERIC(12,2) NOT NULL CHECK (salary > 0),
    start_date     DATE NOT NULL,
    notes          TEXT CHECK (notes IS NULL OR char_length(notes) <= 2000),
    status         TEXT NOT NULL DEFAULT 'draft'
                   CHECK (status IN ('draft','sent','accepted','declined')),
    created_by     UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_candidate_offers ON candidate_offers(tenant_id, candidate_id, created_at DESC);

ALTER TABLE candidate_offers ENABLE ROW LEVEL SECURITY;
ALTER TABLE candidate_offers FORCE ROW LEVEL SECURITY;

CREATE POLICY candidate_offers_isolation ON candidate_offers
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

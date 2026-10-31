-- Performance review cycles and one review per employee per cycle.
CREATE TABLE performance_cycles (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name         TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 200),
    period_start DATE NOT NULL,
    period_end   DATE NOT NULL,
    status       TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','active','closed')),
    created_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (period_end >= period_start)
);

CREATE INDEX idx_performance_cycles_status ON performance_cycles(tenant_id, status, created_at DESC);

CREATE TABLE performance_reviews (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id        UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    cycle_id         UUID NOT NULL REFERENCES performance_cycles(id) ON DELETE CASCADE,
    employee_id      UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    reviewer_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    status           TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','submitted')),
    rating           INT CHECK (rating IS NULL OR rating BETWEEN 1 AND 5),
    strengths        TEXT CHECK (strengths IS NULL OR char_length(strengths) <= 2000),
    improvements     TEXT CHECK (improvements IS NULL OR char_length(improvements) <= 2000),
    submitted_at     TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (cycle_id, employee_id)
);

CREATE INDEX idx_performance_reviews_cycle ON performance_reviews(tenant_id, cycle_id, status);

ALTER TABLE performance_cycles ENABLE ROW LEVEL SECURITY;
ALTER TABLE performance_cycles FORCE ROW LEVEL SECURITY;
ALTER TABLE performance_reviews ENABLE ROW LEVEL SECURITY;
ALTER TABLE performance_reviews FORCE ROW LEVEL SECURITY;

CREATE POLICY performance_cycles_isolation ON performance_cycles
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());
CREATE POLICY performance_reviews_isolation ON performance_reviews
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

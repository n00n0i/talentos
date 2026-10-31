-- Recruitment: job openings and candidates with a simple stage pipeline.
CREATE TABLE job_openings (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    title           TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    department      TEXT CHECK (department IS NULL OR char_length(department) <= 100),
    employment_type TEXT NOT NULL DEFAULT 'full_time'
                    CHECK (employment_type IN ('full_time','part_time','contract','intern')),
    headcount       INT NOT NULL DEFAULT 1 CHECK (headcount BETWEEN 1 AND 100),
    status          TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','closed')),
    description     TEXT CHECK (description IS NULL OR char_length(description) <= 2000),
    created_by      UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_job_openings_status ON job_openings(tenant_id, status, created_at DESC);

CREATE TABLE candidates (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id      UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    job_opening_id UUID NOT NULL REFERENCES job_openings(id) ON DELETE CASCADE,
    first_name     TEXT NOT NULL CHECK (char_length(first_name) BETWEEN 1 AND 100),
    last_name      TEXT NOT NULL CHECK (char_length(last_name) BETWEEN 1 AND 100),
    email          TEXT CHECK (email IS NULL OR char_length(email) <= 200),
    phone          TEXT CHECK (phone IS NULL OR char_length(phone) <= 30),
    stage          TEXT NOT NULL DEFAULT 'applied'
                   CHECK (stage IN ('applied','screening','interview','offer','hired','rejected')),
    source         TEXT CHECK (source IS NULL OR char_length(source) <= 100),
    note           TEXT CHECK (note IS NULL OR char_length(note) <= 2000),
    created_by     UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_candidates_job ON candidates(tenant_id, job_opening_id, stage);

ALTER TABLE job_openings ENABLE ROW LEVEL SECURITY;
ALTER TABLE job_openings FORCE ROW LEVEL SECURITY;
ALTER TABLE candidates ENABLE ROW LEVEL SECURITY;
ALTER TABLE candidates FORCE ROW LEVEL SECURITY;

CREATE POLICY job_openings_isolation ON job_openings
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());
CREATE POLICY candidates_isolation ON candidates
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

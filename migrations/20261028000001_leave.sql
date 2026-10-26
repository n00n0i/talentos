-- Leave management: types, balances, requests.
-- Leave request approval flows through Temporal signals in Phase 2+.

CREATE TABLE leave_types (
    id                SERIAL PRIMARY KEY,
    tenant_id         UUID REFERENCES tenants(id) ON DELETE CASCADE, -- NULL = global Thai statutory default
    name              TEXT NOT NULL,                                 -- annual | sick | personal | maternity | paternity | bereavement | unpaid
    label_th          TEXT NOT NULL,
    label_en          TEXT NOT NULL,
    max_days_per_year NUMERIC(6,2) NOT NULL,
    carry_forward     BOOLEAN NOT NULL DEFAULT FALSE
);

-- Thai statutory minimums (LPA Section 29/39 + SSO maternity rules)
INSERT INTO leave_types (tenant_id, name, label_th, label_en, max_days_per_year, carry_forward) VALUES
(NULL, 'annual',      'ลาพักร้อน',    'Annual Leave',      6.00,  TRUE),
(NULL, 'sick',        'ลาป่วย',       'Sick Leave',       30.00,  FALSE),
(NULL, 'personal',    'ลากิจ',        'Personal Leave',    3.00,  FALSE),
(NULL, 'maternity',   'ลาคลอด',       'Maternity Leave',  98.00,  FALSE),
(NULL, 'paternity',   'ลาบิดา',       'Paternity Leave',  15.00,  FALSE),
(NULL, 'bereavement', 'ลาศพ',         'Bereavement Leave', 3.00,  FALSE),
(NULL, 'unpaid',      'ลากิจไม่รับค่าจ้าง', 'Unpaid Leave',  0.00, FALSE);

CREATE TABLE leave_balances (
    id             BIGSERIAL PRIMARY KEY,
    tenant_id      UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id    UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    leave_type_id  INT NOT NULL REFERENCES leave_types(id) ON DELETE RESTRICT,
    year           INT NOT NULL,
    total_days     NUMERIC(6,2) NOT NULL DEFAULT 0,
    used_days      NUMERIC(6,2) NOT NULL DEFAULT 0,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(employee_id, leave_type_id, year)
);

CREATE TABLE leave_requests (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    entity_id    UUID NOT NULL REFERENCES entities(id) ON DELETE RESTRICT,
    employee_id  UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    leave_type   TEXT NOT NULL,
    start_date   DATE NOT NULL,
    end_date     DATE NOT NULL,
    days         NUMERIC(6,2) NOT NULL CHECK (days > 0),
    reason       TEXT,
    status       TEXT NOT NULL DEFAULT 'pending'
                 CHECK (status IN ('pending', 'approved', 'rejected', 'cancelled')),
    requested_by UUID REFERENCES users(id) ON DELETE SET NULL,
    approver_id  UUID REFERENCES users(id) ON DELETE SET NULL,
    approved_at  TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (end_date >= start_date)
);

CREATE INDEX idx_leave_requests_tenant_status ON leave_requests(tenant_id, status, created_at DESC);
CREATE INDEX idx_leave_requests_employee      ON leave_requests(employee_id, start_date DESC);
CREATE INDEX idx_leave_balances_employee      ON leave_balances(employee_id, year);

ALTER TABLE leave_types    ENABLE ROW LEVEL SECURITY;
ALTER TABLE leave_balances ENABLE ROW LEVEL SECURITY;
ALTER TABLE leave_requests ENABLE ROW LEVEL SECURITY;

ALTER TABLE leave_types    FORCE ROW LEVEL SECURITY;
ALTER TABLE leave_balances FORCE ROW LEVEL SECURITY;
ALTER TABLE leave_requests FORCE ROW LEVEL SECURITY;

-- Global defaults (tenant_id IS NULL) are visible to every tenant.
CREATE POLICY leave_types_isolation ON leave_types
    USING (tenant_id IS NULL OR tenant_id = current_tenant_id());

CREATE POLICY leave_balances_isolation ON leave_balances
    USING (tenant_id = current_tenant_id());

CREATE POLICY leave_requests_isolation ON leave_requests
    USING (tenant_id = current_tenant_id());

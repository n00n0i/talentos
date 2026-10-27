-- Attendance v2: shifts, assignments, punches, and day summaries.
-- Replaces the simpler attendance_records table with row-level tenant isolation.

-- Geofence for entity-level clock-in/out validation.
ALTER TABLE entities
    ADD COLUMN IF NOT EXISTS geofence_lat NUMERIC(9,6),
    ADD COLUMN IF NOT EXISTS geofence_lng NUMERIC(9,6),
    ADD COLUMN IF NOT EXISTS geofence_radius_m INT NOT NULL DEFAULT 200;

CREATE TABLE work_shifts (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    entity_id   UUID NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    start_time  TIME NOT NULL,
    end_time    TIME NOT NULL,
    is_active   BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (entity_id, name)
);

CREATE TABLE shift_assignments (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id     UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    shift_id        UUID NOT NULL REFERENCES work_shifts(id) ON DELETE CASCADE,
    effective_from  DATE NOT NULL,
    effective_until DATE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (employee_id, effective_from)
);

CREATE TABLE attendance_events (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('in', 'out')),
    source      TEXT NOT NULL CHECK (source IN ('mobile', 'web', 'device', 'manual', 'import')),
    occurred_at TIMESTAMPTZ NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_attendance_events_employee_occurred
    ON attendance_events(employee_id, occurred_at);

CREATE TABLE attendance_days (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id  UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    work_date    DATE NOT NULL,
    shift_id     UUID REFERENCES work_shifts(id) ON DELETE SET NULL,
    hours_worked NUMERIC(4,2) NOT NULL DEFAULT 8.00
        CHECK (hours_worked >= 0 AND hours_worked <= 24),
    ot_hours     NUMERIC(4,2) NOT NULL DEFAULT 0.00
        CHECK (ot_hours >= 0 AND ot_hours <= 12),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (employee_id, work_date)
);

ALTER TABLE work_shifts          ENABLE ROW LEVEL SECURITY;
ALTER TABLE shift_assignments    ENABLE ROW LEVEL SECURITY;
ALTER TABLE attendance_events    ENABLE ROW LEVEL SECURITY;
ALTER TABLE attendance_days      ENABLE ROW LEVEL SECURITY;

ALTER TABLE work_shifts          FORCE ROW LEVEL SECURITY;
ALTER TABLE shift_assignments    FORCE ROW LEVEL SECURITY;
ALTER TABLE attendance_events    FORCE ROW LEVEL SECURITY;
ALTER TABLE attendance_days      FORCE ROW LEVEL SECURITY;

CREATE POLICY work_shifts_isolation ON work_shifts
    USING (tenant_id = current_tenant_id());

CREATE POLICY shift_assignments_isolation ON shift_assignments
    USING (tenant_id = current_tenant_id());

CREATE POLICY attendance_events_isolation ON attendance_events
    USING (tenant_id = current_tenant_id());

CREATE POLICY attendance_days_isolation ON attendance_days
    USING (tenant_id = current_tenant_id());

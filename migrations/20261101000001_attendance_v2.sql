-- Attendance v2: shifts, assignments, punches, and day summaries.
-- Replaces the simpler attendance_records table with row-level tenant isolation.

-- Geofence for entity-level clock-in/out validation.
ALTER TABLE entities
    ADD COLUMN IF NOT EXISTS geofence_lat NUMERIC(9,6),
    ADD COLUMN IF NOT EXISTS geofence_lng NUMERIC(9,6),
    ADD COLUMN IF NOT EXISTS geofence_radius_m INT NOT NULL DEFAULT 200;

ALTER TABLE entities
    ADD CONSTRAINT entities_geofence_lat_check
        CHECK (geofence_lat IS NULL OR (geofence_lat >= -90 AND geofence_lat <= 90)),
    ADD CONSTRAINT entities_geofence_lng_check
        CHECK (geofence_lng IS NULL OR (geofence_lng >= -180 AND geofence_lng <= 180)),
    ADD CONSTRAINT entities_geofence_radius_m_check
        CHECK (geofence_radius_m >= 1 AND geofence_radius_m <= 5000);

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

CREATE INDEX idx_work_shifts_tenant ON work_shifts(tenant_id);

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

CREATE INDEX idx_shift_assignments_tenant ON shift_assignments(tenant_id);
CREATE INDEX idx_shift_assignments_shift  ON shift_assignments(shift_id);

CREATE TABLE attendance_events (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('in', 'out')),
    source      TEXT NOT NULL CHECK (source IN ('mobile', 'web', 'device', 'manual', 'import')),
    occurred_at TIMESTAMPTZ NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_attendance_events_tenant ON attendance_events(tenant_id);
CREATE INDEX idx_attendance_events_employee_occurred
    ON attendance_events(employee_id, occurred_at);

CREATE TABLE attendance_days (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id           UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id         UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    work_date           DATE NOT NULL,
    shift_id            UUID REFERENCES work_shifts(id) ON DELETE SET NULL,
    worked_minutes      INT NOT NULL DEFAULT 0 CHECK (worked_minutes >= 0),
    late_minutes        INT NOT NULL DEFAULT 0 CHECK (late_minutes >= 0),
    early_leave_minutes INT NOT NULL DEFAULT 0 CHECK (early_leave_minutes >= 0),
    ot_minutes          INT NOT NULL DEFAULT 0 CHECK (ot_minutes >= 0),
    ot_kind             TEXT NOT NULL DEFAULT 'weekday'
        CHECK (ot_kind IN ('weekday', 'weekend', 'holiday')),
    incomplete          BOOLEAN NOT NULL DEFAULT FALSE,
    status              TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'approved')),
    approved_by         UUID REFERENCES users(id) ON DELETE SET NULL,
    approved_at         TIMESTAMPTZ,
    computed_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (employee_id, work_date)
);

CREATE INDEX idx_attendance_days_tenant ON attendance_days(tenant_id);
CREATE INDEX idx_attendance_days_shift  ON attendance_days(shift_id);

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

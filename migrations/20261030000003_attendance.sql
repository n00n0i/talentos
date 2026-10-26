-- Attendance feeds payroll: OT hours by statutory rate class. Monthly salary is
-- not prorated by hours worked (paid leave is still paid); only approved unpaid
-- leave reduces gross.
CREATE TABLE attendance_records (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    work_date DATE NOT NULL,
    hours_worked NUMERIC(4,2) NOT NULL DEFAULT 8.00
        CHECK (hours_worked >= 0 AND hours_worked <= 24),
    ot_hours NUMERIC(4,2) NOT NULL DEFAULT 0.00
        CHECK (ot_hours >= 0 AND ot_hours <= 12),
    -- Weekday 1.5x, rest day 2x, public holiday 3x (LPA §61-62). The recorder
    -- chooses the class; the payroll DAG applies the multiplier.
    ot_kind TEXT NOT NULL DEFAULT 'weekday'
        CHECK (ot_kind IN ('weekday', 'weekend', 'holiday')),
    note TEXT,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (employee_id, work_date)
);

CREATE INDEX idx_attendance_tenant_date ON attendance_records(tenant_id, work_date DESC);
CREATE INDEX idx_attendance_employee_date ON attendance_records(employee_id, work_date);

ALTER TABLE attendance_records ENABLE ROW LEVEL SECURITY;
ALTER TABLE attendance_records FORCE ROW LEVEL SECURITY;

CREATE POLICY attendance_isolation ON attendance_records
    USING (tenant_id = current_tenant_id());

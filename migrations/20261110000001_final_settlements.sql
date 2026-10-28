-- Final settlement drafts for departing employees.
-- Tax withholding, 50 ทวิ/SSO forms and payroll run integration are out of scope.

CREATE TABLE final_settlements (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id           UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    employee_id         UUID NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    termination_date    DATE NOT NULL,
    unused_leave_days   INT NOT NULL CHECK (unused_leave_days >= 0),
    notice_days         INT NOT NULL CHECK (notice_days >= 0),
    include_severance   BOOLEAN NOT NULL,
    years_of_service    NUMERIC(8,4) NOT NULL,
    last_month_pay      NUMERIC(12,2) NOT NULL,
    leave_pay           NUMERIC(12,2) NOT NULL,
    notice_pay          NUMERIC(12,2) NOT NULL,
    severance_pay       NUMERIC(12,2) NOT NULL,
    total_gross         NUMERIC(12,2) NOT NULL,
    breakdown           JSONB NOT NULL DEFAULT '{}'::jsonb,
    status              TEXT NOT NULL DEFAULT 'draft'
                            CHECK (status IN ('draft', 'finalized', 'cancelled')),
    created_by          UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_final_settlements_tenant_created
    ON final_settlements (tenant_id, created_at DESC);
CREATE INDEX idx_final_settlements_employee
    ON final_settlements (tenant_id, employee_id, created_at DESC);

ALTER TABLE final_settlements ENABLE ROW LEVEL SECURITY;
ALTER TABLE final_settlements FORCE ROW LEVEL SECURITY;

CREATE POLICY final_settlements_isolation ON final_settlements
    USING (tenant_id = current_tenant_id());

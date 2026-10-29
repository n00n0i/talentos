-- Multi-level OT approval chain. Each request snapshots the tenant's
-- current OT chain into ot_approval_steps and advances sequentially, mirroring
-- the leave approval model (manager -> hr_admin, reject/cancel skip).
ALTER TABLE tenant_payroll_settings
  ADD COLUMN ot_approval_chain TEXT[] NOT NULL DEFAULT '{hr_admin}';

CREATE TABLE ot_approval_steps (
    id            BIGSERIAL PRIMARY KEY,
    tenant_id     UUID NOT NULL,
    ot_request_id UUID NOT NULL REFERENCES ot_requests(id) ON DELETE CASCADE,
    step_no       INT NOT NULL,
    approver_role TEXT NOT NULL CHECK (approver_role IN ('manager', 'hr_admin')),
    status        TEXT NOT NULL DEFAULT 'pending'
                  CHECK (status IN ('pending', 'approved', 'rejected', 'skipped')),
    decided_by    UUID REFERENCES users(id) ON DELETE SET NULL,
    decided_at    TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (ot_request_id, step_no)
);

CREATE INDEX idx_ot_approval_steps_request ON ot_approval_steps(ot_request_id);

ALTER TABLE ot_approval_steps ENABLE ROW LEVEL SECURITY;
ALTER TABLE ot_approval_steps FORCE ROW LEVEL SECURITY;

CREATE POLICY ot_approval_steps_isolation ON ot_approval_steps
    USING (tenant_id = current_tenant_id());

-- Multi-level leave approval chain. Each request snapshots the tenant's
-- current chain into leave_approval_steps and advances sequentially.
ALTER TABLE tenant_payroll_settings
  ADD COLUMN leave_approval_chain TEXT[] NOT NULL DEFAULT '{hr_admin}';

CREATE TABLE leave_approval_steps (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    leave_request_id UUID NOT NULL REFERENCES leave_requests(id) ON DELETE CASCADE,
    step_no INT NOT NULL,
    approver_role TEXT NOT NULL CHECK (approver_role IN ('manager', 'hr_admin')),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected', 'skipped')),
    decided_by UUID REFERENCES users(id) ON DELETE SET NULL,
    decided_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (leave_request_id, step_no)
);

CREATE INDEX idx_leave_approval_steps_request ON leave_approval_steps(leave_request_id);

ALTER TABLE leave_approval_steps ENABLE ROW LEVEL SECURITY;
ALTER TABLE leave_approval_steps FORCE ROW LEVEL SECURITY;

CREATE POLICY leave_approval_steps_isolation ON leave_approval_steps
    USING (tenant_id = current_tenant_id());

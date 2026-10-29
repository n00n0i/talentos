-- Approval SLA escalation: notify HR once when a pending leave approval step
-- stays undecided past the tenant's SLA. 0 = disabled.
ALTER TABLE tenant_payroll_settings
  ADD COLUMN approval_sla_hours INT NOT NULL DEFAULT 48
  CHECK (approval_sla_hours BETWEEN 0 AND 720);

ALTER TABLE leave_approval_steps
  ADD COLUMN escalated_at TIMESTAMPTZ;

CREATE INDEX idx_leave_approval_steps_escalation
  ON leave_approval_steps (tenant_id, status, escalated_at, created_at);

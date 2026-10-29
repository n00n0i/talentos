# Approval SLA Escalation Plan

**Goal:** Pending leave approval steps older than the tenant SLA notify HR once (no duplicates), configurable or disabled.

## Schema/Settings
- `tenant_payroll_settings.approval_sla_hours INT NOT NULL DEFAULT 48 CHECK (0..720)` (0 = disabled).
- `leave_approval_steps.escalated_at TIMESTAMPTZ NULL`.

## Behaviour
- Worker (gateway, every 15 min) iterates tenants; for each, finds `pending` steps whose `created_at < now() - sla_hours` and `escalated_at IS NULL`, up to 100.
- For each overdue step: insert outbox notifications (email/LINE) for active HR admins with a Thai body naming employee/leave/dates/step; set `escalated_at = now()` in the same tx.
- If SLA is 0, do nothing. If a step is decided before the sweep, it is no longer pending and is skipped.
- No chain/balance change; notification failures follow the outbox retry rules.
- Settings GET/PUT payroll includes `approval_sla_hours`; settings UI numeric input `approval-sla-hours` (0 = ปิด).

## Tests
- Overdue step → one notification per HR admin and escalated_at set; second sweep no duplicate.
- Fresh step under SLA ignored; decided step ignored; SLA 0 disables.
- Setting validation 0..720.
- Tenant isolation.

## Gates
fmt/clippy/tests/build/Playwright 21/21.

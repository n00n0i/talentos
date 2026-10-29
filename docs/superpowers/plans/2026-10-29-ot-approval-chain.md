# OT Approval Chain Plan

**Goal:** OT requests use the same sequential approval model as leave: a tenant chain (default HR only, or manager → HR), step roles, reject/cancel skip.

## Schema/Settings
- `tenant_payroll_settings.ot_approval_chain TEXT[] NOT NULL DEFAULT '{hr_admin}'`.
- `ot_approval_steps(id BIGSERIAL, tenant_id, ot_request_id REFERENCES ot_requests ON DELETE CASCADE, step_no INT, approver_role TEXT CHECK manager/hr_admin, status TEXT pending/approved/rejected/skipped DEFAULT pending, decided_by UUID, decided_at, created_at, UNIQUE(ot_request_id, step_no))`, RLS FORCE.
- Chain validation identical to leave (1–3, unique, last hr_admin); settings GET/PUT includes it; UI select `ot-approval-chain`.

## Behaviour
- `create_request` reads the chain and inserts steps (default `{hr_admin}`); request stays pending.
- `decide_request`: current pending step (lowest step_no); allow caller with the step role or hr_admin; approve advances, final approve sets `approved` and re-applies effective OT; reject sets `rejected` and skips remaining; only pending.
- `cancel_request` skips pending steps.
- `pending_approvals` returns current step (`step_no`, `total_steps`, `approver_role`) via LEFT JOIN (legacy step-less rows default 1/1/hr_admin).
- No leave/attendance change beyond the final OT apply path; no balance.

## UI
`/ot` HR inbox shows “ขั้น x/y · role”; settings OT chain select; existing approve/reject works.

## Tests
Default one step; manager→HR advance; role denial; HR override; reject skips; cancel skips; snapshot; legacy step-less row; settings validation.

## Gates
fmt/clippy/tests/build/Playwright 21/21.

# Task 1 Report: Multi-level Leave Approvals (Backend)

## Status
Complete.

## Commit
`cdeffad34b0502b784779ddda00bc5e0e68538a3`

## Changes
- `migrations/20261107000001_leave_approval_steps.sql`
  - Adds `tenant_payroll_settings.leave_approval_chain TEXT[] NOT NULL DEFAULT '{hr_admin}'`
  - Creates `leave_approval_steps` with RLS, tenant policy, unique `(leave_request_id, step_no)`
- `crates/domain/talentos-payroll/src/settings.rs`
  - Adds `leave_approval_chain: Vec<String>` to `PayrollSettings` with default `["hr_admin"]`
  - Extends `is_valid()` to enforce chain rules (manager/hr_admin, length 1..=3, last == hr_admin)
- `crates/services/talentos-hr-runner/src/leave.rs`
  - Snapshots chain into `leave_approval_steps` on `create_request`
  - `decide_request` now takes `roles: &[String]` and returns `DecideOutcome { status, step, total_steps }`
  - Enforces step role; `hr_admin` may override any step; others get `LeaveError::Forbidden`
  - Approve advances to next step or final approval; reject marks remaining steps skipped and restores balance once
  - `cancel_request` skips pending steps
  - `pending_approvals` joins current pending step and returns `step_no`, `total_steps`, `approver_role`
  - Legacy pending requests without steps get a single `hr_admin` step on the fly
- `crates/api/gateway/src/handlers/leave.rs`
  - Passes `ctx.roles` to `decide_request`
  - Decision response now includes `status`, `step`, `total_steps`
  - Maps `Forbidden` to 403
- `crates/api/gateway/src/handlers/settings.rs`
  - Reads/writes `leave_approval_chain` in payroll settings GET/PUT
  - Returns 400 on invalid chain
- `crates/services/talentos-payroll-runner/src/lib.rs`
  - Reads `leave_approval_chain` when loading payroll settings
- `crates/services/talentos-hr-runner/tests/leave_approval.rs` (new)
  - 12 integration tests covering default one-step, two-step advance, role enforcement, HR override, reject skip, cancel skip, chain snapshot, legacy requests, inbox step display, balance behavior
- `crates/services/talentos-hr-runner/tests/leave_self.rs`
  - Updated existing callers for new `decide_request` signature and role requirements
- `crates/services/talentos-hr-runner/tests/common/mod.rs`
  - Added `set_payroll_chain` helper; expanded cleanup

## Test Summary
- `cargo fmt --all` ✓
- `cargo clippy -p talentos-hr-runner -p talentos-gateway --all-targets -- -D warnings` ✓
- `cargo test -p talentos-hr-runner` ✓ — 73 passed (11 unit + 62 integration)
- `cargo test -p talentos-gateway` ✓ — 48 passed
- `cargo check -p talentos-payroll-runner` ✓

## Concerns / Risks
- Between Task 1 and Task 2, the web settings page PUT does not send `leave_approval_chain`, so saving payroll settings will reset the chain to the default `["hr_admin"]`. Task 2 updates the UI to preserve/send the chain.
- `pending_approvals` only shows requests that have a pending step; legacy pending rows without steps will not appear until a decision creates the step. This matches the plan's legacy-on-the-fly behavior for `decide_request`.
- Migration was applied manually with `docker exec -i talentos-postgres-1 psql ...`; ensure CI/production migrations include `20261107000001_leave_approval_steps.sql`.

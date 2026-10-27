# Task 1 Report: Multi-level Leave Approvals (Backend)

## Status
Complete. Review fixes applied.

## Commits
- Initial implementation: `cdeffad34b0502b784779ddda00bc5e0e68538a3`
- Review fixes: `cec854d`

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
- `cargo clippy -p talentos-hr-runner -p talentos-gateway -p talentos-graphql --all-targets -- -D warnings` ✓
- `cargo test -p talentos-hr-runner` ✓ — 81 passed (11 unit + 70 integration)
- `cargo test -p talentos-gateway` ✓ — 48 passed
- `cargo test -p talentos-graphql` ✓ — 3 passed
- `cargo check -p talentos-payroll-runner` ✓

## Review Fixes (cec854d)
1. **Rejection notification regression** — `decide_request` now enqueues `leave_decided` for both final approve and final reject; intermediate steps still do not notify. Added `rejected_request_creates_notification_for_employee` test.
2. **GraphQL bypass** — `request_leave`, `approve_leave_request`, `reject_leave_request`, and `cancel_leave_request` mutations in `crates/api/graphql/src/resolvers/leave.rs` now immediately return an `async_graphql::Error` directing callers to the equivalent REST endpoints. Query resolvers remain intact.
3. **Legacy inbox visibility** — `pending_approvals` now uses a `LEFT JOIN` to `leave_approval_steps`; pending requests without steps appear as `step_no=1`, `total_steps=1`, `approver_role='hr_admin'`.
4. **Reject step number** — `decide_request` now returns the step that was actually rejected (`step_no`) rather than `total_steps`.
5. **Duplicate roles** — `PayrollSettings::leave_approval_chain_is_valid` and the defensive `normalise_chain` both reject duplicate roles (e.g. `["manager","manager","hr_admin"]` is invalid).

## Concerns / Risks
- Between Task 1 and Task 2, the web settings page PUT does not send `leave_approval_chain`, so saving payroll settings will reset the chain to the default `["hr_admin"]`. Task 2 updates the UI to preserve/send the chain.
- Migration was applied manually with `docker exec -i talentos-postgres-1 psql ...`; ensure CI/production migrations include `20261107000001_leave_approval_steps.sql`.

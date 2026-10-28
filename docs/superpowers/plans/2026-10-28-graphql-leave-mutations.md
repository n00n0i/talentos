# GraphQL Leave Mutations Re-enable Plan

**Goal:** GraphQL `requestLeave`, `approveLeaveRequest`, `rejectLeaveRequest`, and `cancelLeaveRequest` work again and go through the same service layer as REST, including multi-level approvals and balance rules.

**Files:** `crates/api/graphql/src/resolvers/leave.rs`, Cargo dep on `talentos-hr-runner` if absent.

## Rules
- `requestLeave` calls `talentos_hr_runner::create_request` (employee resolved from the caller; no on-behalf).
- `approve/reject` call `decide_request(pool, tenant, user, &roles, id, approve)`; `reject` keeps its `reason` argument but the service does not store extra rejection text (documented).
- `cancelLeaveRequest` calls `cancel_request`.
- After the service call, fetch the row with the existing joined SELECT by id and return it.
- Permission checks stay (`submit_self` / `approve_team`). Service errors map to GraphQL errors with a clear message (not found/forbidden/conflict).
- Remove the “use REST” error stubs.

## Tests
Unit test the error-to-message mapping if practical; existing query tests must stay green. No UI change.

## Gates
`cargo fmt`, `cargo clippy -p talentos-graphql -p talentos-hr-runner --all-targets -- -D warnings`, `cargo test -p talentos-graphql`, `npm run build` (no web change), Playwright 21/21 as regression.

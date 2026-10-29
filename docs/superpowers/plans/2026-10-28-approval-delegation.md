# Approval Delegation Plan

**Goal:** A manager/HR can delegate leave-approval authority to another active user for a date range; the delegate can act on a step their delegator's role would allow.

## Files
Migration `approval_delegations`; `leave.rs` decide role check; gateway `delegations.rs` + routes + RBAC; settings/leave UI; tests.

## Schema
`approval_delegations(id, tenant_id, delegator_user_id, delegate_user_id, starts_on DATE, ends_on DATE, created_by, created_at, UNIQUE(tenant_id, delegator_user_id, delegate_user_id, starts_on), CHECK (delegator <> delegate), CHECK (ends_on >= starts_on))`, RLS FORCE.

## Rules
- Only the delegator or hr_admin creates/cancels; delegate must be an active tenant user.
- Active delegation = `starts_on <= current_date <= ends_on`.
- `decide_request`: allow if caller has the step role; otherwise allow if an active delegation exists from a delegator who has that role. HR override still applies.
- Employees cannot gain approval rights this way unless a manager/HR delegates a role they actually hold.
- Expired/self delegation is rejected/ignored.
- No change to chains, balances, notifications.

## Endpoints
`GET /api/v1/approval-delegations` (own + hr all), `POST {delegate_user_id, starts_on, ends_on}`, `DELETE /:id` (delegator/hr_admin).

## UI
`/settings` (or `/leave` HR card) small “มอบหมายการอนุมัติ” section: user select, date range, save, list with cancel. Thai.

## Tests
Manager delegates to employee → delegate approves manager-step request; expired delegation denied; self-delegation 400; employee cannot delegate manager rights; HR creates for another manager; cancel stops rights.

## Gates
fmt/clippy/tests/build/Playwright 22/22 (new E2E: HR delegates to invited employee, employee approves a 2-step manager step, then HR final).

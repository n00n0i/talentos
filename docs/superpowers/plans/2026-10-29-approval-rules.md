# Conditional Leave Approval Rules Plan

**Goal:** Tenants can route leave by type and minimum days (e.g. annual leave ≥5 days → manager→HR), with the tenant default chain as fallback.

## Schema
`leave_approval_rules(id BIGSERIAL, tenant_id, leave_type TEXT NULL, min_days NUMERIC(6,2) NULL, chain TEXT[] NOT NULL, priority INT NOT NULL DEFAULT 0, created_at, created_by)`, RLS FORCE.

## Selection at request creation
- Match `(leave_type IS NULL OR leave_type = input) AND (min_days IS NULL OR days >= min_days)`.
- Order: type-specific first, then min_days-specific, then priority desc, id; first match wins.
- No match → `tenant_payroll_settings.leave_approval_chain` (default `{hr_admin}`).
- Chain validation identical to the default (manager/hr_admin, 1–3, no duplicates, last hr_admin).
- Rules are snapshotted into steps as today; later rule changes do not alter live requests.

## Endpoints/UI
- `GET/POST/DELETE /api/v1/leave/approval-rules` (`settings.manage_company`, hr_admin).
- Settings card “เงื่อนไขการอนุมัติ”: list rules, add (leave type or all, min days optional, chain), delete. Thai.
- Rules with no type/days duplicate the default; allow but document.

## Tests
Type-specific beats generic; min-days threshold; below threshold uses default; priority tie-break; snapshot; validation; tenant scope.

## Gates
fmt/clippy/tests/build/Playwright 21/21.

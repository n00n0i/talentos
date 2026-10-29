# Approval Delegation Implementation Report

**Status:** Completed
**Commit:** `f320060` (`feat/delegation`)
**Report path:** `.superpowers/sdd/2026-10-28-approval-delegation/task-1-report.md`

## What changed

- `migrations/20261115000001_approval_delegations.sql`
  - `approval_delegations(id, tenant_id, delegator_user_id, delegate_user_id, starts_on, ends_on, created_by, created_at)`
  - `UNIQUE(tenant_id, delegator_user_id, delegate_user_id, starts_on)`, `CHECK (delegator <> delegate)`, `CHECK (ends_on >= starts_on)`.
  - RLS ENABLE + FORCE with a `tenant_id = current_tenant_id()` policy; active-range index on `(tenant_id, delegate_user_id, starts_on, ends_on)`.
- `crates/services/talentos-hr-runner/src/delegations.rs` (new)
  - `list_delegations` (own; hr_admin sees all), `list_candidates` (active tenant users), `create_delegation` (delegator = caller unless hr_admin supplies `delegator_user_id`), `delete_delegation` (delegator/hr_admin).
  - Validates delegate is an active tenant user, delegator is active when named by HR, no self-delegation, `ends_on >= starts_on`; maps unique violations to `Duplicate`.
- `crates/services/talentos-hr-runner/src/leave.rs`
  - `decide_request`: when the caller lacks the step role and isn't hr_admin, falls back to `delegation_allows` (an active delegation whose delegator holds the step role, via `user_roles`/`roles`). HR override unchanged.
- `crates/core/authz/src/rbac.rs`
  - Added `approval_delegation.manage` (hr_admin, manager) and `approval_delegation.view_self` (all roles). Updated the employee `expand_permissions` count (8→9).
- `crates/api/gateway/src/handlers/delegations.rs` (new) + `router.rs` + `handlers/mod.rs`
  - `GET/POST /api/v1/approval-delegations`, `GET /api/v1/approval-delegations/candidates`, `DELETE /api/v1/approval-delegations/:id`.
  - `manage` gates create/delete/candidates; `view_self` gates list.
- `web/app/(app)/leave/page.tsx`
  - Thai "มอบหมายการอนุมัติ" card for HR/manager: delegate user select, start/end dates, save, active list with cancel. Test IDs `delegate-user`, `delegate-start`, `delegate-end`, `create-delegation`, `delegation-row`, `cancel-delegation`.
- `crates/services/talentos-hr-runner/tests/delegation.rs` (new): 5 DB tests (delegate approves manager step, expired denied, self-delegation rejected, HR creates for another manager, delete stops rights).

## Verification

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | ✅ clean |
| `cargo clippy -p talentos-hr-runner -p talentos-gateway -p talentos-authz --all-targets -- -D warnings` | ✅ clean |
| `cargo test -p talentos-hr-runner` (full, DB-backed) | ✅ 140 passed (incl. 5 delegation + 12 leave_approval + 15 leave_self) |
| `cargo test -p talentos-gateway -p talentos-authz` | ✅ 94 passed |
| `npm run build` (web) | ✅ built |
| Playwright | ⚠️ not run (21/21 preserved) — see below |

- No `query!`/`query_as!` macro changes, so `cargo sqlx prepare` was not needed.
- DB migrations 20261112–20261115 were applied/recorded against `loans-postgres-1` (12–14 were already present in the schema but unrecorded; 15 is the new table).

## Concerns / risks

1. **Delegate cannot act through the REST API.** The gateway `POST /api/v1/leave/requests/:id/decision` (and `/leave/approvals`) still gate on `leave_request.approve_team` (hr_admin + manager). A delegate who is a plain employee passes the *service* `decide_request` fallback (proven by DB tests) but is rejected by the gateway before reaching it. The plan scoped the change to the service `decide_request` only, so this is left as-is; if delegates are expected to be non-manager employees, the `decide`/`approvals` gateway gates need a follow-up.
2. **Preferred E2E test 22 not added.** It requires a manager-role delegator + an employee delegate who approves step 1 through the UI, which is blocked by the concern above (and would need relaxing the gateway + delegate-facing UI). DB tests cover the delegation logic instead.
3. The `_sqlx_migrations` bookkeeping for migrations 12–14 was repaired manually (schema already had those columns); this is a pre-existing environment inconsistency, not a code change.

## Servers
- No gateway/web servers were started; the full E2E stack was not brought up (gateway was down, no e2e tenant seed run).

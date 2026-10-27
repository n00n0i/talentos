# Leave Self-Service (Phase 3a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** An employee sees their own leave balances/history, requests leave and cancels a pending request; HR sees all pending requests and approves/rejects them.

**Architecture:** The leave domain/service already exists (`create_request`, `list_balances`, `list_requests`, `employee_of_user`). Add a self-scoped read endpoint, a cancel operation, and an HR approval inbox; add a Thai `/leave` page that switches between employee and HR views by permission. No payroll or schema changes.

**Tech Stack:** Rust/Axum/SQLx, existing `talentos-hr-runner::leave`, Next.js 15, Playwright.

**Spec:** `docs/superpowers/plans/2026-10-27-competitive-gap-roadmap.md` (Phase 3, leave/OT self-service slice).

## Global Constraints

- Every query in `TenantPool::begin_tenant`; FORCE RLS; runtime `talentos_app`.
- No `unwrap()`/`expect()` in production; `thiserror`.
- Thai UI, existing sage/teal tokens; dates `YYYY-MM-DD` in API, Thai in UI.
- Employees can read/cancel only their own leave; HR (`leave_request.view_team`) sees the tenant inbox.
- `unpaid` leave is never balance-blocked; paid types use existing balance checks.
- Existing endpoints/permissions stay backward compatible.
- fmt/clippy/tests, `npm run build`, Playwright 10/10.

## Review Focus

- **Unlinked user**: `/me/leave` returns `employee_linked:false` with empty arrays, not 404/500.
- **Cross-employee cancel**: an employee cannot cancel another employee's request (404, not silent success).
- **Cancel state**: only `pending` may be cancelled; approved/rejected return 409 and payroll-relevant balances are untouched.
- **HR inbox**: a tenant-wide HR user is not linked to an employee and must still see pending requests with employee codes.
- **Timezone/date**: dates are calendar dates; no UTC shift.

---

### Task 1: Self leave API + cancel + HR inbox

**Files:**
- Modify: `crates/core/authz/src/rbac.rs` — add `leave_request.view_self` (all roles).
- Modify: `crates/services/talentos-hr-runner/src/leave.rs` — `cancel_request`, `pending_approvals`.
- Modify: `crates/api/gateway/src/handlers/leave.rs` — `me_leave`, `me_cancel`, `approvals`.
- Modify: `crates/api/gateway/src/router.rs`.
- Test: `crates/services/talentos-hr-runner/tests/leave_self.rs`.

**Interfaces:**
```rust
pub async fn cancel_request(pool, tenant, user_id: Uuid, request_id: Uuid)
    -> Result<String, LeaveError>; // returns "cancelled"
pub struct PendingLeave { pub id, pub employee_id, pub employee_code, pub employee_name,
    pub leave_type, pub start_date, pub end_date, pub days, pub reason }
pub async fn pending_approvals(pool, tenant) -> Result<Vec<PendingLeave>, LeaveError>;
```
- `GET /api/v1/me/leave` (view_self): `{employee_linked, balances, requests}`; current year.
- `POST /api/v1/me/leave/:id/cancel` (submit_self): 200 `{status:"cancelled"}`; 409 if not pending; 404 if not own.
- `GET /api/v1/leave/approvals` (view_team): pending requests with codes/names.
- Reuse `create_request` for self requests (`employee_id: null` resolves by user).

- [ ] **Step 1: Write failing DB tests** — linked user sees only own balances/requests; unlinked gets empty; cancel pending succeeds; cancel another employee's request 404; cancel approved 409; HR inbox lists a pending request with code.
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Implement service + handlers + routes + RBAC.**
- [ ] **Step 4: Focused tests + RBAC tests.**
- [ ] **Step 5: Commit.**

---

### Task 2: `/leave` page + E2E

**Files:**
- Create: `web/app/(app)/leave/page.tsx`
- Modify: `web/components/Shell.tsx` — nav “การลา” (all roles).
- Modify: `web/e2e/app.spec.ts` — test 10 (serial).

**Interfaces:** consumes Task 1 endpoints + existing create/decision.

**UI:** balances as cards; request form (type, start/end, reason); history table with cancel for pending; if `leave_request.view_team`, a pending inbox with approve/reject. Test IDs: `leave-type`, `leave-start`, `leave-end`, `leave-reason`, `request-leave`, `cancel-leave`, `approve-leave`, `reject-leave`.

- [ ] **Step 1: Failing E2E** — invited employee requests annual leave 2026-12-01..02; sees pending; HR logs in, approves; employee reloads and sees approved.
- [ ] **Step 2: Run, expect failure (page 404).**
- [ ] **Step 3: Implement page + nav.**
- [ ] **Step 4: `npm run build` + Playwright 10/10.**
- [ ] **Step 5: Commit.**

---

### Task 3: Docs + gates

- [ ] README leave section (self-service endpoints, HR inbox, cancel rule).
- [ ] BUILD_STATUS test/E2E counts and the unchanged schema.
- [ ] Full `cargo fmt/clippy/test`, web build, Playwright 10/10.
- [ ] Commit.

---

## Self-Review

- Roadmap Phase 3 item “Leave/OT self-service” is only partially covered: leave self-service yes; OT requests are deferred to the next slice.
- No schema change; no payroll change.
- Review Focus tests are in Task 1/2.

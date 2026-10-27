# OT Self-Service Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An employee requests overtime authorization for a date, HR approves or rejects it, and approved requests appear as an authorization trail for payroll review.

**Architecture:** A new `ot_requests` table and service mirror the leave slice: employee create/list/cancel, tenant-wide HR pending inbox, approve/reject. Approved OT is **authorization only** in this slice: payroll continues to use attendance-derived OT so the same minutes cannot be paid twice; HR sees approved requests next to the attendance day. A `shortcut:` comment records that integration decision.

**Tech Stack:** Rust/Axum/SQLx, existing `TenantPool`, Next.js 15, Playwright.

**Spec:** `docs/superpowers/plans/2026-10-27-competitive-gap-roadmap.md` (Phase 3 item 1: employee OT self-service; attendance already feeds actual OT to payroll).

## Global Constraints

- Every query in `begin_tenant`; FORCE RLS; `ot_request.*` permissions mirror leave (`submit_self`/`view_self` all roles, `view_team`/`approve_team` hr_admin+manager).
- Employees create/cancel only their own pending requests; another employee's request is 404.
- Only `pending` may be cancelled/decided; otherwise 409.
- Derived attendance and payroll are unchanged; approved OT requests do not alter `employee_days.ot_minutes` or payroll inputs in this slice.
- One live request per employee per date (`pending`/`approved`); a rejected/cancelled request frees the date.
- `minutes` 1..=720; work_date is a calendar date; reason optional, max 500 chars.
- No `unwrap()`/`expect()` in production; Thai UI; existing endpoints unchanged.
- `cargo fmt/clippy/test`, `npm run build`, Playwright 14/14.

## Review Focus

- **Double count:** an approved request must never increase payroll OT on top of attendance-derived OT (this slice leaves payroll untouched).
- **Ownership:** an employee cannot cancel/see another employee's request; HR inbox sees tenant-wide requests with employee code/name.
- **State:** approve/reject/cancel are terminal; a second decision is 409, not a silent success.
- **Duplicate dates:** a second live request for the same employee/date is rejected with 409; rejected/cancelled frees it.
- **Unlinked user:** `/me/ot` returns `employee_linked:false` with empty arrays, not 404/500.
- **Bounds:** 0, 721, negative minutes, and blank dates are 422/400 with a clear message.

---

### Task 1: OT API + DB tests

**Files:**
- Create: `migrations/20261105000001_ot_requests.sql`
- Create: `crates/services/talentos-hr-runner/src/ot.rs`
- Modify: `crates/services/talentos-hr-runner/src/lib.rs`
- Create: `crates/api/gateway/src/handlers/ot.rs`
- Modify: `crates/api/gateway/src/handlers/mod.rs`, `router.rs`
- Modify: `crates/core/authz/src/rbac.rs`
- Test: `crates/services/talentos-hr-runner/tests/ot_self.rs`

**Interfaces:**
```sql
ot_requests(
  id UUID PK, tenant_id UUID, entity_id UUID, employee_id UUID,
  work_date DATE, minutes INT CHECK (minutes BETWEEN 1 AND 720),
  reason TEXT, status TEXT DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected','cancelled')),
  requested_by UUID, decided_by UUID, decided_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT now(), updated_at TIMESTAMPTZ DEFAULT now()
)
-- partial unique: (tenant_id, employee_id, work_date) WHERE status IN ('pending','approved')
```
```rust
pub struct OtRequestView { id, employee_id, work_date, minutes, reason, status }
pub struct PendingOt { id, employee_id, employee_code, employee_name, work_date, minutes, reason }
pub async fn create_request(pool, tenant, user_id, employee_id: Option<Uuid>, work_date, minutes, reason) -> Result<OtRequestView, OtError>;
pub async fn list_requests(pool, tenant, employee_id, status: Option<&str>) -> Result<Vec<OtRequestView>, OtError>;
pub async fn cancel_request(pool, tenant, user_id, id) -> Result<(), OtError>;
pub async fn pending_approvals(pool, tenant) -> Result<Vec<PendingOt>, OtError>;
pub async fn decide_request(pool, tenant, approver, id, approve) -> Result<String, OtError>;
```
- `GET /api/v1/me/ot` (view_self) → `{employee_linked, requests}`.
- `POST /api/v1/me/ot` (submit_self) `{work_date, minutes, reason?}`; 409 duplicate live date.
- `POST /api/v1/me/ot/:id/cancel` (submit_self) → 200.
- `GET /api/v1/ot/approvals` (view_team) → `{requests:[PendingOt]}`.
- `POST /api/v1/ot/requests/:id/decision` (approve_team) `{approve: bool}` → 200 `{status}`; 404/409 as above.
- HR on-behalf create is not part of this slice (`employee_id` stays server-resolved from the caller).

- [ ] **Step 1: Failing DB tests** — linked create/list; unlinked empty; cancel pending; cancel another's 404; cancel approved 409; duplicate date 409; rejected frees date; HR inbox code/name; decide twice 409; bounds 0/721.
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Migration, service, handlers, routes, RBAC; apply migration with psql.**
- [ ] **Step 4: Focused tests + clippy.**
- [ ] **Step 5: Commit.**

---

### Task 2: `/ot` page + E2E

**Files:**
- Create: `web/app/(app)/ot/page.tsx`
- Modify: `web/components/Shell.tsx` (nav “OT” for all roles)
- Modify: `web/e2e/app.spec.ts` (test 14)
- Test IDs: `ot-date`, `ot-hours`, `ot-reason`, `request-ot`, `ot-row`, `cancel-ot`, `ot-approval-row`, `approve-ot`, `reject-ot`.

**Interfaces:** consumes Task 1 endpoints; fetches `/auth/me` for `ot_request.view_team`.

- Employee view: form date/hours/reason, history table (date, hours, reason, status), cancel pending.
- HR view: pending inbox with employee code/name, date, hours, reason, approve/reject.
- Show a short Thai note: approved OT is an authorization record; payroll uses actual punches.
- E2E test 14 (serial): invited employee requests 2026-12-03, 2 hours, reason E2E → sees pending; HR approves → employee reloads and sees approved.

- [ ] **Step 1: Add E2E test 14; run, expect failure (page 404).**
- [ ] **Step 2: Implement page + nav.**
- [ ] **Step 3: `npm run build`; Playwright 14/14.**
- [ ] **Step 4: Commit.**

---

## Self-Review

- Roadmap item 1 is covered for leave (previous slice) and OT submissions/approvals.
- Payroll integrity is preserved by not writing attendance/payroll from requests; the shortcut is documented for a future auto-apply slice.
- No change to attendance derivation, payroll math, or existing endpoints.
- Tests pin ownership, state transitions, duplicates, and bounds.

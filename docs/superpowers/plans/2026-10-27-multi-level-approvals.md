# Multi-Level Leave Approvals Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A tenant configures a sequential leave approval chain (e.g. manager → HR); a request advances step by step and is approved/rejected only by the step's role (HR may override).

**Architecture:** `tenant_payroll_settings` gains `leave_approval_chain TEXT[]` (default `{hr_admin}`). Creating a leave request snapshots the chain into `leave_approval_steps`; `decide_request` advances the current step instead of flipping the request status directly. The last step sets the request status. Existing one-step tenants behave exactly as today.

**Tech Stack:** Rust/Axum/SQLx, existing leave service, Next.js 15, Playwright.

**Spec:** `docs/superpowers/plans/2026-10-27-competitive-gap-roadmap.md` (Phase 3 item 2: approvals manager → HR).

## Global Constraints

- Chain values only `manager`/`hr_admin`, length 1–3, last step must be `hr_admin`; default `{hr_admin}`.
- Steps are snapshotted at creation; later settings changes do not alter live requests.
- Only the current step's role may decide; `hr_admin` may override any step. Other roles 403.
- Reject at any step rejects the request and skips remaining steps; cancel skips pending steps; balances/notifications only change on final approve/reject (as today).
- Existing one-step behavior and endpoints stay backward compatible; response gains `step`/`total_steps` (additive).
- RLS/`begin_tenant`, no unwrap/expect, Thai UI, `cargo fmt/clippy/test`, `npm run build`, Playwright 15/15.

## Review Focus

- **Default unchanged:** a tenant with no settings row still creates one HR step and approves in one action.
- **Order:** step 1 manager cannot be skipped; step 2 HR is only reachable after step 1 approves.
- **Role check:** a manager cannot decide the HR step; an employee cannot decide any step; HR can override.
- **Reject/cancel:** remaining steps become skipped and the request is terminal.
- **Snapshot:** changing the tenant chain does not alter pending requests.
- **Balances:** intermediate approval does not re-debit/restore; final reject restores once.

---

### Task 1: Steps schema, chain setting, advance logic, DB tests

**Files:**
- Create: `migrations/20261107000001_leave_approval_steps.sql`
- Modify: `crates/services/talentos-hr-runner/src/leave.rs`
- Modify: `crates/api/gateway/src/handlers/leave.rs` (decision response/roles)
- Modify: `crates/api/gateway/src/handlers/settings.rs` (payroll settings GET/PUT chain)
- Test: `crates/services/talentos-hr-runner/tests/leave_self.rs` (or new `leave_approval.rs`)

**Interfaces:**
```sql
ALTER TABLE tenant_payroll_settings
  ADD COLUMN leave_approval_chain TEXT[] NOT NULL DEFAULT '{hr_admin}';
CREATE TABLE leave_approval_steps (
  id UUID PK, tenant_id UUID, leave_request_id UUID REFERENCES leave_requests ON DELETE CASCADE,
  step_no INT, approver_role TEXT CHECK (approver_role IN ('manager','hr_admin')),
  status TEXT DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected','skipped')),
  decided_by UUID, decided_at TIMESTAMPTZ, created_at TIMESTAMPTZ DEFAULT now(),
  UNIQUE (leave_request_id, step_no)
);
```
```rust
pub struct PendingLeave { ..., pub step_no: i32, pub total_steps: i64, pub approver_role: String }
pub async fn decide_request(pool, tenant, approver, roles: &[String], request_id, approve) -> Result<DecideOutcome, LeaveError>
pub struct DecideOutcome { pub status: String, pub step: i32, pub total_steps: i64 }
```
- `create_request`: read chain (default `{hr_admin}`), insert one step per role.
- `decide_request`: load request + current pending step; enforce role (`hr_admin` override); approve → mark step, advance, final sets `leave_requests.status`; reject → request rejected, remaining skipped; use the existing balance restore only on final reject.
- `cancel_request`: mark pending steps skipped.
- `pending_approvals`: join the current step; include step/no/total; keep the tenant-wide list.
- Settings GET/PUT payroll include/accept `leave_approval_chain` with validation.

- [ ] **Step 1: Failing DB tests** — default one step; manager→HR two-step advance; manager cannot approve HR step; HR override; reject at step 1 skips step 2; cancel skips; chain snapshot.
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Migration (apply with psql), service, handlers, settings.**
- [ ] **Step 4: Focused tests + clippy.**
- [ ] **Step 5: Commit.**

---

### Task 2: Settings chain selector, inbox step display, E2E

**Files:**
- Modify: `web/app/(app)/settings/page.tsx`
- Modify: `web/app/(app)/leave/page.tsx`
- Modify: `web/e2e/app.spec.ts` (test 15)

- Settings payroll card: select “ลำดับอนุมัติการลา” with options `HR อนุมัติขั้นเดียว` (`["hr_admin"]`) and `หัวหน้างาน → HR` (`["manager","hr_admin"]`); save with the payroll settings.
- Leave HR inbox: show “ขั้น {step}/{total} · {role label}”; keep approve/reject buttons; after an intermediate approve, row stays with the next step.
- E2E test 15: HR sets the two-step chain; invited employee requests leave 2026-12-04..05; HR approves step 1 (override), sees step 2; approves step 2; employee sees approved; HR restores the one-step chain at the end.

- [ ] **Step 1: Add E2E test 15; expect failure.**
- [ ] **Step 2: Implement UI.**
- [ ] **Step 3: `npm run build`; Playwright 15/15; commit.**

---

## Self-Review

- Delegation/SLA/escalation are not built; the chain covers sequential roles only (documented).
- OT approvals remain single-step; this slice is leave-only.
- Notifications still fire on the final decision only; intermediate step has no notification.
- No payroll change beyond the settings column.

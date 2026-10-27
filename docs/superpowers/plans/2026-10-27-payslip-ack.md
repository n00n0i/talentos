# Payslip Delivery & Acknowledgement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An employee opens their payslip from the existing notification/portal and confirms receipt; HR can see acknowledgement state.

**Architecture:** A `payslip_acknowledgements` table records the linked user, time, and IP. The self payslip list returns `acknowledged_at`; a new POST endpoint writes it idempotently. The payslip-ready notification body gains an absolute link to `/me/payslips`. HR payslip rows expose the acknowledgement timestamp.

**Tech Stack:** Rust/Axum/SQLx, existing outbox worker, Next.js 15, Playwright.

**Spec:** `docs/superpowers/plans/2026-10-27-competitive-gap-roadmap.md` (Phase 3 item 5: payslip delivery link + acknowledgement).

## Global Constraints

- Only the linked employee can acknowledge their own payslip; another payslip is 404.
- Acknowledgement is idempotent (second POST returns the original timestamp, 200).
- `begin_tenant` + RLS FORCE on the new table; no payroll math change.
- Notification link uses `APP_URL`; the payslip still requires login.
- No production unwrap/expect; Thai UI; existing endpoints unchanged.
- `cargo fmt/clippy/test`, `npm run build`, Playwright 14/14.

## Review Focus

- **Ownership:** a user cannot acknowledge a colleague's payslip; an unlinked user gets 404/empty, never data.
- **Idempotency:** double-clicking confirm does not fail or duplicate.
- **Notification content:** absolute link, no salary/PII in the message body.
- **HR visibility:** acknowledgement time appears in the run payslip list, not only to the employee.
- **Existing payslips:** rows created before this migration show "not acknowledged".

---

### Task 1: Ack table, endpoint, list fields, notification link

**Files:**
- Create: `migrations/20261106000001_payslip_ack.sql`
- Modify: `crates/api/gateway/src/handlers/me.rs` (`payslips`, `acknowledge`)
- Modify: `crates/api/gateway/src/handlers/payroll.rs` (`list_payslips`)
- Modify: `crates/api/gateway/src/router.rs`
- Modify: `crates/services/talentos-hr-runner/src/notify.rs` (link in payslip body)
- Test: E2E covers ownership/flow; unit test the body link rendering.

**Interfaces:**
```sql
payslip_acknowledgements(
  payslip_id UUID PRIMARY KEY REFERENCES payslips(id) ON DELETE CASCADE,
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  acknowledged_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  ip_address INET
)
```
- `POST /api/v1/me/payslips/:id/acknowledge` (`payslip.view_self`) → `{acknowledged_at}`; join `payslips` → `employees.user_id = caller`; `INSERT ... ON CONFLICT DO NOTHING`, then select the timestamp.
- `GET /me/payslips` adds `acknowledged_at` (LEFT JOIN).
- HR `GET /payroll/runs/:id/payslips` adds `acknowledged_at` per row.
- Payslip-ready body: `... เปิดดูได้ที่ {APP_URL}/me/payslips` (`APP_URL` default `http://localhost:3000`).

- [ ] **Step 1: Implement migration + endpoint + fields + body link.**
- [ ] **Step 2: Focused tests; clippy.**
- [ ] **Step 3: Commit.**

---

### Task 2: UI confirm button + status; E2E

**Files:**
- Modify: `web/app/(app)/me/payslips/page.tsx`
- Modify: `web/e2e/app.spec.ts` (test 7)
- Test IDs: `acknowledge-payslip`, `payslip-ack-status`.

- [ ] **Step 1: Add an “ยืนยันรับสลิปแล้ว” button for unacknowledged rows; show time/status after; reload the list.**
- [ ] **Step 2: Extend test 7: click acknowledge on the first payslip, expect “ยืนยันแล้ว”.**
- [ ] **Step 3: `npm run build`; Playwright 14/14; commit.**

---

## Self-Review

- Delivery already uses the notification worker; this adds the link and receipt.
- No acknowledgement status in payroll reports yet; HR sees it per payslip.
- No signed link; login remains required.

# Expense Claims Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An employee submits an expense claim with a receipt; HR approves or rejects it and can view the receipt.

**Architecture:** A `expense_claims` table with tenant RLS. Receipts are uploaded as raw bytes (JPEG/PNG/PDF, ≤5 MB) and stored on local disk under `CLAIM_UPLOAD_DIR` with a random filename; download is authenticated and tenant-scoped. The `/claims` page switches between the employee form/history and the HR inbox by permission. Payroll reimbursement is deferred.

**Tech Stack:** Rust/Axum/SQLx, `tokio::fs`, Next.js 15, Playwright.

**Spec:** `docs/superpowers/plans/2026-10-27-competitive-gap-roadmap.md` (Phase 4: expense claims with receipt upload and approval).

## Global Constraints

- RLS/`begin_tenant`; `claim.submit_self` all roles, `claim.view_team`/`approve_team` hr_admin+manager.
- Own-only create/list/cancel; pending-only cancel/decide; another employee's claim is 404.
- Categories: `travel`, `meal`, `supplies`, `other`; amount > 0; expense_date required; description ≤500.
- Receipt: max 5 MB, only `image/jpeg`, `image/png`, `application/pdf`; filename is server-generated (no path traversal); download requires the owning employee or HR.
- Approval does not change payroll in this slice (documented).
- No production unwrap/expect; Thai UI; `cargo fmt/clippy/test`, `npm run build`, Playwright 19/19.

## Review Focus

- **Ownership:** a user cannot read/download another employee's receipt; HR can.
- **Upload safety:** content-type allowlist, 5 MB cap, generated filename, no user path.
- **State:** only pending can be cancelled/decided; second decision 409.
- **Missing receipt:** a claim can be approved without one; download 404, never a panic.
- **Tenant isolation:** cross-tenant claim id is 404 on JSON and receipt download.

---

### Task 1: Schema, service, API, receipt storage + tests

**Files:**
- Create: `migrations/20261109000001_expense_claims.sql`
- Create: `crates/services/talentos-hr-runner/src/claims.rs`; modify `lib.rs`
- Create: `crates/api/gateway/src/handlers/claims.rs`; modify `mod.rs`, `router.rs`, RBAC
- Test: `crates/services/talentos-hr-runner/tests/claims.rs`

**Interfaces:**
```sql
expense_claims(id UUID PK, tenant_id, employee_id, category TEXT CHECK (...),
 amount NUMERIC(12,2) > 0, expense_date DATE, description TEXT,
 receipt_path TEXT, receipt_type TEXT, status TEXT ('pending','approved','rejected','cancelled'),
 decided_by UUID, decided_at TIMESTAMPTZ, created_at, updated_at)
```
Endpoints: `GET/POST /api/v1/me/claims`, `POST /api/v1/me/claims/:id/cancel`, `POST /api/v1/me/claims/:id/receipt` (raw body), `GET /api/v1/me/claims/:id/receipt`; HR `GET /api/v1/claims/approvals`, `POST /api/v1/claims/:id/decision`, `GET /api/v1/claims/:id/receipt`.

- [ ] **Step 1: Failing DB tests** — create/list own, unlinked empty, cancel pending, cancel other 404, decide once, decide twice 409, HR inbox.
- [ ] **Step 2: Implement schema, service, upload/download, routes, RBAC.**
- [ ] **Step 3: Focused tests + clippy; commit.**

---

### Task 2: `/claims` page + E2E

- Employee: form (category, amount, date, description, receipt file `claim-receipt`), submit `submit-claim`; history rows `claim-row` with status/cancel `cancel-claim`; receipt link.
- HR: pending inbox `claim-approval-row` with employee/date/amount/receipt link, approve `approve-claim`/reject `reject-claim`.
- Nav “ค่าใช้จ่าย” for all roles.
- E2E test 19: invited employee submits a ฿500 meal claim with a tiny PNG receipt; HR approves; employee reloads and sees approved.
- [ ] Build, Playwright 19/19, commit.

## Self-Review

- Reimbursement through payroll is not wired; HR pays outside payroll (documented).
- No OCR/auto-category, no multi-currency, no per-category limits.

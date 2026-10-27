# Opening Leave Balances Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** HR imports opening leave balances per employee/type/year from CSV with a dry-run report; the employee's `/leave` page then shows a provisioned quota.

**Architecture:** A JSON rows endpoint mirrors the employee/attendance import shape: validate codes, leave types, years, and totals; dry-run writes nothing; commit upserts every row in one tenant transaction. A small CSV panel on the leave page reuses the existing Thai validation style.

**Tech Stack:** Rust/Axum/SQLx, `leave_balances`, Next.js 15, Playwright.

**Spec:** `docs/superpowers/plans/2026-10-27-competitive-gap-roadmap.md` (Phase 3 bulk onboarding: employees + opening balances).

## Global Constraints

- Permission `leave_balance.manage` (hr_admin); `begin_tenant`; all-or-nothing commit; dry-run no writes.
- Rows: `employee_code`, `leave_type`, `year`, `total_days`, optional `used_days` (default 0); `0 <= used_days <= total_days <= 366`; year 2000..=2100.
- Unknown employee/type, duplicates in file, and invalid numbers are per-row errors; commit returns `created: 0` when any row is invalid.
- Upsert key `(employee_id, leave_type_id, year)`; reopening an existing balance overwrites totals.
- No payroll change; no production unwrap/expect; Thai UI; `cargo fmt/clippy/test`, `npm run build`, Playwright 16/16.

## Review Focus

- **Dry run:** repeats without writes; commit with one invalid row writes nothing.
- **Tenant scope:** an employee code from another tenant or a tenant-specific leave type is rejected.
- **Used vs total:** used > total and negative values fail the row.
- **Upsert:** re-importing the same row updates, not duplicates.
- **Provisioned flag:** after import `/me/leave` shows `provisioned: true` with the imported numbers.

---

### Task 1: Import service + endpoint + DB tests

**Files:**
- Modify: `crates/services/talentos-hr-runner/src/leave.rs`
- Modify: `crates/api/gateway/src/handlers/leave.rs`, `router.rs`
- Modify: `crates/core/authz/src/rbac.rs`
- Test: `crates/services/talentos-hr-runner/tests/leave_balances_import.rs`

**Interfaces:**
```rust
pub struct BalanceImportRow { pub employee_code: String, pub leave_type: String, pub year: i32, pub total_days: Decimal, pub used_days: Option<Decimal> }
pub struct BalanceImportReport { pub dry_run: bool, pub total: usize, pub valid: usize, pub invalid: usize, pub created: usize, pub rows: Vec<BalanceImportRowResult> }
pub async fn import_balances(pool, tenant, dry_run, rows) -> Result<BalanceImportReport, LeaveError>;
```
- `POST /api/v1/leave/balances/import` `{dry_run, rows}` → report; 200.
- Validate, detect in-file duplicate `(code,type,year)`, resolve employees and leave types once.
- Commit: one tx, upsert each valid row, no inserts when any invalid.
- Tests: dry-run no writes; valid upsert; duplicate file; used>total; unknown employee; second import updates.

- [ ] **Step 1: Failing DB tests.**
- [ ] **Step 2: Implement service, handler, route, RBAC.**
- [ ] **Step 3: Focused tests + clippy; commit.**

---

### Task 2: Import panel + E2E

**Files:**
- Modify: `web/app/(app)/leave/page.tsx`
- Modify: `web/e2e/app.spec.ts` (test 16)

- HR card “นำเข้าโควตาการลา (CSV)” with file input `import-balance-csv`, dry-run `dry-run-balance-import`, preview, confirm `confirm-balance-import`.
- CSV header `employee_code,leave_type,year,total_days,used_days`.
- Test 16: HR imports `E2E001,annual,2026,10,0`; employee reloads `/leave` and sees `10 / 10`.

- [ ] **Step 1: Add test 16; expect failure.**
- [ ] **Step 2: Implement panel.**
- [ ] **Step 3: `npm run build`; Playwright 16/16; commit.**

---

## Self-Review

- Employee bulk import exists; this closes opening balances. Attendance import exists.
- No carry-forward/write-off logic; HR sets the opening numbers directly.
- No audit row per import yet (same as employee import); the request is permission-gated.

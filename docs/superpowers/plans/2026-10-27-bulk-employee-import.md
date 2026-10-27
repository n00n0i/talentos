# Bulk Employee Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An HR admin uploads a CSV of employees, sees a per-row dry-run report, and imports the whole file atomically when every row is valid.

**Architecture:** One JSON endpoint (`POST /api/v1/employees/import`) mirrors the existing attendance import shape: the browser parses the CSV into rows, the server validates every row with the same rules as single create, checks duplicate codes inside the file and against the tenant, and inserts all rows in one transaction. The `/employees` page gains a CSV panel that previews the dry-run and enables import only when nothing is invalid.

**Tech Stack:** Rust/Axum/SQLx, existing `employees` handler helpers, Next.js 15, Playwright.

**Spec:** `docs/superpowers/plans/2026-10-27-competitive-gap-roadmap.md` (Phase 3 item 4: bulk import with dry-run; acceptance: 50-employee tenant onboarded by CSV).

## Global Constraints

- Every query in `TenantPool::begin_tenant`; RLS stays on; no `unwrap()`/`expect()` in production.
- Permission `employee.create` for both dry-run and commit.
- Commit is all-or-nothing: one invalid row means zero inserts.
- `base_salary` is `Decimal`; dates are `YYYY-MM-DD`; `entity_id` defaults to the token's entity or the tenant's only entity.
- PII fields (national id, bank account, SSO number) are not part of the CSV in v1; HR adds them on the employee page.
- No payroll or schema change; existing create endpoint stays unchanged.
- Thai UI, existing sage/teal classes; `npm run build`; Playwright 11/11.

## Review Focus

- **Duplicate codes:** two rows with the same code, or a code already in the tenant, must be reported on the row and block the commit.
- **All-or-nothing:** a file with 49 valid rows and 1 invalid row creates zero employees.
- **Dry-run purity:** a dry run performs no writes and can be repeated.
- **Entity scope:** a row cannot attach an employee to another tenant's entity; ambiguous tenant-wide tokens must supply `entity_id`.
- **Money/date parsing:** blank or non-numeric salary, and invalid dates, fail the row instead of defaulting to zero.

---

### Task 1: Import endpoint with dry-run

**Files:**
- Modify: `crates/api/gateway/src/handlers/employees.rs` (`import`, `ImportBody`, `ImportEmployeeRow`, `ImportReport`, tests)
- Modify: `crates/api/gateway/src/router.rs`
- Test: unit tests inside `employees.rs` for row validation/report helpers.

**Interfaces:**
```rust
#[derive(Debug, Deserialize)]
pub struct ImportEmployeeRow {
    pub employee_code: String,
    pub first_name_th: String,
    pub last_name_th: String,
    pub position_title: String,
    pub base_salary: Decimal,
    pub hire_date: NaiveDate,
    pub first_name_en: Option<String>,
    pub last_name_en: Option<String>,
    pub department: Option<String>,
    pub entity_id: Option<Uuid>,
    pub tax_filing_status: Option<i16>,
    pub pf_member: Option<bool>,
    pub sso_enrolled: Option<bool>,
}
#[derive(Debug, Deserialize)]
pub struct ImportBody { #[serde(default)] pub dry_run: bool, pub rows: Vec<ImportEmployeeRow> }
#[derive(Debug, Serialize)]
pub struct ImportRowResult { pub row: usize, pub employee_code: String, pub status: String, pub errors: Vec<String> }
#[derive(Debug, Serialize)]
pub struct ImportReport { pub dry_run: bool, pub total: usize, pub valid: usize, pub invalid: usize, pub created: usize, pub rows: Vec<ImportRowResult> }
```
- `POST /api/v1/employees/import` (employee.create) → 200 `ImportReport`.
- Dry run: validate only. Commit: if `invalid > 0` insert nothing and return the report with `created: 0`; otherwise insert all rows, one audit row each, in one transaction.
- Existing-code lookup: one query for `employee_code` values in the report's target entities.

- [ ] **Step 1: Write failing unit tests** for `validate_import_rows` (duplicate in file, blank code, negative salary, bad tax status, valid file) and `report_from_rows` counts.
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Implement the endpoint, report structs, duplicate detection, atomic insert, route.**
- [ ] **Step 4: Unit tests pass; clippy clean.**
- [ ] **Step 5: Commit.**

---

### Task 2: CSV panel on `/employees` + E2E

**Files:**
- Modify: `web/app/(app)/employees/page.tsx`
- Modify: `web/e2e/app.spec.ts` (test 11)

**Interfaces:** consumes `POST /api/v1/employees/import` from Task 1.

- CSV columns (header required): `employee_code,first_name_th,last_name_th,position_title,department,base_salary,hire_date`; optional `first_name_en,last_name_en,tax_filing_status,pf_member,sso_enrolled`.
- Panel: file input (`import-csv`), dry-run button (`dry-run-import`), preview table with row/status/errors, confirm button (`confirm-import`), summary text.
- Confirm is enabled only when `valid > 0 && invalid === 0`.
- After commit: success message, reload employee list, clear the panel.
- E2E test 11: HR logs in, uploads a one-row CSV for `E2E-IMP-1`, dry-run shows 1 valid, confirm, row appears in the employee table.

- [ ] **Step 1: Add E2E test 11; run and expect failure (no panel).**
- [ ] **Step 2: Implement CSV parsing + panel.**
- [ ] **Step 3: `npm run build`; Playwright 11/11.**
- [ ] **Step 4: Commit.**

---

## Self-Review

- Roadmap item 4 covered for employees and opening balances? Employees only; opening leave balances already have a provisioning surface from the leave slice. Attendance import exists.
- Dry-run/no-write and duplicate behavior are pinned in Task 1 tests and E2E.
- No schema change; no payroll change.

# Finance Reports & GL Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** HR/finance can see headcount, payroll cost by department, and OT trend, and export a deterministic GL journal CSV for an approved payroll run.

**Architecture:** Read-only tenant-scoped report queries over existing tables (`employees`, `payslips`, `payroll_runs`, `employee_days`), plus one CSV renderer for journal lines. Department is the cost center in v1 (no new schema). A `/reports` page presents the three tables.

**Tech Stack:** Rust/Axum/SQLx, `rust_decimal`, Next.js 15, Playwright.

**Spec:** `docs/superpowers/plans/2026-10-27-competitive-gap-roadmap.md` (Phase 4 items 5–6: GL export/cost centers and reports).

## Global Constraints

- Every query in `begin_tenant`; permissions: reports `payroll_run.export_report` (hr_admin/payroll_manager), GL export `payroll_run.export_report`.
- Approved runs only for cost/GL; headcount uses `status != 'terminated'`; OT uses approved employee days.
- GL CSV is UTF-8 with a header; stable row order; amounts are `Decimal` strings with 2 dp; formula-injection-safe text fields (reuse `csv_field` style).
- Journal must balance: sum(debits) == sum(credits) per run; no payroll math change.
- No production unwrap/expect; Thai UI; `cargo fmt/clippy/test`, `npm run build`, Playwright 17/17.

## Review Focus

- **Tenant isolation:** every report is scoped to the caller's tenant; another tenant's run id is 404.
- **Approved only:** draft/calculating runs are excluded from cost and rejected by GL export.
- **Balanced journal:** debit salary expense equals net + tax + SSO + PF credits for the run (within 0.01).
- **Cost center:** department is the only dimension; null department is grouped as “ไม่ระบุ”.
- **Determinism:** exporting the same run twice yields identical bytes.
- **Formula safety:** department/name fields starting with `=`, `+`, `-`, `@` are prefixed.

---

### Task 1: Report + GL endpoints and DB tests

**Files:**
- Create: `crates/api/gateway/src/handlers/reports.rs`
- Modify: `crates/api/gateway/src/handlers/mod.rs`, `router.rs`
- Modify: `crates/api/gateway/src/handlers/payroll.rs` (GL export) or put the renderer in `reports.rs`
- Test: `crates/services/talentos-hr-runner/tests/reports.rs`

**Interfaces:**
- `GET /api/v1/reports/headcount` → `{active, probation, leave, by_department:[{department,count}], terminated_last_12m}`
- `GET /api/v1/reports/payroll-cost?from=&to=` → `{rows:[{department,employees,gross,net,tax,sso,pf}]}`
- `GET /api/v1/reports/ot-trend?from=&to=` → `{rows:[{month,ot_minutes,approved_days}]}` (month `YYYY-MM`)
- `GET /api/v1/payroll/runs/:id/export.gl.csv` → `text/csv`:
  `account_code,account_name,department,debit,credit,description`
  accounts: `5100 ค่าแรง`, `2100 เงินเดือนค้างจ่าย`, `2200 ภาษีหัก ณ ที่จ่าย`, `2210 ประกันสังคม`, `2220 กองทุนสำรองเลี้ยงชีพ`.
- Tests seed two employees/departments, one approved run with payslips, approved attendance days; assert headcount, cost sums, OT months, balanced GL, deterministic bytes, cross-tenant 404 at the service query level.
- Permission gate on every handler.

- [ ] **Step 1: Failing DB tests for the query functions.**
- [ ] **Step 2: Implement query functions + handlers + routes.**
- [ ] **Step 3: Focused tests + clippy; commit.**

---

### Task 2: `/reports` page + E2E

**Files:**
- Create: `web/app/(app)/reports/page.tsx`
- Modify: `web/components/Shell.tsx` (nav “รายงาน” for HR/payroll/manager)
- Modify: `web/e2e/app.spec.ts` (test 17)

- Cards: headcount totals + by-department table; payroll cost table from/to (default current year); OT trend table.
- GL export link on the payroll run detail page (`/payroll`) for approved runs: “ดาวน์โหลด GL (CSV)”.
- Test 17: HR opens `/reports`, sees the heading and a headcount total; sets a from/to and sees the cost table (empty state allowed).

- [ ] **Step 1: Add test 17; expect failure.**
- [ ] **Step 2: Implement page + nav + GL link.**
- [ ] **Step 3: `npm run build`; Playwright 17/17; commit.**

---

## Self-Review

- Department is the cost center; a dedicated cost-center field is deferred.
- GL account codes are fixed defaults, not tenant-configurable.
- No report export to Excel/PDF; CSV only.
- No payroll math change.

# SUD-01 — Manager Team Access

**Gate:** `SUD-A1_TEAM_ACCESS_VERIFIED` — **PASS**

## Implementation

- Backend service: `crates/services/talentos-hr-runner/src/team.rs`
  - `list_my_team(pool, tenant, user)` joins `employees e1` (caller) to `employees e2`
    on `e2.manager_id = e1.id` and `e2.tenant_id = e1.tenant_id`.
  - Explicit `e1.tenant_id = $1` and `e1.user_id = $2`; RLS still enforced by
    `begin_tenant`.
  - Excludes `terminated`; ordered by employee code.
- Gateway: `GET /api/v1/me/team`, gated by `employee.read_own_team`
  (`hr_admin`, `payroll_manager`, `manager`). Employees receive **403**.
- Response fields only: `id`, `employee_code`, `employee_name`, `position_title`,
  `department`, `status`. No salary, bank, national ID, SSO number, email or user id.
- UI: `/team` (“ทีมของฉัน”, read-only). Shell resolves permissions before rendering,
  so an employee is redirected to `/clock` without firing the team API.
- HR administrator `/employees` remains unchanged and fully privileged.

## Evidence

| Test | Result |
|---|---|
| Service test `team.rs::returns_only_direct_reports_with_safe_fields` | pass: only direct report returned; unrelated employee and other tenant excluded; serialized keys contain no salary/bank/national ID/SSO/email |
| E2E “manager team directory is scoped and read-only” | pass: manager sees E2E002, not E2E001; no salary/account text; employee gets redirect + API **403** |
| Role matrix E2E | HR/payroll/manager can open `/team`; employee is redirected |
| Rust workspace | 535 passed / 1 ignored |

Screens: `evidence/screens/manager/1440_team.png`,
`manager/390_team.png`; admin/employee screenshots in `evidence/screens/`.
Full log: `evidence/logs/playwright-isolated.log`.

## Limitations

- One flat team level only (direct reports); no recursive org chart.
- HR/payroll users without an employee link see an empty team page (no cross-tenant
  or all-employee bypass).
- No pagination: acceptable for a line manager’s direct reports; add if >200.

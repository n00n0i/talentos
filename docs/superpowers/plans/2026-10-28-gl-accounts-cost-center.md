# GL Chart of Accounts & Cost Center Plan

**Goal:** Tenants configure their GL account codes and assign employees a cost center; GL/report grouping uses cost center (falling back to department).

## Files
Migration; `tenant_payroll_settings` 7 text columns; `employees.cost_center`; settings GET/PUT; employee create/update/list/DTO + UI; GL/reports queries; tests.

## Rules
- Settings columns: `gl_salary_account` 5100, `gl_net_account` 2100, `gl_tax_account` 2200, `gl_sso_account` 2210, `gl_pf_account` 2220, `gl_loan_account` 2230, `gl_claim_account` 5200; non-empty, max 20 chars, digits/letters.
- GL export uses these codes and groups by `COALESCE(e.cost_center, e.department, 'ไม่ระบุ')`; account name stays Thai.
- Payroll cost report groups by the same cost center; headcount by department unchanged.
- Employee `cost_center TEXT NULL` editable on create/update and returned in the DTO; CSV employee import keeps department and leaves cost_center null (documented).
- No payroll math change; existing GL defaults preserved for tenants without a settings row.
- Excel/PDF exports remain out of scope; CSV is the supported format (documented).

## Tests
- DB/unit: default codes when no settings; custom codes in GL CSV; cost-center grouping/fallback.
- Settings validation rejects empty/too-long codes.
- E2E: settings shows the account fields; employee edit can set a cost center and persists.

## Gates
fmt/clippy/tests/build/Playwright 21/21.

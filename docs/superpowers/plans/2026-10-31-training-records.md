# Employee Training Records Plan

**Goal:** HR can record completed training per employee (title, provider, completion/expiry dates) and see expiring certificates; employees can read their own training history.

## Schema
`migrations/20261202000001_training_records.sql`:
`employee_training_records(id, tenant_id, employee_id, title, provider, completed_on, expires_on, created_by, created_at)`
- title 1–200, provider ≤200, `expires_on >= completed_on` when both set.
- index `(tenant_id, employee_id)` and `(tenant_id, expires_on)`.
- RLS forced + tenant policy.

## Service (`talentos-hr-runner/src/training.rs`)
- `TrainingInput` / `TrainingView` (adds `days_left` from Bangkok today).
- `list_training(pool, tenant, employee_id)`.
- `create_training(pool, tenant, user, employee_id, input)` — employee must belong to tenant; 422 invalid title/provider/dates.
- `delete_training(pool, tenant, id)`.
- `expiring_training(pool, tenant, days)` — 1..=365, `expires_on BETWEEN today-365d AND today+days`, exclude terminated employees, oldest first.
- `list_my_training(pool, tenant, user)`.
- `TrainingError` (EmployeeNotFound, NotFound, InvalidTitle/Provider/Dates, Db).

## Endpoints
- `GET/POST /api/v1/employees/:id/training` (`employee.read_all` / `employee.update`).
- `DELETE /api/v1/training/:id` (`employee.update`).
- `GET /api/v1/training/expiring?days=90` (`employee.read_all`).
- `GET /api/v1/me/training` (self).
- PDPA export: add training records per employee.

## UI
- `/training` (nav “อบรม”, HR writes; payroll read-only):
  - expiring panel (`training-expiring`) with days-left badges;
  - employee selector, add form (title/provider/completed/expiry), records table (`training-row`) with expiry badge and delete.
- Shell: nav item + `hrOnly` guard.

## Tests
- DB: create/list, expiry filter includes expired + excludes far-future, invalid dates/title/provider, delete, tenant + self scoping.
- E2E: HR records a training expiring in 30 days for E2E001, sees it in the table and expiring panel; payroll can open read-only; employee is redirected.
- Gates: fmt/clippy/tests/build, Playwright 50/50 (or final count).

## Out of scope
- No certificate file upload, no course catalog, no reminders/notifications.

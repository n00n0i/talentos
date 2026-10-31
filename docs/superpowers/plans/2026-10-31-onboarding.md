# Employee Onboarding Checklist Plan

**Goal:** HR can start a standard onboarding checklist for an employee, add ad-hoc tasks, track completion, and employees can see/complete their own tasks.

## Schema
`migrations/20261131000001_onboarding_tasks.sql`:
`employee_onboarding_tasks(id, tenant_id, employee_id, title, notes, due_date, status, completed_at, completed_by, created_by, created_at, updated_at)`
- `status` in `pending|done`; title 1–200; notes ≤500.
- unique `(tenant_id, employee_id, title)` so the standard checklist is idempotent.
- index `(tenant_id, employee_id, status)`; RLS forced + tenant policy.

## Service (`talentos-hr-runner/src/onboarding.rs`)
- Standard template (code): sign contract (0d), submit ID copy (3d), payroll bank details (5d), SSO documents (7d), safety/company rules training (7d), equipment/email setup (1d). Due date = hire date + offset.
- `list_tasks(pool, tenant, employee_id)` — ordered by done state then due date.
- `start_standard(pool, tenant, user, employee_id)` — inserts missing standard tasks (`ON CONFLICT DO NOTHING`), returns the full list.
- `create_task(pool, tenant, user, employee_id, input)` — duplicate title → `DuplicateTask`.
- `set_done(pool, tenant, user, task_id, done)` — sets `completed_at/by`; idempotent.
- `delete_task(pool, tenant, task_id)`.
- Self: `list_my_tasks`, `set_my_done` (ownership via `employees.user_id`).
- `TaskView` includes `days_left` from Bangkok today.

## Endpoints
- `GET/POST /api/v1/employees/:id/onboarding` (`employee.read_all` / `employee.update`).
- `POST /api/v1/employees/:id/onboarding/standard` (`employee.update`).
- `PATCH/DELETE /api/v1/onboarding/:id` (`employee.update`).
- `GET /api/v1/me/onboarding`, `PATCH /api/v1/me/onboarding/:id` (self).
- No payroll/timesheet changes.

## UI
- `/onboarding` (nav “Onboarding”, HR writes; payroll read-only via `employee.update`):
  - employee selector, progress “เสร็จ X/Y”, “สร้างรายการมาตรฐาน”, add-task form (title/due/notes), task rows with checkbox/delete, overdue badge.
- Shell: nav item + `hrOnly` guard.
- PDPA export: include onboarding tasks under each employee.

## Tests
- DB: standard checklist idempotent, due dates from hire date, toggle sets/clears `completed_at/by`, duplicate title rejected, tenant scoping, self list only own tasks.
- E2E: HR starts the standard checklist for E2E001, sees tasks, ticks one, progress updates; payroll can open, employee is redirected.
- Gates: fmt/clippy/tests/build, Playwright 48/48 (or final count).

## Out of scope
- No notifications/reminders for due tasks (page-level overdue badge only).
- No custom tenant templates (standard list is code for now; HR can add ad-hoc tasks).

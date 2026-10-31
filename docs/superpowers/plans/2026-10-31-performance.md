# Performance Review Cycles Plan

**Goal:** HR can create a review cycle, populate one review per active employee, record a 1–5 rating with strengths/improvements, and close the cycle. Employees can read their own reviews via API.

## Schema
`migrations/20261203000001_performance_reviews.sql`:
- `performance_cycles(id, tenant_id, name, period_start, period_end, status, created_by, created_at, updated_at)`.
  - name 1–200, `end >= start`, status in `draft|active|closed`.
- `performance_reviews(id, tenant_id, cycle_id, employee_id, reviewer_user_id, status, rating, strengths, improvements, submitted_at, created_at, updated_at)`.
  - status in `pending|submitted`; rating 1–5 or NULL; strengths/improvements ≤2000; unique `(cycle_id, employee_id)`.
- indexes `(tenant_id, status)` and `(tenant_id, cycle_id, status)`; RLS forced + tenant policies.

## Service (`talentos-hr-runner/src/performance.rs`)
- `CycleInput` / `CycleView` (review_count, submitted_count) / `ReviewView` (employee code/name, cycle name/period).
- `list_cycles`, `create_cycle` (draft).
- `start_cycle` — draft only; insert pending reviews for active (non-terminated) employees with `ON CONFLICT DO NOTHING`; set active.
- `close_cycle` — active only; delete cycle.
- `list_reviews(cycle_id)`.
- `submit_review(review_id, rating, strengths, improvements)` — only while the cycle is active; 422 on rating outside 1–5 / long text.
- `list_my_reviews(user)` — self, tenant scoped.
- `PerformanceError` with 404/409/422 mapping.

## Endpoints
- `GET/POST /api/v1/performance/cycles` (`employee.read_all` / `employee.update`).
- `POST /api/v1/performance/cycles/:id/start|close`, `DELETE /api/v1/performance/cycles/:id` (`employee.update`).
- `GET /api/v1/performance/cycles/:id/reviews` (`employee.read_all`).
- `PATCH /api/v1/performance/reviews/:id` (`employee.update`).
- `GET /api/v1/me/reviews` (self).

## UI
- `/performance` (nav “ประเมินผล”, HR writes; payroll read-only):
  - cycle create form (name/start/end), cycle list with status and review counts, start/close/delete;
  - selected cycle review table (`review-row`) with employee, status, rating select, strengths/improvements inputs, submit (`submit-review`), submitted badge.
- Shell: nav item + `hrOnly` guard.
- PDPA export: add reviews per employee.

## Tests
- DB: cycle create/validation, start populates active employees only and is idempotent, submit rating/text/status, close blocks submit, tenant isolation, self list only own.
- E2E: HR creates a cycle, starts it, rates E2E001 and submits, sees submitted; payroll can open read-only; employee is redirected.
- Gates: fmt/clippy/tests/build, Playwright 51/51 (or final count).

## Out of scope
- No goals/OKR tracking, manager workflow, calibration, notifications, or employee acknowledgement UI (self API only).

# Recruitment Pipeline Plan

**Goal:** HR can open job requisitions, add candidates, and move them through a simple pipeline (applied → screening → interview → offer → hired/rejected).

## Schema
`migrations/20261201000001_recruitment.sql`:
- `job_openings(id, tenant_id, title, department, employment_type, headcount, status, description, created_by, created_at, updated_at)`.
  - title 1–200, employment_type in `full_time|part_time|contract|intern`, status in `open|closed`, headcount 1–100, description ≤2000.
  - index `(tenant_id, status, created_at DESC)`.
- `candidates(id, tenant_id, job_opening_id, first_name, last_name, email, phone, stage, source, note, created_by, created_at, updated_at)`.
  - stage in `applied|screening|interview|offer|hired|rejected`, names 1–100, email ≤200, phone ≤30, source ≤100, note ≤2000.
  - index `(tenant_id, job_opening_id, stage)`.
- RLS forced + tenant policies on both tables.

## Service (`talentos-hr-runner/src/recruitment.rs`)
- `list_jobs(pool, tenant)` → jobs with `candidate_count` and `hired_count`.
- `create_job(pool, tenant, user, input)`; `set_job_status(pool, tenant, id, open: bool)`; `delete_job` (candidates cascade).
- `list_candidates(pool, tenant, job_id)` ordered by stage rank then created_at.
- `create_candidate`; `set_candidate_stage`; `delete_candidate`.
- Validation + `RecruitmentError` (NotFound, InvalidTitle/Type/Status/Stage/Name/Email/Note, Db).
- `JobView`/`CandidateView` serialized; no payroll/timesheet changes.

## Endpoints
- `GET/POST /api/v1/jobs` (`employee.read_all` / `employee.update`).
- `PATCH/DELETE /api/v1/jobs/:id` (`employee.update`).
- `GET/POST /api/v1/jobs/:id/candidates` (`employee.read_all` / `employee.update`).
- `PATCH/DELETE /api/v1/candidates/:id` (`employee.update`).
- Stage body `{stage}`; job status body `{status: "open"|"closed"}`.

## UI
- `/recruitment` (nav “สรรหาบุคลากร”, HR writes; payroll read-only via `employee.update`):
  - job list with status/headcount/candidate counts, create form, close/reopen/delete;
  - selected job candidate list, add candidate form, stage `<select>` per row, delete;
  - Thai labels, empty states, error handling.
- Shell: nav item + `hrOnly` guard.

## Tests
- DB: create job, candidate counts, stage ordering/update, invalid stage/title, job status toggle, delete cascades, tenant isolation (cross-tenant job/candidate by id), payroll write blocked at service level is covered by E2E route guard.
- E2E: HR creates an opening, adds a candidate, moves applied → interview, sees it; payroll can open the page read-only; employee is redirected.
- Gates: fmt/clippy/tests/build, Playwright 49/49 (or final count).

## Out of scope
- No file attachments/CVs, no interview scheduling, no offer-letter/e-signature, no automatic employee creation on hire, no stage history table.

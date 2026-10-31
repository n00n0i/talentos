# Candidate Interview Scheduling Plan

**Goal:** Schedule interviews for a candidate (time, mode, location/link, interviewer, notes), track status, and list them per candidate.

## Schema
`migrations/20261207000001_candidate_interviews.sql`:
`candidate_interviews(id, tenant_id, candidate_id, scheduled_at, mode, location, interviewer_user_id, status, notes, created_by, created_at, updated_at)`
- mode in `onsite|online|phone`; status in `scheduled|completed|cancelled`.
- location ≤500, notes ≤1000.
- candidate FK `ON DELETE CASCADE`; indexes `(tenant_id, scheduled_at)` and `(tenant_id, candidate_id)`; RLS forced + tenant policy.

## Service (`talentos-hr-runner/src/recruitment.rs`)
- `InterviewInput {scheduled_at, mode, location?, interviewer_user_id?, notes?}` / `InterviewView`.
- `schedule_interview` — candidate must belong to tenant; mode validated; status defaults scheduled.
- `list_interviews(candidate_id)` ordered by `scheduled_at` (newest first).
- `set_interview_status(id, status)`.
- `delete_interview(id)`.
- Errors: InvalidMode/Status/Location/Notes, CandidateNotFound, InterviewNotFound.
- Deleting a candidate/job cascades interviews.

## Endpoints
- `GET/POST /api/v1/candidates/:id/interviews` (`employee.read_all` / `employee.update`).
- `PATCH/DELETE /api/v1/interviews/:id` (`employee.update`).

## UI
- `/recruitment` candidate rows gain “สัมภาษณ์” (`candidate-interview-button`); opens a card (`candidate-interviews`) with:
  - schedule form: `datetime-local`, mode, location/link, interviewer user id (optional), notes;
  - interview list (`interview-row`) with time, mode, location, status select, delete.
- No new nav/route.

## Tests
- DB: schedule/list/status/delete, mode/status/location validation, candidate/tenant scoping, cascade on candidate delete.
- E2E: schedule an online interview for the candidate, see it listed, mark completed, see status.
- Gates: fmt/clippy/tests/build, Playwright 53/53 (or final count).

## Out of scope
- No calendar integration, invites/notifications, interviewer conflict checks, or offer letters.

# Candidate Stage History Plan

**Goal:** Keep an immutable audit trail of candidate stage moves (who moved it, from/to, when) and show it per candidate.

## Schema
`migrations/20261206000001_candidate_stage_events.sql`:
`candidate_stage_events(id BIGSERIAL, tenant_id, candidate_id, from_stage NULL, to_stage, changed_by NULL, changed_at)`
- `candidate_id` FK `ON DELETE CASCADE`; indexes `(tenant_id, candidate_id, changed_at)`; RLS forced + tenant policy.

## Service (`talentos-hr-runner/src/recruitment.rs`)
- `StageEventView { id, from_stage, to_stage, changed_by, changed_at }`.
- `create_candidate` inserts `NULL → applied` in the same transaction.
- `set_candidate_stage` inserts `old → new` only when the stage actually changes (same stage is a no-op).
- `list_stage_history(pool, tenant, candidate_id)` — newest first; candidate must belong to the tenant.
- Deleting a candidate/job cascades the events.
- `RecruitmentError` unchanged (NotFound on missing candidate).

## Endpoint
- `GET /api/v1/candidates/:id/history` (`employee.read_all`).

## UI
- `/recruitment` candidate row: “ประวัติ” button (`candidate-history-button`); loads history into a card (`candidate-history`) listing `from → to`, time, and actor user id (short) newest first.

## Tests
- DB: create writes `applied`; stage change writes an event with from/to; same stage writes none; tenant scoping; delete cascades.
- E2E: move a candidate applied → interview, open history, see both entries.
- Gates: fmt/clippy/tests/build, Playwright 53/53 (or final count).

## Out of scope
- No actor display name (user id only), no revert/undo, no interview scheduling/offer letters.

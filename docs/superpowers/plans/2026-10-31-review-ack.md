# Performance Self-Service & Acknowledgement Plan

**Goal:** Employees see their own submitted performance reviews and acknowledge them; HR sees the acknowledged state.

## Schema
`migrations/20261209000001_review_ack.sql`:
- add `performance_reviews.acknowledged_at TIMESTAMPTZ`;
- extend the status check with `acknowledged` (`pending|submitted|acknowledged`).

## Service (`talentos-hr-runner/src/performance.rs`)
- `ReviewView` gains `acknowledged_at`.
- `acknowledge_review(pool, tenant, user, review_id)` — only the employee linked to the review, status `submitted`; sets `acknowledged` + `acknowledged_at`; idempotent once acknowledged; `pending` → `ReviewNotSubmitted` (409); foreign/unknown → NotFound.
- `list_my_reviews` already filters by `employees.user_id`.
- PDPA export includes `acknowledged_at`.

## Endpoints
- `POST /api/v1/me/reviews/:id/acknowledge` (self).
- existing `GET /api/v1/me/reviews`.

## UI
- New `/me/reviews` page (nav “ผลประเมินของฉัน”, all roles): cycle name/period, rating, strengths/improvements, status badge, acknowledge button (`acknowledge-review`) for submitted reviews; acknowledged shows the date.
- `/performance` HR table shows “รับทราบแล้ว” for acknowledged reviews.

## Tests
- DB: acknowledge own submitted review, idempotent timestamp, pending rejected, foreign tenant/other user 404, self list reflects the status, PDPA export field.
- E2E: after HR submits E2E001’s review, the employee opens `/me/reviews`, sees rating 4, acknowledges, sees the acknowledged badge.
- Gates: fmt/clippy/tests/build, Playwright 53/53 (or final count).

## Out of scope
- No employee self-rating, comments/disputes, calibration, or notifications.

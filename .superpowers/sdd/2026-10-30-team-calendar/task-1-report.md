# Team Calendar — Task 1 Report

**Status:** Completed
**Branch:** `feat/calendar`
**Commit:** `c1af52e` — `feat(calendar): team calendar view (holidays, leave, OT)`
**Report path:** `.superpowers/sdd/2026-10-30-team-calendar/task-1-report.md`

## What changed

### Backend
- `crates/services/talentos-hr-runner/src/calendar.rs` (new): `calendar(pool, tenant, from, to)`.
  - Validates range: `from ≤ to` and span ≤ 62 days (`MAX_CALENDAR_RANGE_DAYS`), else `BadRange`/`TooWide`.
  - Tenant-scoped reads of `tenant_holidays`, `leave_requests` (status pending/approved, overlap via `start_date ≤ to AND end_date ≥ from`) and `ot_requests` (work_date in range, pending/approved).
  - `label_th` resolved via a correlated subquery (tenant override first, then global default, else the raw type).
  - Returns `Vec<CalendarDay>` ascending; **days with no holiday/leave/OT are omitted** (documented in module doc).
- `crates/services/talentos-hr-runner/src/lib.rs`: re-exports `calendar`, `CalendarDay`, `CalendarError`, `CalendarHoliday`, `CalendarLeave`, `CalendarOt`.
- `crates/api/gateway/src/handlers/calendar.rs` (new): `GET /api/v1/calendar?from=&to=`, gated on `attendance.read`; maps `BadRange`/`TooWide` to 400, DB errors to 500.
- `router.rs` / `handlers/mod.rs`: route registered.

### UI
- `web/app/(app)/calendar/page.tsx` (new): Thai month input (`calendar-month`), day cards (`calendar-day`) showing holiday names, leave chips (`label_th` + employee + status), OT minutes, empty state, `handleApiError`.
- `web/components/Shell.tsx`: nav “ปฏิทินทีม” (`/calendar`); `/calendar` added to the `attendanceOnly` route guard and to the non-HR nav filter keyed on `attendance.read`.

### E2E
- `web/e2e/app.spec.ts`: test 39 creates a holiday (`holiday-date`/`holiday-name`/`add-holiday`) for 2026-11-25, opens `/calendar`, sets `calendar-month` to 2026-11, expects “วันลอยกระทง”.
- `web/e2e/roles.spec.ts`: employee `/calendar` → `/clock` (and no “ปฏิทินทีม” link); manager sees the link and can open `/calendar`.

## Tests

- `talentos-hr-runner --test calendar` (DB): 2 passed (day bucketing + range validation).

## Gate results

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p talentos-hr-runner -p talentos-gateway --all-targets -- -D warnings` | clean |
| focused DB tests (hr-runner calendar) | 2 passed |
| `npm run build` (web) | built (`/calendar` route present) |
| Playwright (44 tests; empty DB + `/setup` token, no seed; gateway 18449 / web 3153) | **44 passed** |

## Concerns

- Empty days (no holiday/leave/OT) are omitted from the response; the UI treats a whole empty month as the empty state. If a future view needs contiguous day cells, the backend must emit empty buckets for every date in range.
- No new tables or payroll changes; the endpoint reads existing tables only.

## Out of scope

- No pagination (range is capped at 62 days, so a full month's data is bounded).
- No leave spanning across multiple day buckets: a leave request is listed only under its `start_date` (matching the pinned shape), not repeated on every day it overlaps.

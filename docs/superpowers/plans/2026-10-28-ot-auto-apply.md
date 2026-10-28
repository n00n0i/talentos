# OT Auto-Apply to Attendance/Payroll Plan

**Goal:** An approved OT request for a date raises that day's payroll OT to `max(derived OT, approved requested minutes)` without double counting; rejection/cancel restores the derived value.

**Files:** migration adds `attendance_days.derived_ot_minutes`; `attendance.rs` recompute + public reapply; `ot.rs` decision hook; tests; attendance UI note.

## Rules
- `derived_ot_minutes` stores the punch-derived value; `ot_minutes` is the effective value used by payroll.
- On recompute: `effective = max(derived, sum(approved OT request minutes for that employee/date))`.
- On approve/reject/cancel of an OT request: if an attendance day exists and is `pending`, update `ot_minutes` only (keep status); if the day is `approved` and the effective value would change, return 409 (`day already approved`); if no day, no change.
- Payroll runner unchanged; it already reads `ot_minutes` from approved days.
- Request minutes are a cap/floor: actual punches above the request still count (max).
- Migration backfills `derived_ot_minutes = ot_minutes`.
- No double count: sum of approved requests, max against derived, not addition.

## Tests
- Recompute with an approved 120-min request and 60 derived → 120 effective; derived stays 60.
- Approve after day pending → effective raises; reject/cancel → back to derived.
- Approved day + request that changes OT → 409; request that does not exceed derived → no-op success.
- Existing attendance/payroll tests unchanged.

## Gates
fmt/clippy/tests, build, Playwright 21/21; no payroll math change beyond the effective day value.

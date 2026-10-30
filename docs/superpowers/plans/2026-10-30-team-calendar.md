# Team Calendar Plan

**Goal:** One month view for HR/managers showing leave, approved OT, and public holidays per day.

## Backend
- `GET /api/v1/calendar?from=YYYY-MM-DD&to=YYYY-MM-DD` (`attendance.read`): returns
  `{days:[{date, holidays:[{name}], leave:[{employee_code,employee_name,leave_type,label_th,start_date,end_date,status}], ot:[{employee_code,employee_name,minutes,status}]}]}`
  for leave overlapping the range and approved/pending OT on dates in range.
- Tenant-scoped, limited to 62 days, 400 if invalid/too wide.
- No new tables.

## UI
- `/calendar` page (HR/manager nav “ปฏิทินทีม”): month picker, day cards/list with holiday, leave chips, OT minutes; empty state; Thai dates.
- Route guard: attendance.read.

## E2E
- HR creates a holiday in `/attendance`, opens `/calendar`, sees it on that day; employee cannot open `/calendar` (redirect).

## Gates
fmt/clippy/tests/build, Playwright 43/43.

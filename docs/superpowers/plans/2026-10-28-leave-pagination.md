# Leave History Pagination Implementation Plan

**Goal:** An employee can page through leave history instead of silently losing rows after 200; pending requests still come first.

**Files:** `crates/services/talentos-hr-runner/src/leave.rs` (`list_requests`), `crates/api/gateway/src/handlers/leave.rs` (`me_leave`, `requests` query), `web/app/(app)/leave/page.tsx`, tests in `tests/leave_self.rs`.

**Interface:** `GET /api/v1/me/leave?limit=&offset=` and `GET /api/v1/leave/requests?employee_id=&status=&limit=&offset=`; limit 1..=100 (default 50), offset ≥ 0; response adds `has_more`. Order `(status='pending') DESC, start_date DESC, created_at DESC`; stable.

**Tests:** seed 60 requests; page 1/page 2 no overlap; pending-first; limit clamp; negative offset 400; ownership unchanged.

**UI:** “โหลดเพิ่ม” button appends the next page; hide when `has_more` false; keep the existing cards/form.

**Constraints:** no schema change, no payroll change, RLS, Thai UI, no production unwrap/expect, fmt/clippy/tests, Playwright 20/20.

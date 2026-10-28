# Leave Pagination Implementation Report

**Status:** Completed  
**Commit:** `22ecffc` (`feat/leave-page`)  
**Report path:** `.superpowers/sdd/2026-10-28-leave-pagination/task-1-report.md`

## What changed

- `crates/services/talentos-hr-runner/src/leave.rs`
  - `list_requests` now takes `limit`, `offset`, clamps limit to `1..=100` (default 50), treats negative offset as 0.
  - Orders by `(status = 'pending') DESC, start_date DESC, created_at DESC`.
  - Fetches `limit + 1` rows to compute `has_more` and returns `ListRequestsResult { requests, has_more }`.
- `crates/api/gateway/src/handlers/leave.rs`
  - `GET /api/v1/me/leave?limit=&offset=` returns `{ employee_linked, balances, requests, has_more }`.
  - `GET /api/v1/leave/requests?employee_id=&status=&limit=&offset=` returns `{ requests, has_more }`.
  - Invalid limit (not 1–100) or negative offset returns 400.
- `web/app/(app)/leave/page.tsx`
  - Loads first page with `limit=50&offset=0`.
  - Adds Thai "โหลดเพิ่ม" button (`data-testid="load-more-leave"`) that appends next page; hidden when `has_more` is false.
- `crates/services/talentos-hr-runner/tests/leave_self.rs`
  - Added `pagination_orders_and_pages_requests` test: seeds 60 rows, checks page boundaries, pending-first ordering, limit clamping, last-page `has_more=false`, negative-offset defence, and status filter.
- `scripts/init-db.sql`
  - Added missing default sequence privileges for `talentos_auth` so login/refresh/audit writes work after a fresh DB reset.

## Verification

| Gate | Result |
|------|--------|
| `cargo fmt --all` | ✅ clean |
| `cargo clippy -p talentos-hr-runner -p talentos-gateway --all-targets -- -D warnings` | ✅ clean |
| Focused DB tests (`leave_self`, `leave_approval`) | ✅ 27 passed |
| `npm run build` | ✅ built; static assets copied to `.next/standalone/.next/static` |
| Playwright e2e (`E2E_BASE_URL=http://127.0.0.1:18269`) | ✅ 20/20 passed |

Test environment:
- DB container: `loans-postgres-1`
- Gateway port: `3135`
- Web port: `18269`
- Extra env required for 2FA test: `PII_ENCRYPTION_KEY=<64-hex-chars>`

## Concerns / risks

- The standalone Next.js server needs `.next/static` copied into `.next/standalone/.next/static`; this is a deployment/runtime detail, not a code change.
- `init-db.sql` change is a dev-env fix; it does not affect the application logic but is required for clean e2e re-seeds.
- No `query!` macro changes were introduced, so `cargo sqlx prepare` was not needed.
- Servers were stopped after the e2e run.

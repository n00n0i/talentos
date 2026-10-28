# Notification Admin & Retry Implementation Plan

**Goal:** HR can see the notification outbox (pending/failed/sent counts and rows) and retry a failed delivery.

**Files:** `crates/api/gateway/src/handlers/notifications.rs`, `router.rs`, `web/app/(app)/settings/page.tsx`, E2E.

## Interfaces
- `GET /api/v1/notifications?status=&limit=` (`audit_log.read`/hr_admin) → `{summary:{pending,failed,sent}, rows:[{id,channel,recipient_masked,subject,status,attempts,last_error,created_at,sent_at}]}`.
- `POST /api/v1/notifications/:id/retry` → reset a `failed` row to `pending` (attempts 0, last_error null); 409 if not failed; 404 if cross-tenant.
- Recipient masked (`a***@domain`, LINE `****1234`); never return full addresses or secrets.
- Summary counts tenant-wide; rows newest first, limit 1..=100 default 50.

## UI
Settings “การแจ้งเตือน” card: summary chips, failed table with masked recipient/subject/error/attempts, `retry-notification` button; reload after retry. No new route.

## Tests
DB unit not available for gateway; E2E test 21: HR opens `/settings`, sees the notification summary and (empty/rows) table without error. Backend logic exercised by the existing outbox tests; retry manually verified in E2E by seeding? If no failed row exists, assert the empty state.

## Constraints
RLS/begin_tenant, hr_admin only, no secrets/full addresses, no worker change, Thai UI, fmt/clippy/tests, build, Playwright 21/21.

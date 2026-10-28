# Notification Admin & Retry — Task 1 Report

## Status
Completed.

## Commit
`270ed6a` — `feat(notifications): admin outbox summary, failed rows, and retry`

## Changes
- `crates/api/gateway/src/handlers/notifications.rs` (new)
  - `GET /api/v1/notifications?status=&limit=` — `audit_log.read` gate (hr_admin only).
  - Tenant-scoped via `begin_tenant`; summary counts for `pending`/`sent`/`failed`; rows newest first.
  - Recipient masking: email `a***@domain.com`, LINE `****1234`.
  - `limit` clamped to 1..=100, default 50; optional status filter validated.
  - `POST /api/v1/notifications/:id/retry` — resets only `failed` rows to `pending` with `attempts = 0`, `last_error = NULL`; returns 409 otherwise, 404 cross-tenant.
  - Unit tests for masking helper and status validation.
- `crates/api/gateway/src/handlers/mod.rs` — exported `notifications` module.
- `crates/api/gateway/src/router.rs` — wired `notifications::list` and `notifications::retry` routes.
- `web/app/(app)/settings/page.tsx`
  - New "การแจ้งเตือน" card with summary chips (`รอส่ง`/`ล้มเหลว`/`ส่งแล้ว`).
  - Failed rows table: masked recipient, subject, status, attempts, last error, retry button (`data-testid="retry-notification"`).
  - Empty state (`data-testid="notify-empty"`) and reload after retry.
- `web/e2e/app.spec.ts`
  - Test 21: HR opens `/settings`, sees the notification summary card and either rows or the empty state without error.

## Test Summary
```
cargo fmt --all                                       ok
cargo clippy -p talentos-gateway --all-targets -- -D warnings  ok
cargo test -p talentos-gateway                        54 passed, 0 failed
npm run build (web/)                                  ok
Playwright e2e (21 tests, ports 18279/3136)           21 passed, 0 failed
```

## Concerns
- First E2E run failed test 12 (2FA enrollment) because `PII_ENCRYPTION_KEY` was unset, causing `/api/v1/auth/2fa/setup` to return 503. Re-running the gateway with a generated `PII_ENCRYPTION_KEY` made the full suite pass. Documenting this here because the task gates did not explicitly list the env var but the existing 2FA test requires it.
- No `query!` macros were added; queries are runtime `sqlx::query`, so `cargo sqlx prepare` was not needed.
- E2E relies on `loans-postgres-1` and `talentos-redis-1` containers already running; the test harness re-seeded `scripts/seed-e2e.sql` before the final run.

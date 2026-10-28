# 2FA Recovery Codes & Session Management — Task 1 Report

## Status
Completed.

## Commit
`cc435b4` — `feat(auth): 2FA recovery codes and session management`

## Changes
- `migrations/20261111000001_recovery_sessions.sql`
  - New `totp_recovery_codes` table: `id`, `tenant_id`, `user_id`, `code_hash BYTEA`, `used_at`, `created_at`, `UNIQUE(user_id, code_hash)`.
  - RLS `FORCE` + tenant isolation policy.
- `crates/core/authz/src/recovery_codes.rs` (new)
  - Generates 10 codes per user: 8 base32 chars formatted `XXXX-XXXX`.
  - `normalize()` strips dashes/spaces/lowercase; `hash_code()` returns SHA-256 bytes.
  - Unit tests for generation/format/hash and normalization.
- `crates/api/gateway/src/handlers/auth.rs`
  - `enable_2fa` now generates/rotates recovery codes in the enabling transaction and returns `recovery_codes` once.
  - New `POST /api/v1/auth/2fa/recovery-codes` handler regenerates codes after a valid TOTP.
  - Login accepts either a TOTP code or an unused recovery code; recovery codes are normalized, hashed, and marked `used_at = now()` atomically.
  - New session endpoints using `auth_pool` and never exposing token hashes:
    - `GET /api/v1/auth/sessions`
    - `DELETE /api/v1/auth/sessions/:id`
    - `POST /api/v1/auth/sessions/revoke-all`
  - `disable_2fa` deletes recovery codes.
- `crates/api/gateway/src/handlers/users.rs`
  - `reset_2fa` deletes the target user's recovery codes.
- `crates/api/gateway/src/router.rs`
  - Added routes for recovery-code regeneration and session management.
- `crates/api/gateway/src/dto/requests.rs`
  - `LoginRequest.validate()` now accepts 6-digit TOTP or a valid recovery-code format.
- `web/app/(app)/settings/page.tsx`
  - Shows recovery codes once after enable with Thai warning.
  - "สร้างรหัสสำรองใหม่" with TOTP input and regenerate button.
  - Sessions table with per-row revoke and "ออกจากทุกอุปกรณ์" revoke-all.

## Test Summary
```
cargo fmt --all                                       ok
cargo clippy -p talentos-gateway --all-targets -- -D warnings  ok
cargo clippy -p talentos-authz --all-targets -- -D warnings    ok
cargo test -p talentos-authz -p talentos-gateway      89 passed, 0 failed
npm run build (web/)                                  ok
Playwright e2e (20 tests, ports 18259/3134)           20 passed, 0 failed
```

## Concerns
- The DB migration was applied manually to the `loans-postgres-1` container because `_sqlx_migrations` did not exist; I baselined the table and inserted the new migration record. CI/production should run `sqlx migrate run` normally once `_sqlx_migrations` is in sync.
- `_sqlx_migrations` checksums for older migrations differ from `sqlx migrate info`'s local computation in this environment; this predates my change and does not affect runtime, but a fresh `sqlx migrate revert`/`run` cycle may need the migration table rebuilt.
- No new integration tests were added in `tests/integration`; coverage is via unit tests and the existing Playwright 2FA flow. Adding backend integration tests for recovery-code single-use and rotation would strengthen the slice.

---

# Review Fixes

## Commit
`8581d70` — `fix(auth): populate session metadata, tenant-scoped session queries, recovery-code E2E`

## Changes
- `crates/api/gateway/src/handlers/auth.rs`
  - `store_refresh_token` now binds `ip_address` and `user_agent` from the request.
  - `login` and `refresh` extract `ConnectInfo` IP and `User-Agent` header and pass them through; refresh rotation CTE inserts metadata for the successor token.
  - `list_sessions`, `revoke_session`, and `revoke_all_sessions` now run inside `state.pool.begin_tenant(ctx.tenant_id)` so RLS applies.
  - `verify_recovery_code` moved from `auth_pool` to a tenant-scoped transaction (tenant is already known at that point).
- `crates/core/authz/src/recovery_codes.rs`
  - Added rationale comment for unsalted SHA-256 (40-bit single-use random codes).
  - `generate_codes` deduplicates with a `HashSet` and loops until 10 unique codes are produced.
- `web/app/login/page.tsx`
  - TOTP input now accepts recovery codes: `maxLength={9}`, pattern `[A-Z0-9\-]{6,9}`.
- `web/e2e/app.spec.ts`
  - Test 12 now reads a recovery code from the UI after enabling 2FA, logs in once with it, asserts reuse fails and keeps the user on the login page, then logs back in with a fresh TOTP and disables 2FA to keep the suite replayable.

## Test Summary
```
cargo fmt --all                                       ok
cargo clippy -p talentos-gateway -p talentos-authz --all-targets -- -D warnings  ok
cargo test -p talentos-authz -p talentos-gateway      89 passed, 0 failed
npm run build (web/)                                  ok
Playwright e2e (20 tests, ports 18259/3134)           20 passed, 0 failed
```

## Concerns
- Same `_sqlx_migrations` baseline note as above; the runtime behavior is verified by tests and e2e.
- The `pkill` helper in my local server teardown scripts can hit the surrounding shell; I used standalone `/tmp/*.sh` scripts for the final run to avoid self-termination.

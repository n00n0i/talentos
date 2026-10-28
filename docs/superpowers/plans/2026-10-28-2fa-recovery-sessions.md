# 2FA Recovery Codes & Session Management Implementation Plan

**Goal:** A 2FA user can recover access with one-time backup codes, and can see/revoke their active refresh sessions.

**Architecture:** `totp_recovery_codes` stores SHA-256 hashes (10 per user, one-time). Enabling 2FA returns the plaintext codes once; regenerating requires a current TOTP code. The existing `refresh_tokens` rows become the session list; revoking one sets `revoked_at`. Login accepts either a TOTP code or an unused recovery code.

**Files:** migration; `auth.rs` (enable/regenerate/login/session endpoints); `users.rs` reset/disable cleanup; router; RBAC unchanged; `web/app/(app)/settings/page.tsx`; tests.

## Constraints

- Recovery codes are 8 base32 chars formatted `XXXX-XXXX`, 10 per user, hashed at rest, single-use, never logged.
- Regeneration requires the current TOTP code; disable/admin reset deletes codes.
- Session list is the caller's own active refresh tokens only; revoke is tenant/user scoped; never expose hashes.
- `/auth/sessions` uses the access token (no refresh token required); revoking a session does not revoke the caller's access token immediately.
- No production unwrap/expect; Thai UI; `cargo fmt/clippy/test`, `npm run build`, Playwright 20/20 (no new browser flow).

## Endpoints

- `POST /api/v1/auth/2fa/recovery-codes` `{code}` → `{codes:[...]}` (regenerate).
- Enable response adds `recovery_codes` on first enable.
- `GET /api/v1/auth/sessions` → `{sessions:[{id,created_at,expires_at,ip,user_agent}]}`.
- `DELETE /api/v1/auth/sessions/:id` → 200.
- `POST /api/v1/auth/sessions/revoke-all` → 200.

## Tests

- TOTP enable returns 10 codes; DB stores only hashes.
- Login with a recovery code succeeds once; reuse fails.
- Regeneration requires a valid TOTP code and invalidates old codes.
- Sessions list excludes revoked/expired; revoke one; revoke-all.
- Disable/reset deletes recovery codes.

## UI

Settings security card: show recovery codes once with a copy hint; “สร้างรหัสสำรองใหม่” with TOTP input; sessions table with revoke buttons and “ออกจากทุกอุปกรณ์”.

## Out of scope

Passkeys/WebAuthn, device naming, per-session access-token revocation, recovery-code download as a file.

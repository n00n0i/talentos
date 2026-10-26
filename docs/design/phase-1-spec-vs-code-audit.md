# TalentOS Phase 1 — Spec vs Code Audit

> Date: 2026-10-26 · Scope: all 4 crates + migrations, checked against `docs/design/phase-1-spec.md` and `phase-1-test-cases.md`.
> Verdict: **not deployable**. The auth core (JWT, Argon2, RBAC) is solid; the gateway wiring has blockers that would break in production.

## Blockers (must fix before ship)

### B1. Migration schema ≠ handler queries → will not compile / will crash at runtime
`migrations/20261025000002_refresh_tokens.sql` defines `token_hash TEXT PRIMARY KEY`, no `parent_hash`, no `id`, no `created_at`/`ip_address`/`user_agent`, and misses `FORCE ROW LEVEL SECURITY`.
But `handlers/auth.rs` binds `.as_bytes()` (BYTEA) to `token_hash`, SELECTs/INSERTs/WHEREs on `parent_hash` (lines 190–293), and relies on family-walk for token-theft revocation (TC-18).
**Fix**: rewrite migration per spec §7 (`BYTEA NOT NULL UNIQUE`, `parent_hash BYTEA`, `BIGSERIAL id`, `FORCE RLS`) and rename file to `20261025000003_...` — two files currently share prefix `20261025000002` which makes sqlx ordering ambiguous.

### B2. Wrong env var names throughout `main.rs`
Spec Appendix A: `AUTH_JWT_SECRET`, `AUTH_ACCESS_TTL_SECS`, `AUTH_REFRESH_TTL_DAYS`, `GATEWAY_PORT`, `AUTH_ALLOWED_ORIGINS`.
Code reads: `JWT_SECRET`, `ACCESS_TTL_SECS`, `REFRESH_TTL_DAYS`, `BIND_ADDR`. Deploy with a compliant `.env` → process panics at boot or silently uses defaults.
Also: `REFRESH_TTL_DAYS` default is `"30"`, spec §2 says **7 days**. And no ≥32-byte entropy check on the secret (spec §6.8 "refuse to boot").

### B3. Redis never connects to `REDIS_URL`
`main.rs:51-56` builds `RedisConfig::default()` (localhost) and discards `redis_url` via `let _ = &redis_url;`. In any non-local deployment every rate-limit and health-check call fails. The inline `shortcut:` comment acknowledges it but this is not a Phase-1 shortcut — it's a broken dependency wire.

### B4. Rate limiter fails CLOSED, spec mandates fail OPEN
`ratelimit.rs:16-21` returns `Err(60)` when Redis errors → all logins blocked during Redis outage. Spec §6.5 explicitly: *"if Redis is unreachable, allow requests through but log error loudly."* Availability requirement inverted.

### B5. `/auth/me` routed at wrong path
`router.rs:17` registers `/api/v1/me`; spec §4.3 defines `/api/v1/auth/me`. TC-02 asserts the latter. One-line fix.

### B6. CORS layer missing entirely
Spec §5 step 2 requires `tower_http::cors::CorsLayer` driven by `AUTH_ALLOWED_ORIGINS`. Not present anywhere in `router.rs`. Browser clients cannot call the API. TC-25 fails.

### B7. Silent swallowing of DB errors
`auth.rs:197, 297, 320` use `.unwrap_or(None)` / `.unwrap_or_default()` on sqlx results. A pool exhaustion or PG outage surfaces as `401 invalid credentials` instead of `503`. Worse, in `revoke_family` a failed UPDATE looks like "family revoked" — a security-relevant lie. Map sqlx errors to a distinct internal-error arm returning 500.

## Major gaps (spec-required behavior absent)

| # | Missing | Spec ref | Impact |
|---|---------|----------|--------|
| M1 | Login success does not update `users.last_login_at` nor insert `audit_log` row | §4.1 side effects | TC-01 assertion fails |
| M2 | No rejection of zero-role users at login | §6.3 | TC-22 fails; role-less user gets valid tokens |
| M3 | Validation error code is `"validation_error"` not `"invalid_request"` | §4.1/§4.5 | TC-20 fails |
| M4 | IP rate window is 15 min; spec says 10 req/**minute**; email lockout semantics differ from §6.5 | §6.5 | TC-15/TC-16 flaky-or-fail |
| M5 | Health checks return `"up"/"down"`; spec §4.4 says `"ok"/"error"` | §4.4 | TC-04 string assert fails |
| M6 | Refresh endpoint has no rate limiting at all | §5 step 3 applies to all `/auth/*` | brute-force vector open |
| M7 | `jwt.rs:70-74` maps `InvalidToken` → `TokenSignatureInvalid`; garbage strings should be `TokenMalformed` | §4.5 error taxonomy | TC-06 vs TC-07 indistinguishable |
| M8 | `db_err()` maps *every* SQL error (incl. connection loss) to `InvalidCredentials` → 401 | — | outages masquerade as bad passwords |

## Minor / acceptable deviations

- `issue_access_token(ttl_secs: i64)` vs spec's `Duration` — cosmetic.
- Fixed-window instead of sliding-window rate limit — documented shortcut, acceptable Phase 1.
- Non-atomic INCR+EXPIRE in ratelimit — documented shortcut, acceptable.
- `Authed` extractor uses RPITIT instead of `#[async_trait]` — correct for axum 0.7, better than spec snippet.
- Atomic CTE rotation in refresh is *stronger* than spec's transaction wording — keep it.
- `expand_permissions()` added beyond spec API surface — justified by §4.3 response shape.

## What is genuinely good

- `sqlx::query!` everywhere — compile-time-checked SQL, no injection surface (§B.1 satisfied).
- Timing equalization done right: dummy Argon2 verify on unknown-user AND on inactive-account paths (`auth.rs:114, 119`). TC-12 well-covered.
- Generic `"Invalid email or password"` message unifies InvalidCredentials + AccountDisabled. Correct.
- Family-walk revocation logic matches §2 threat model exactly (modulo B1 schema gap).
- Kernel crate stays dependency-free as advertised.
- Unit tests: jwt 8, password 4, rbac 9 — cover round-trip, expiry, tamper, wrong-key, leeway, cost-parity, matrix edges. Good baseline.

## Recommended fix order

1. Rewrite refresh_tokens migration (B1) — unblocks everything else.
2. Env var renames + TTL default + secret length check (B2).
3. Wire Redis properly (B3); flip rate limiter to fail-open (B4).
4. Router: fix `/auth/me` path, add CorsLayer, add rate-limit middleware covering both `/auth/login` and `/auth/refresh` (B5, B6, M6).
5. Replace `unwrap_or(None)` with proper error arms (B7, M8).
6. Add last_login_at + audit_log writes, zero-role rejection, error-code renames, health strings (M1–M5).
7. Re-run `cargo test --workspace` then the integration suite against docker-compose.

## Risk notes

- Cannot verify compilation locally: `sqlx::query!` needs a live PG with the corrected schema (`DATABASE_URL` + `sqlx prepare`). Expect several additional type errors once B1 is fixed — budget time for that pass.
- Two migration files sharing version prefix may already have been applied out-of-order on some dev database; coordinate a clean re-migrate before trusting any local testing.

Skipped: did not run cargo build/test (needs live PG for sqlx macros), did not review `middleware/{logging,cors}.rs` bodies beyond registration, did not check CI config or Dockerfiles. Biggest residual risk is undiscovered type mismatches surfacing only at first real compile after the migration rewrite.
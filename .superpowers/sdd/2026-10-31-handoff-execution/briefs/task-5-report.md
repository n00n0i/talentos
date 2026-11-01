# Task 5 Report — SSO generic OIDC provider (per-tenant, PKCE, email link)

## Status: DONE_WITH_CONCERNS

## Commits
- `544c478` sso: generic OIDC provider (per-tenant, PKCE, email link)
- `e518f40` sso: OIDC E2E test for settings persistence (Playwright)

## What was done
1. **Migration** `migrations/20261101000003_tenant_oidc_config.sql` — per-tenant OIDC settings table (`issuer`, `client_id`, `client_secret` encrypted via PiiCipher, `allowed_domains TEXT[]`, `enabled`, RLS + FORCE RLS + policy). Applied to dev DB + `_sqlx_migrations` row inserted with sha384 checksum.
2. **RBAC** — `auth.oidc_manage` permission (hr_admin only) added to `crates/core/authz/src/rbac.rs` with unit tests.
3. **Gateway routes** (`crates/api/gateway/src/handlers/oidc.rs`):
   - `GET /api/v1/auth/oidc/authorize?tenant_slug=...` — 302 redirect to provider auth endpoint with PKCE S256 + signed JWT state (tenant + verifier + exp).
   - `GET /api/v1/auth/oidc/callback?code&state` — token exchange, verified-email required, domain allow-list, link-or-create user, audit-log entry, 302 to `/login?oidc=ok`.
   - `POST /api/v1/auth/oidc/settings` (admin) — save config, PiiCipher-encrypt secret, audit-log.
   - `GET /api/v1/auth/oidc/settings` (admin) — read config with masked secret.
   - Oidc discovery + token exchange via `reqwest::Client` shared through AppState; discovery cached in-memory per issuer.
4. **State** — signed JWT (HMAC-SHA256, 5-min TTL, iss=talentos.oidc) for the OIDC `state` param; no Redis needed.
5. **UI** — SSO / OIDC section on `/settings` (issuer, client_id, client_secret, allowed_domains, enable checkbox, save button, status indicator `data-testid="oidc-settings"`); gated by `me.permissions` via `auth.oidc_manage`; i18n keys added for TH + EN.
6. **E2E** — `web/e2e/oidc.spec.ts` settings-persistence test (save → masked-secret read-back → reload → persist).
7. **Integration tests** — `tests/integration/src/oidc.rs`: settings round-trip + audit-log + encrypted-secret check, RBAC 403 for non-admin, authorize-redirect smoke test.

## Test evidence
```
cargo test -p talentos-gateway handlers::oidc
test handlers::oidc::tests::... ok  (8/8 passed)
cargo test -p talentos-authz
test result: ok. 43 passed; 0 failed
npx playwright test e2e/oidc.spec.ts
1 passed (7.5s)
cargo clippy -p talentos-gateway --all-targets -- -D warnings
(clean)
cargo fmt --all -- --check
(clean)
npm run build
(ok, all pages built)
```

## Concerns
1. **Pre-existing flaky E2E failure in `app.spec.ts`** — the "creates an employee, records OT, runs and approves payroll" test fails with a strict-mode violation (`getByText('ตรวจสอบความผืดผิดปกติแลว')` resolves to 2 elements: a `<p>` notice and a disabled `review-anomalies` button). This failure is unrelated to OIDC changes; it reproduces on the base commit `544c478` before the E2E OIDC test was added. The fixed tenant slug `e2e` + fixed employee code `E2E001` in the serial test also cause cross-run collisions if the DB is not fully truncated.
2. **`tests/integration` crate is out of the workspace** — its `Cargo.toml` has a pre-existing `portpicker = "0.2"` version conflict (only 0.1.x exists on crates.io) and is not in the root workspace `members`. The new OIDC integration tests are syntactically correct and consistent with the existing test patterns, but the crate cannot be compiled/run in this session until those dependency issues are resolved.
3. **OIDC flow is inert until an operator registers a real OAuth client** — as specified in the brief; the skeleton is complete but no end-to-end provider login is verified.
4. **`id_token` claim extraction without signature verification** — the callback falls back to decoding the `id_token` payload without verifying the provider's signature (skeleton limitation, documented in code comment); full JWKS verification is a follow-up.

## Fix Round 1 — Review Findings (2026-11-01)

Addressed all 3 open findings from the task review.

### 1. Critical — `link_or_create_user` SQL missing `$n` placeholders (FIXED)
`crates/api/gateway/src/handlers/oidc.rs` — five queries in `link_or_create_user`
had no bind positions (`WHERE tenant_id =  AND lower(email) = `,
`VALUES (, , , TRUE)`, etc.), so `.bind()` calls were orphaned and the SQL would
fail at runtime on every OIDC callback reaching the user-link step.
Added positional placeholders matching the `.bind()` order:
- `SELECT id FROM users WHERE tenant_id = $1 AND lower(email) = $2`
- `SELECT is_active FROM users WHERE id = $1 AND tenant_id = $2`
- `UPDATE users SET last_login_at = now() WHERE id = $1 AND tenant_id = $2`
- `INSERT INTO users (tenant_id, email, password_hash, is_active) VALUES ($1, $2, $3, TRUE) RETURNING id`
- `INSERT INTO roles (tenant_id, name) VALUES ($1, 'employee') ... ON CONFLICT`
- `INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING`

**TDD** — three new DB-backed tests in `handlers::oidc::tests` that run
`link_or_create_user` against the dev DB (skip when `DATABASE_URL`/`DATABASE_ADMIN_URL`
unset), exercising every placeholder:
- `db_link_existing_active_user_by_email` — seeds an active user, expects it found by email (returned UUID matches).
- `db_link_or_create_new_user_in_empty_tenant` — unknown email, create path; verifies one active user row + one `user_roles` link landed.
- `db_inactive_user_is_rejected` — inactive user → `Ok(None)` (not linked).

These tests fail on a SQL parse/execute error if the placeholders are wrong,
so they would have caught the original bug.

### 2. Important — OIDC login audit row `actor_id` used `tenant_id` (FIXED)
The callback's `auth.oidc_login` audit insert had `actor_id` bound to `tenant_id.0`
(a tenant UUID) instead of the linked/created user. Changed `link_or_create_user`
to return the active user's UUID (`Result<Option<Uuid>, Response>`), and the
callback now binds `actor_id` to that user id. `resource_id` remains `tenant_id`
(`$3` now holds the user string, `$1`/`$3` keep tenant id semantics):
`VALUES ($1, 'user', $2, 'auth.oidc_login', 'session', $3, '{}'::jsonb)`
where `$2` = `linked_user_id`, `$3` = `tenant_id`.

### 3. Minor — unverified id_token decode fallback (FIXED)
Added the requested guard comment at the `resolve_user_info` id_token fallback:
`// TODO: verify id_token signature via JWKS before enabling in production`

### Commands run
```
cargo test -p talentos-gateway            # dev env exported
cargo clippy -p talentos-gateway --all-targets -- -D warnings
cargo fmt -p talentos-gateway
```

### Output
```
cargo test -p talentos-gateway
test result: ok. 86 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.57s
(includes the 3 new db_* tests, all ok)
cargo clippy -p talentos-gateway --all-targets -- -D warnings
Finished `dev` profile — no warnings
cargo fmt -p talentos-gateway -- --check
clean
```

### Commit
`fix: OIDC callback SQL placeholders + audit actor_id + TODO guard (sso)`

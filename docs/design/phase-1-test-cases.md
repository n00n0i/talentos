# TalentOS Phase 1 — Integration Test Cases

> All tests are **integration-level**: they exercise real HTTP endpoints against a live gateway instance with PostgreSQL + Redis running (via docker-compose). No mocks of the auth layer itself.
>
> Base URL: `http://localhost:8080/api/v1`
>
> Fixtures (seeded before suite): tenant A (`acme-corp`), tenant B (`globex-inc`); users in each with roles per matrix; one inactive user; one user with zero roles.

---

## Test Case Table

| TC-ID | Category | Description | Given | When | Then | Priority |
|-------|----------|-------------|-------|------|------|----------|
| TC-01 | Happy Path | Successful login returns valid token pair | Tenant `acme-corp`, active user `somchai@acme.co.th` with password `Secret123!` and role `hr_admin` | `POST /auth/login` with correct email, password, tenant_slug | `200`; body contains non-empty `access_token` (JWT format), non-empty `refresh_token` (64-char hex), `token_type="Bearer"`, `expires_in=900`; response has `X-Request-Id` header; `users.last_login_at` updated in DB | P0 |
| TC-02 | Happy Path | GET /auth/me returns full profile with permissions | Valid access token from TC-01 (role=`hr_admin`) | `GET /auth/me` with `Authorization: Bearer <token>` | `200`; body contains `user_id`, `email`, `full_name_th`, `roles=["hr_admin"]`, `permissions` array includes `"employee.create"` and `"payroll_run.approve"`; `tenant_id` matches fixture | P0 |
| TC-03 | Happy Path | Refresh rotates tokens correctly | Valid refresh token R1 from prior login | `POST /auth/refresh` with `{"refresh_token":"R1"}` | `200`; new `access_token` ≠ old; new `refresh_token` R2 ≠ R1; in DB: R1's row has `revoked_at IS NOT NULL`; R2's row has `parent_hash = hash(R1)` | P0 |
| TC-04 | Happy Path | Health endpoint reports ok when deps healthy | PG accepting connections, Redis responding | `GET /health` (no auth header) | `200`; `status="ok"`, `checks.database="ok"`, `checks.redis="ok"`, `uptime_seconds > 0` | P1 |
| TC-05 | Token Validation | Expired access token rejected | Manually craft JWT with `exp` = now − 60s (past 30s leeway), signed with valid secret | `GET /auth/me` with expired token | `401`; error code `"token_expired"`; no user data leaked | P0 |
| TC-06 | Token Validation | Malformed token string rejected | Arbitrary garbage string `"not-a-jwt-at-all"` | `GET /auth/me` with `Authorization: Bearer not-a-jwt-at-all` | `401`; error code `"token_malformed"`; no stack trace or internal detail in response | P0 |
| TC-07 | Token Validation | Tampered signature rejected | Take valid JWT, change one char in payload segment (e.g., alter `roles` array), keep original signature | `GET /auth/me` with tampered token | `401`; error code `"token_signature_invalid"` | P0 |
| TC-08 | Token Validation | Token signed with wrong key rejected | Craft JWT with valid structure but sign using different HMAC secret | `GET /auth/me` | `401`; error code `"token_signature_invalid"` | P0 |
| TC-09 | RBAC Enforcement | Employee cannot access admin-only resource | Active user with role=`employee` only | Attempt action mapped to `hr_admin` exclusive (e.g., future `DELETE /employees/:id`) via permission check at handler level | `403`; error code `"insufficient_permission"`; verify via unit-style assertion on `has_permission(["employee"], "employee", "delete") == false` exercised through an endpoint that calls it | P1 |
| TC-10 | RBAC Enforcement | Payroll manager can calculate but not manage settings | User with role=`payroll_manager` | Call permission check for `("payroll_run","calculate")` → allowed; `("settings","manage_company")` → denied | Assert both outcomes through protected endpoints once implemented; Phase 1 validates via `/auth/me` permissions array containing `"payroll_run.calculate"` but NOT `"settings.manage_company"` | P1 |
| TC-11 | RBAC Enforcement | Manager sees team-scoped permissions only | User with role=`manager`, entity_id=E1 | `GET /auth/me` | Permissions include `"leave_request.approve_team"`, `"payslip.view_team_summary"`; do NOT include `"payroll_run.export_report"` or `"audit_log.read"` | P1 |
| TC-12 | Cross-Tenant Isolation | Login with wrong tenant slug fails identically to wrong password | User exists in tenant A; attempt login specifying tenant B slug | `POST /auth/login` with tenant B slug, correct email/password for tenant A | `401`; error message byte-identical to TC-13 (wrong password case); timing difference < 5ms median across 100 runs (timing-safe comparison via dummy Argon2 verify) | P0 |
| TC-13 | Cross-Tenant Isolation | Cannot read another tenant's data despite forged tid claim | Valid JWT for tenant A; attacker modifies `tid` claim to tenant B UUID | Send request with modified token | `401` signature invalid (HMAC catches it). If somehow bypassed: RLS ensures SET LOCAL uses verified claim → queries return empty/error. Dual defense proven. | P0 |
| TC-14 | Cross-Tenant Isolation | Refresh token from tenant A unusable in tenant B context | Obtain refresh token via tenant A login | Call `/auth/refresh` normally → succeeds, issues tenant-A-scoped tokens. Verify issued access token's decoded `tid` = tenant A ID, never tenant B | Confirmed isolation | P1 |
| TC-15 | Brute Force / Rate Limiting | More than 10 login attempts/min from same IP triggers 429 | Fresh rate-limit window for test IP | Send 11 rapid `POST /auth/login` requests (invalid creds) from same IP within 60 seconds | Requests 1–10: `401`. Request 11+: `429` with `Retry-After` header present and numeric; body includes `"rate_limited"` error code | P0 |
| TC-16 | Brute Force / Rate Limiting | Per-email lockout after 5 consecutive failures | Same email targeted from 5 different IPs | After 5 failed logins for same email regardless of source IP | 6th attempt (any IP) returns `429` mentioning retry-after cooldown (~900s); legitimate user must wait or use reset flow (out of scope Phase 1) | P1 |
| TC-17 | Concurrent Sessions | Two devices logged in simultaneously maintain independent sessions | User logs in from Device A (gets RT_A) and Device B (gets RT_B) | Both call `GET /auth/me` with their respective access tokens | Both succeed independently. Revoke RT_A via logout-equivalent (manual DB revoke or family walk); RT_B still functional for subsequent `/auth/refresh` | P1 |
| TC-18 | Concurrent Sessions | Simultaneous refresh with same token triggers reuse detection | Single refresh token R shared between two parallel clients | Fire two concurrent `POST /auth/refresh` with identical R | Exactly one succeeds (`200`, new tokens). Other receives `401`. Post-condition: entire token family (including successor from winner) is revoked — next refresh attempt on successor also yields `401` | P0 |
| TC-19 | Error Responses | Missing Authorization header on protected route | No `Authorization` header sent | `GET /auth/me` | `401`; standardized envelope `{error, message, request_id}`; no leakage of middleware internals, Rust type names, or file paths | P0 |
| TC-20 | Error Responses | Invalid JSON body on login | Syntactically broken JSON `{"email": "` (unterminated) | `POST /auth/login` | `400`; error code `"invalid_request"`; `details` array identifies parse failure; no raw deserializer panic text exposed | P1 |
| TC-21 | Error Responses | Inactive account cannot authenticate | User row exists with `is_active=false` | `POST /auth/login` with correct credentials | `401`; generic `"Invalid email or password"` message — indistinguishable from wrong-password case | P0 |
| TC-22 | Error Responses | Zero-role user cannot authenticate | User exists, active, but no rows in `user_roles` | `POST /auth/login` with correct credentials | `401`; generic message; server-side WARN log emitted (verified by capturing tracing output in test harness) | P1 |
| TC-23 | Database Impact | New refresh_tokens table respects RLS | Tenant A connection with `app.current_tenant_id` set to A | Query `SELECT * FROM refresh_tokens` under tenant A context | Returns only rows where `tenant_id = A`; zero rows belonging to tenant B visible | P0 |
| TC-24 | Token Validation | Clock skew tolerance accepts slightly-future iat/nbf | Generate JWT with `iat`/`nbf` = now + 20s (within 30s leeway) | Use token immediately for `GET /auth/me` | `200` success — leeway applied | P2 |
| TC-25 | Happy Path | OPTIONS preflight returns correct CORS headers | Origin `http://localhost:3000` configured in `AUTH_ALLOWED_ORIGINS` | `OPTIONS /auth/login` with `Origin` + `Access-Control-Request-Method: POST` | `204` or `200`; `Access-Control-Allow-Origin` echoes exact origin; `Allow-Methods` includes POST; `Allow-Headers` includes `Authorization`, `Content-Type` | P1 |

---

## Coverage Summary

| Category | Count | IDs |
|----------|-------|-----|
| Happy Path | 4 | TC-01, TC-02, TC-03, TC-04, TC-25 |
| Token Validation | 4 | TC-05, TC-06, TC-07, TC-08, TC-24 |
| RBAC Enforcement | 3 | TC-09, TC-10, TC-11 |
| Cross-Tenant Isolation | 3 | TC-12, TC-13, TC-14 |
| Brute Force / Rate Limiting | 2 | TC-15, TC-16 |
| Concurrent Sessions | 2 | TC-17, TC-18 |
| Error Responses | 4 | TC-19, TC-20, TC-21, TC-22 |
| Database Impact | 1 | TC-23 |
| **Total** | **25** | |

Every security boundary in `phase-1-spec.md` §6 has at least one valid-input and one invalid-input test paired together (TC-05↔TC-01 expiry vs happy, TC-07↔TC-02 tamper vs clean, TC-12↔TC-01 wrong-tenant vs correct, TC-15↔TC-01 flood vs single, TC-18↔TC-03 race vs sequential rotation, TC-21/TC-22 ↔ TC-01 disabled/no-role vs active-with-role).

---

## Execution Notes

- Run via `cargo nextest run --features integration` or a dedicated `tests/integration/` directory using `reqwest` against a spawned gateway process.
- Each test seeds its own fixture data inside a transaction rolled back afterward, OR uses unique emails per run (`test-{uuid}@acme.co.th`) to avoid cross-test pollution. Preferred: unique-per-run approach since RLS makes cleanup tricky across tenants.
- Docker Compose stack (`docker-compose.yml`) provides PG16, Redis, Kafka, Temporal — integration suite targets this directly; no separate test-infra needed.
- Flaky-prone tests (TC-15, TC-16, TC-18 involving concurrency/timing) should be retried up to 3× in CI before marking failure, and always assert on outcome stability rather than precise millisecond values except where spec explicitly demands timing parity (TC-12).
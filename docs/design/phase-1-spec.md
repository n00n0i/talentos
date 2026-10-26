# TalentOS Phase 1 — Authentication & Authorization Specification

> **Scope**: JWT-based auth, RBAC, multi-tenant RLS injection, health endpoint.
> Every subsequent phase depends on this foundation being correct and complete.

---

## 1. Module Boundaries

### 1.1 `crates/core/authz/`

Pure domain logic. Zero framework dependencies (same rule as `kernel`).

| Responsibility | Type / Function | Notes |
|---|---|---|
| JWT encode | `fn issue_access_token(ctx: &TenantContext, secret: &[u8], ttl: Duration) -> Result<String, AuthError>` | HS256; claims per §2 |
| JWT decode + validate | `fn verify_access_token(token: &str, secret: &[u8]) -> Result<TenantContext, AuthError>` | Checks `exp`, `iss`, `aud`; returns `TenantContext` from kernel |
| Refresh token generation | `fn issue_refresh_token(user_id: UserId, tenant_id: TenantId, secret: &[u8], ttl: Duration) -> Result<String, AuthError>` | Opaque random 256-bit hex, NOT a JWT — stored hashed in DB |
| Password hashing | `fn hash_password(plain: &str) -> Result<String, AuthError>` | Argon2id, params: m=64MB, t=3, p=4 |
| Password verification | `fn verify_password(plain: &str, hash: &str) -> Result<bool, AuthError>` | Constant-time compare via argon2 crate |
| Permission check | `fn has_permission(roles: &[String], resource: &str, action: &str) -> bool` | Looks up static RBAC matrix (§3) |
| Error enum | `enum AuthError { InvalidCredentials, TokenExpired, TokenMalformed, TokenSignatureInvalid, InsufficientPermission, AccountDisabled, RateLimited }` | Maps to HTTP status codes (§4) |

**Public API surface** (`lib.rs`):

```rust
pub mod jwt;       // issue_*, verify_*
pub mod password;  // hash_*, verify_*
pub mod rbac;      // has_permission, Role, Resource, Action enums
pub mod error;     // AuthError

pub use error::AuthError;
pub use rbac::{has_permission, Role};
```

Depends on: `talentos-kernel` (for `TenantId`, `UserId`, `EntityId`, `TenantContext`).

### 1.2 `crates/infra/db/`

Wraps `sqlx::PgPool`. Single responsibility: guarantee every checked-out connection carries the correct tenant session variable before any query runs.

```rust
pub struct TenantPool {
    inner: sqlx::PgPool,
}

impl TenantPool {
    /// Build pool from DATABASE_URL env var. max_connections default = 20.
    pub async fn connect(url: &str, max_conns: u32) -> Result<Self, sqlx::Error>;

    /// Acquire a connection and SET LOCAL app.current_tenant_id.
    /// The SET LOCAL is scoped to the current transaction — caller MUST use
    /// within a tx or the setting leaks to the next borrower.
    pub async fn acquire_with_tenant(
        &self,
        ctx: &TenantContext,
    ) -> Result<PooledConnection<'_, Postgres>, sqlx::Error>;

    /// Convenience: run a closure inside a tenant-scoped transaction.
    pub async fn with_tenant_tx<F, T>(&self, ctx: &TenantContext, f: F)
        -> Result<T, AuthError>
    where
        F: FnOnce(&mut Transaction<'_, Postgres>) -> BoxFuture<Result<T, AuthError>>;
}
```

**Critical invariant**: `with_tenant_tx` executes `SET LOCAL app.current_tenant_id = $1` as the *first* statement inside the transaction, binding `ctx.tenant_id.to_string()`. Because it's `SET LOCAL`, the GUC reverts at COMMIT/ROLLBACK — no cross-request leakage even if the pool recycles connections.

If no tenant context is available (login endpoint), use `acquire_raw()` which does NOT set the GUC. The login query must look up user by `(tenant_slug_or_id, email)` explicitly — never rely on RLS for the bootstrap lookup. Use a dedicated `BYPASSRLS`-granted DB role (`talentos_auth`) for this single query path only. Document clearly: **only the login/email-lookup code path may use `acquire_raw()`**.

### 1.3 `crates/api/gateway/`

Axum binary crate. Owns routing, middleware, request/response serialization.

```
src/
  main.rs          — tokio::main, load config, build router, serve
  router.rs        — Router<AppState> construction
  state.rs         — AppState { pool: TenantPool, jwt_secret: Arc<[u8]>, rate_limiter }
  middleware/
    mod.rs
    logging.rs     — tower_http TraceLayer wrapper, structured JSON spans
    cors.rs        — tower_http CorsLayer config
    auth.rs        — extract Bearer token → verify → inject TenantContext into request extensions
    tenant.rs      — read TenantContext from extensions → attach to DB calls (no-op here, db layer does the SET LOCAL)
  handlers/
    auth.rs        — POST /auth/login, POST /auth/refresh, GET /auth/me
    health.rs      — GET /health
  dto/
    requests.rs    — LoginRequest, RefreshRequest
    responses.rs   — TokenResponse, MeResponse, ErrorResponse, HealthResponse
```

`AppState` is cheaply cloneable (`Arc` internals) and passed to all handlers via Axum's `FromRef`.

---

## 2. JWT Claims Schema

### Access Token (JWT, HS256)

| Claim | Type | Value / Source | Required |
|---|---|---|---|
| `sub` | `"string"` | `UserId` UUID v7 string | ✅ |
| `tid` | `"string"` | `TenantId` UUID v7 string | ✅ |
| `eid` | `null \| "string"` | `EntityId` UUID string; `null` = cross-entity admin | ✅ |
| `roles` | `["string"]` | Array of role names from `user_roles` joined with `roles.name` | ✅ |
| `iat` | `"integer"` (NumericDate) | Unix seconds at issuance | ✅ |
| `exp` | `"integer"` (NumericDate) | `iat + ACCESS_TTL_SECONDS` | ✅ |
| `nbf` | `"integer"` (NumericDate) | Same as `iat` | ✅ |
| `iss` | `"string"` | `"talentos.auth"` — constant | ✅ |
| `aud` | `"string"` | `"talentos.api"` — constant | ✅ |
| `jti` | `"string"` | Random UUID v4, unique per token; used for revocation blacklist | ✅ |

**TTL**: `ACCESS_TTL_SECONDS` = **900** (15 minutes). Configurable via env var `AUTH_ACCESS_TTL_SECS`.

### Refresh Token (opaque, NOT JWT)

A refresh token is a **random 256-bit hex string** (64 chars). It is returned to the client alongside the access token but is never parsed locally. Server stores only its SHA-256 hash.

Stored in new table `refresh_tokens` (§7):

| Column | Purpose |
|---|---|
| `token_hash` | SHA-256 of the opaque value |
| `user_id` | FK → users.id |
| `tenant_id` | FK → tenants.id |
| `expires_at` | `now() + REFRESH_TTL_DAYS` days |
| `revoked_at` | nullable; set on rotation or logout |
| `created_at` | audit timestamp |
| `parent_hash` | nullable; links rotated predecessor for reuse-detection chain |

**TTL**: `REFRESH_TTL_DAYS` = **7** days. Configurable via env var `AUTH_REFRESH_TTL_DAYS`.

**Rotation policy**: On successful `/auth/refresh`, the presented refresh token is revoked (`revoked_at = now()`), a new one is issued, and `parent_hash` on the new row points to the old `token_hash`. If a *revoked* token is presented again → treat as theft: revoke the entire family (walk `parent_hash` chain upward) and return 401.

---

## 3. RBAC Permission Matrix

Roles are defined statically in code (not dynamic DB rows for Phase 1). The `roles.permissions` JSONB column exists in schema for future customization but Phase 1 ignores it — use the hardcoded matrix below.

Format: `resource.action` → allowed roles.

| Resource | Action | hr_admin | payroll_manager | manager | employee |
|---|---|---|---|---|---|
| `employee` | `create` | ✅ | ❌ |  | ❌ |
| `employee` | `read_own_team` | ✅ | ✅ | ✅ | ❌ |
| `employee` | `read_all` | ✅ | ✅ | ❌ | ❌ |
| `employee` | `update` | ✅ | ❌ | ❌ | ❌ |
| `employee` | `delete` | ✅ | ❌ | ❌ | ❌ |
| `employee_profile` | `update_self` | ✅ | ✅ | ✅ | ✅ |
| `payslip` | `view_self` | ✅ | ✅ | ✅ | ✅ |
| `payslip` | `view_team_summary` | ✅ | ✅ | ✅ | ❌ |
| `payslip` | `view_all` | ✅ | ✅ | ❌ | ❌ |
| `payroll_run` | `calculate` | ✅ | ✅ | ❌ | ❌ |
| `payroll_run` | `approve` | ✅ | ✅ | ❌ | ❌ |
| `payroll_run` | `export_report` | ✅ | ✅ | ❌ | ❌ |
| `leave_request` | `submit_self` | ✅ | ✅ | ✅ | ✅ |
| `leave_request` | `approve_team` | ✅ | ❌ | ✅ | ❌ |
| `leave_request` | `view_team` | ✅ | ❌ | ✅ | ❌ |
| `settings` | `manage_company` | ✅ | ❌ | ❌ | ❌ |
| `settings` | `view_company` | ✅ | ✅ | ✅ | ❌ |
| `user_management` | `invite` | ✅ | ❌ | ❌ | ❌ |
| `user_management` | `assign_role` | ✅ | ❌ | ❌ | ❌ |
| `audit_log` | `read` | ✅ | ❌ | ❌ | ❌ |

**Implementation**: Static `HashMap<(&str, &str), &[&str]>` built once at process start in `rbac.rs`. Lookup is O(1).

```rust
// crates/core/authz/src/rbac.rs
pub fn has_permission(roles: &[String], resource: &str, action: &str) -> bool {
    let key = (resource, action);
    MATRIX.get(&key).map_or(false, |allowed| {
        roles.iter().any(|r| allowed.contains(&r.as_str()))
    })
}
```

**Manager scope note**: `manager` role's team visibility is enforced at the query level (JOIN through org hierarchy or `entity_id` match), not through RBAC alone. RBAC answers "can this role ever do X?" — scoping ("which records?") is the handler's job using `TenantContext.entity_id`.

---

## 4. API Endpoint Contracts

All endpoints under base URL `/api/v1`. All request/response bodies are `application/json; charset=utf-8`.

Common headers on every response:
- `X-Request-Id`: UUID generated by gateway, echoed for tracing
- `Cache-Control: no-store` on all auth endpoints

### 4.1 `POST /auth/login`

**Authentication**: None (public endpoint). Rate-limited (§6).

**Request body**:
```json
{
  "email": "somchai@example.co.th",
  "password": "...",
  "tenant_slug": "acme-corp"
}
```

| Field | Type | Constraints |
|---|---|---|
| `email` | string | RFC 5322 valid, max 255 chars |
| `password` | string | min 8, max 128 chars |
| `tenant_slug` | string | matches `[a-z0-9-]{2,63}` |

**Success response** — `200 OK`:
```json
{
  "access_token": "<jwt>",
  "refresh_token": "<opaque-hex-64>",
  "token_type": "Bearer",
  "expires_in": 900
}
```

**Error responses**:

| Status | Condition | Body |
|---|---|---|
| `400` | Malformed JSON / missing fields / validation failure | `{"error":"invalid_request","message":"...","request_id":"..."}` |
| `401` | Wrong credentials OR inactive account OR unknown tenant slug | `{"error":"unauthorized","message":"Invalid email or password","request_id":"..."}` |
| `429` | Rate limit exceeded | `{"error":"rate_limited","message":"Too many attempts. Retry after Ns","retry_after_seconds":N,"request_id":"..."}` + `Retry-After` header |

⚠️ **Security**: 401 message must be identical whether the email doesn't exist, the tenant doesn't exist, or the password is wrong. Never reveal which factor failed. Timing equalization: always run Argon2 verify against a dummy hash when user/tenant not found, so response time is indistinguishable.

Side effects on success: update `users.last_login_at = now()`, insert `audit_log` entry with `action='auth.login'`.

### 4.2 `POST /auth/refresh`

**Authentication**: None (uses refresh token in body). Rate-limited.

**Request body**:
```json
{
  "refresh_token": "<opaque-hex-64>"
}
```

**Success response** — `200 OK`:
```json
{
  "access_token": "<new-jwt>",
  "refresh_token": "<new-opaque-hex-64>",
  "token_type": "Bearer",
  "expires_in": 900
}
```

Old refresh token is atomically revoked; new pair issued (rotation).

**Error responses**:

| Status | Condition |
|---|---|
| `400` | Missing/malformed `refresh_token` |
| `401` | Token not found, expired, already revoked (reuse detected → family invalidated), or user deactivated |

Reuse detection: if the presented token has `revoked_at IS NOT NULL`, walk the `parent_hash` chain and revoke ALL tokens in the family. Return 401. Log security event to `audit_log` with `action='auth.token_reuse_detected'`.

### 4.3 `GET /auth/me`

**Authentication**: `Authorization: Bearer <access_token>` required.

**Success response** — `200 OK`:
```json
{
  "user_id": "0194...",
  "email": "somchai@example.co.th",
  "full_name_th": "สมชาย ใจดี",
  "full_name_en": "Somchai Jaidee",
  "tenant_id": "0194...",
  "entity_id": "0194...",
  "roles": ["hr_admin"],
  "permissions": ["employee.create", "employee.update", "..."],
  "last_login_at": "2026-10-25T08:30:00Z"
}
```

`permissions` array is computed server-side by expanding the user's roles through the RBAC matrix — gives frontend a flat list for UI gating without reimplementing the matrix.

**Error responses**:

| Status | Condition |
|---|---|
| `401` | Missing header, invalid/expired/tampered token |

No `403` possible here — if the token is valid, the user can always see their own profile.

### 4.4 `GET /health`

**Authentication**: None. Excluded from rate limiting. Intended for k8s/LB probes.

**Success response** — `200 OK`:
```json
{
  "status": "ok",
  "version": "0.1.0",
  "checks": {
    "database": "ok",
    "redis": "ok"
  },
  "uptime_seconds": 3600
}
```

Database check: `SELECT 1` with 2-second timeout. Redis check: `PING`. If either fails → `503 Service Unavailable` with same shape but failing component marked `"error"`. Never expose connection strings or internal IPs in health output.

### 4.5 Error Response Envelope (all endpoints)

```json
{
  "error": "machine_readable_code",
  "message": "Human-readable, safe-to-display text",
  "details": [],
  "request_id": "uuid"
}
```

`details` is an optional array present only on `400` validation errors, containing field-level messages:
```json
{"field": "email", "message": "must be a valid email address"}
```

Never include stack traces, SQL fragments, internal paths, or dependency versions in any error response.

---

## 5. Middleware Stack Order

Applied outermost → innermost. Each wraps the next.

```
┌─────────────────────────────────┐
│ 1. Request Logging (TraceLayer) │  ← tower_http::trace::TraceLayer
│    Captures method, path,       │     span per request; latency recorded on drop
│    status, latency, request_id  │
├─────────────────────────────────┤
│ 2. CORS                         │  ← tower_http::cors::CorsLayer
│    Allow origins from ENV       │     AUTH_ALLOWED_ORIGINS (comma-separated)
│    Methods: GET, POST, OPTIONS  │     Headers: Authorization, Content-Type, X-Request-Id
│                                 │     Max-Age: 86400
├─────────────────────────────────┤
│ 3. Rate Limiter                 │  ← custom tower Layer backed by Redis
│    Key: IP + route              │     Sliding window: 10 req/min/IP on /auth/*
│    Applies only to /auth/*      │     Burst: 5 concurrent
├─────────────────────────────────┤
│ 4. Auth Extraction              │  ← custom Axum middleware / extractor
│    Parse Authorization header   │     Skipped for /health, /auth/login, /auth/refresh
│    Verify JWT → TenantContext   │     On failure: short-circuit 401
│    Insert into RequestExt       │
├─────────────────────────────────┤
│ 5. Route Handler                │  ← axum handler fn(AppState, ...)
│    Uses TenantContext from ext  │     Calls pool.with_tenant_tx(ctx, ...) for DB ops
└─────────────────────────────────┘
```

**Note**: Steps 4→5 boundary — `TenantContext` lives in `http::Extensions`. Handlers retrieve it via a typed extractor:

```rust
pub struct Authed(pub TenantContext);

#[axum::async_trait]
impl<S: Send + Sync> FromRequestParts<S> for Authed {
    type Rejection = AuthError;
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<TenantContext>()
            .cloned()
            .ok_or(AuthError::TokenMalformed)
    }
}
```

This makes it impossible to forget auth on a protected route — if the handler takes `Authed`, the middleware must have populated it.

---

## 6. Edge Cases & Mitigations

### 6.1 Expired Access Token
- `verify_access_token` checks `exp` claim with ±30s clock skew tolerance (`leeway`).
- Returns `AuthError::TokenExpired` → HTTP 401. Client should call `/auth/refresh`.
- Gateway does NOT auto-refresh; separation of concerns stays with client.

### 6.2 Wrong Tenant Claim
- After JWT verification, handler receives `TenantContext` whose `tenant_id` came from the signed `tid` claim. Since the token is HMAC-signed, tampering with `tid` invalidates the signature → caught in verification step. Cannot produce "wrong tenant" from a validly-signed token unless the signing key is compromised.
- Defense-in-depth: DB layer sets `app.current_tenant_id` from the verified claim. Even if application logic has a bug passing wrong context, RLS still constrains rows to whatever was SET LOCAL'd.

### 6.3 Missing Role
- User with zero rows in `user_roles` → `roles: []` in `TenantContext`.
- `has_permission([], ...)` returns `false` for everything except nothing. Effectively read-only self-service denied too.
- Decision: reject login if user has no roles assigned. Return 401 with generic message. Log warning internally. A user must have at least `employee` role to authenticate.

### 6.4 Concurrent Sessions
- Multiple devices/sessions share the same `users` row but get independent refresh tokens (each `/auth/login` creates a new `refresh_tokens` row).
- No session-count limit in Phase 1 (YAGNI). Documented limitation: `shortcut: unlimited concurrent sessions, add max_sessions_per_user config in Phase 2 if abuse observed`.
- Logout on one device revokes only that device's refresh token family.

### 6.5 Brute-Force Protection
- Redis-backed sliding-window counter keyed by `{ip}:{route}`.
- `/auth/login`: 10 attempts per minute per IP. On breach → 429 with `Retry-After`.
- Additionally track per-email failures: after 5 consecutive failures on the same email across any IP, require a CAPTCHA token (Phase 1 stub: just lock the email for 15 minutes, return 429). Store counter in Redis with TTL.
- Rate limiter must fail-open: if Redis is unreachable, allow requests through but log error loudly. Availability > perfect throttling.

### 6.6 Token Rotation on Refresh
- Covered in §2. Atomic operation: within a single DB transaction, INSERT new token + UPDATE old token SET revoked_at. If either fails, neither commits.
- Race condition guard: two simultaneous refresh calls with the same token. First succeeds, second finds `revoked_at IS NOT NULL` → triggers family revocation (treat as potential replay attack). Acceptable behavior: legitimate double-click loses one session, forces re-login. Safer than allowing parallel validity.

### 6.7 Clock Skew Between Pods
- JWT `exp` validation uses ±30s leeway. Sufficient for NTP-synced infrastructure.
- Refresh token expiry uses DB `now()` (server-authoritative), immune to client/pod clock drift entirely.

### 6.8 Signing Key Management
- `AUTH_JWT_SECRET` loaded from environment at startup. Minimum entropy: 32 bytes. Refuse to boot if shorter or unset.
- Phase 1: single symmetric key, no rotation mechanism. `shortcut: single static HS256 key, migrate to RS256 with JWKS endpoint in Phase 2 for multi-service token verification`.

---

## 7. Database Impact

Existing migrations cover `tenants`, `entities`, `users`, `roles`, `user_roles`, `employees`, `audit_log` with RLS. Phase 1 requires **one additional migration**:

### New migration: `20261025000003_refresh_tokens.sql`

```sql
CREATE TABLE refresh_tokens (
    id BIGSERIAL PRIMARY KEY,
    token_hash BYTEA NOT NULL UNIQUE,          -- SHA-256 digest, 32 bytes
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    parent_hash BYTEA REFERENCES refresh_tokens(token_hash),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ip_address INET,                            -- originating IP for forensics
    user_agent TEXT                             -- truncated, max 512 chars
);

CREATE INDEX idx_refresh_tokens_hash ON refresh_tokens(token_hash);
CREATE INDEX idx_refresh_tokens_user ON refresh_tokens(user_id) WHERE revoked_at IS NULL;
CREATE INDEX idx_refresh_tokens_expiry ON refresh_tokens(expires_at) WHERE revoked_at IS NULL;

ALTER TABLE refresh_tokens ENABLE ROW LEVEL SECURITY;
ALTER TABLE refresh_tokens FORCE ROW LEVEL SECURITY;

CREATE POLICY refresh_token_isolation ON refresh_tokens
    USING (tenant_id = current_tenant_id());
```

**Note**: The `/auth/refresh` endpoint looks up the token by `token_hash` globally (before knowing the tenant), so this lookup must also go through the `BYPASSRLS` auth role like login. Once resolved, the embedded `tenant_id` establishes context for issuing the new access token.

No changes needed to existing tables. `users.password_hash` already stores Argon2id output. `roles.permissions` JSONB column exists but is unused in Phase 1 (static matrix instead).

Cleanup job (Temporal workflow, out of Phase 1 scope but noted): periodically delete `refresh_tokens WHERE expires_at < now() - INTERVAL '30 days' OR revoked_at < now() - INTERVAL '7 days'`.

---

## 8. New Dependencies

Add to `[workspace.dependencies]` in root `Cargo.toml`:

```toml
# JWT
jsonwebtoken = "9"

# Password hashing
argon2 = "0.5"

# Cryptographic randomness (for refresh tokens, jti)
rand = "0.8"

# SHA-256 (refresh token hashing)
sha2 = "0.10"

# Hex encoding (refresh token representation)
hex = "0.4"

# Web framework
axum = { version = "0.7", features = ["macros"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "cors", "request-id", "timeout"] }

# Async traits helper (for with_tenant_tx boxed futures)
futures-util = "0.3"

# Redis client (rate limiting, health check)
fred = { version = "10", features = ["i-keys"] }

# HTTP client for testing
reqwest = { version = "0.12", features = ["json"] }
```

Per-crate additions:

| Crate | Depends on |
|---|---|
| `crates/core/authz` | `jsonwebtoken`, `argon2`, `rand`, `sha2`, `hex`, `talentos-kernel`, `serde`, `thiserror`, `chrono` |
| `crates/infra/db` | `sqlx`, `talentos-kernel`, `tokio`, `futures-util`, `tracing` |
| `crates/api/gateway` | `axum`, `tower`, `tower-http`, `fred`, `serde`, `serde_json`, `talentos-authz`, `talentos-db`, `talentos-kernel`, `tracing`, `tracing-subscriber`, `dotenvy`, `chrono`, `uuid`, `rand` |

Register new members in workspace `Cargo.toml`:

```toml
members = [
    "crates/core/kernel",
    "crates/core/authz",
    "crates/infra/db",
    "crates/api/gateway",
]
```

---

## Appendix A: Environment Variables

| Variable | Default | Description |
|---|---|---|
| `DATABASE_URL` | *(required)* | PostgreSQL connection string |
| `REDIS_URL` | *(required)* | Redis connection string for rate limiter + health check |
| `AUTH_JWT_SECRET` | *(required, ≥32 bytes)* | HS256 signing key |
| `AUTH_ACCESS_TTL_SECS` | `900` | Access token lifetime |
| `AUTH_REFRESH_TTL_DAYS` | `7` | Refresh token lifetime |
| `AUTH_ALLOWED_ORIGINS` | `http://localhost:3000` | Comma-separated CORS origins |
| `GATEWAY_PORT` | `8080` | Listen port |
| `RUST_LOG` | `info` | Tracing filter directive |
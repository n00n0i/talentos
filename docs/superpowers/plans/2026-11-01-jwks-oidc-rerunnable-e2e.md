# Plan: OIDC id_token JWKS verification + re-runnable E2E suite

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the two deferred items from the handoff-execution review: (1) verify the OIDC `id_token` signature via JWKS instead of unverified claim decode; (2) make the Playwright E2E suite re-runnable without a clean DB.

**Architecture:** The gateway already has `jsonwebtoken` 9 (ring) and a discovery-document cache in `crates/api/gateway/src/handlers/oidc.rs`. Task 1 adds `jwks_uri` to the discovery struct, a JWKS fetch + kid-matching RSA verification for `id_token` (RS256), and replaces the unverified base64 decode with the verified path; the mock provider test gains a JWKS endpoint and a signed `id_token`. Task 2 makes `first-run setup` tolerate the already-initialized case and `creates an employee` skip creation when E2E001 already exists.

**Tech Stack:** Rust / Axum / jsonwebtoken 9 (ring backend, no new deps) / Playwright.

**Spec:** `docs/superpowers/plans/2026-10-31-handoff-execution.md` Task 5 + review rulings R6 (id_token JWKS) and the BUILD_STATUS note on the non-idempotent E2E case.

## Global Constraints

- No `unwrap()`/`expect()` in production code (tests allowed).
- RLS: every tenant-scoped query carries `tenant_id = $1`; OIDC handlers already follow this — do not break it.
- Money: N/A for these tasks.
- Significant actions write `audit_log` (OIDC login already does — do not break).
- No new migration.
- Next proxy: no new gateway HTTP methods → no change.
- Quality gates: `cargo fmt --all -- --check`, `cargo clippy -p talentos-gateway --all-targets -- -D warnings`, `cargo test -p talentos-gateway`, `npm run build` (web).

## Review Focus

1. id_token with alg=HS256 or an unknown `kid` → must be rejected, not accepted (spoofed claims).
2. id_token `iss` not matching the configured issuer, or `aud` missing the client_id → rejected.
3. Expired / not-yet-valid id_token (`exp`/`nbf` out of window, ±10 s leeway) → rejected.
4. JWKS fetch fails (provider down) → 503, no unverified fallback path remains.
5. No `jwks_uri` in the discovery document but an id_token is the only identity source → 503 (do not silently accept unverified claims).

---

### Task 1: Verify id_token via JWKS

**Files:**
- Modify: `crates/api/gateway/src/handlers/oidc.rs`
  - `OidcDiscovery` (~line 40): add `jwks_uri: Option<String>`
  - `resolve_user_info` (~line 631): replace the unverified base64 decode (lines 664-688) with `verify_id_token(http, discovery, token)`
  - New `async fn verify_id_token(http: &reqwest::Client, discovery: &OidcDiscovery, id_token: &str) -> Result<serde_json::Value, Response>`: fetch `jwks_uri` (cache it like discovery, or per-request — per-request is fine), select the `JWK` whose `kid` matches the id_token header (if no `kid`, and exactly one key, use it), build `jsonwebtoken::DecodingKey::from_rsa_components(n, e)` per key, `decode` with `Validation` for RS256, `set_issuer([discovery.issuer])` — NOTE: `OidcDiscovery` currently has no `issuer` field; add `issuer: String` and populate it in `discovery()` (the function already knows the issuer). Validate `aud` contains the tenant config's client_id — signature: `verify_id_token(http, discovery, cfg_client_id, id_token)`; on any failure return `service_unavailable` with a distinct message.
  - Remove the `// TODO: verify id_token signature via JWKS` comment (line 667).
  - `OidcUserInfo` extraction from verified claims stays the same shape (email, email_verified).
- Test (in `mod tests`, `crates/api/gateway/src/handlers/oidc.rs`):
  - Extend `mock_provider()`: serve a JWKS at `/jwks` (one RS256 key with `n`, `e`, `kid="k1"` — generate a static test RSA keypair in the test with `jsonwebtoken`/`ring`? Use a hardcoded 2048-bit RSA n/e as decimal-string constants — acceptable for a test), and in a second mock variant serve a **signed** `id_token` (RS256, `iss` = base, `aud` = "client-e2e", `sub`, `email`, `email_verified:true`, `kid="k1"`) when the provider has no userinfo endpoint.
  - New tests:
    - `id_token_verified_via_jwks` — provider without `userinfo_endpoint`, token exchange returns the signed id_token → `resolve_user_info` returns the email (proves the happy path through JWKS).
    - `id_token_rejected_when_signature_mismatch` — token signed with a *different* key than the JWKS → `Err`.
    - `id_token_rejected_when_aud_missing` — token without the client_id in `aud` → `Err`.
    - `id_token_rejected_when_expired` — `exp` in the past → `Err`.
    - `resolve_user_info_503_when_no_jwks_and_no_userinfo` — discovery without `jwks_uri`/`userinfo_endpoint` → `Err` (503), never the unverified path.
  - Update existing tests that build `OidcDiscovery` literals (e.g. `discovery_cache_hit_avoids_network` ~line 962) to include the new `issuer` field.

- [ ] **Step 1: TDD RED — write the 5 new tests + field additions to test fixtures, run, watch fail**

Run: `cargo test -p talentos-gateway --lib handlers::oidc` (with dev env exported; DB tests in the same module still run)
Expected: new tests FAIL (function/fields don't exist yet — compile error is the red).

- [ ] **Step 2: Implement `jwks_uri` + `issuer` on `OidcDiscovery`, JWKS fetch, `verify_id_token`, and the `resolve_user_info` replacement**

One design note: JWKS fetch is per-request (no cache) — one extra HTTPS call per OIDC callback is negligible; skip a cache unless `try` shows a problem. `Validation` leeway: `set_leeway(10)`.

- [ ] **Step 3: Run tests, watch GREEN**

Run: `cargo test -p talentos-gateway --lib handlers::oidc`
Expected: all oidc tests pass (existing 8 + new 5, DB-backed ones still ok).

- [ ] **Step 4: Full gateway suite + gates**

Run: `cargo test -p talentos-gateway` then `cargo fmt --all -- --check` and `cargo clippy -p talentos-gateway --all-targets -- -D warnings`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/api/gateway/src/handlers/oidc.rs
git commit -m "sso: verify OIDC id_token signature via JWKS (RS256, iss/aud/exp)"
```

---

### Task 2: Re-runnable E2E suite (no clean-DB requirement for repeat runs)

**Files:**
- Modify: `web/e2e/app.spec.ts`
  - `first-run setup` test (line 43): after `setup-submit`, accept EITHER the login heading ("เข้่าสู่ระบบ" redirect on success) OR an "already initialized / slug unavailable" error state — in the latter case skip the expectation and go straight to `login(page)`. (The setup endpoint returns 409 "this installation is already initialized" when a tenant already exists; the `/setup` page shows the gateway error message via `handleApiError` — the `/setup` page shows the gateway error message via `<p className="error">` (web/app/setup/page.tsx:89) — add `data-testid="setup-error"` to that element, then match it in the test.)
  - `creates an employee` test (line 110): before opening the add form, `GET api/gw/api/v1/employees?q=E2E001`; if an **active** E2E001 is found, skip the create-form block (the "เพิ่่่่มพนั่กงานแลว้้" notice + row assertion) and continue — the rest of the test (shift/OT import → attendance approval → new payroll run → anomaly review → approve → bank CSV → mark-paid → payslip) operates on the existing employee and a new run, so it stays meaningful on repeat runs. (Do NOT soft-delete: terminated employees fall out of the payroll run and later steps break. NOTE: `DELETE /api/v1/employees/:id` is `terminate`, i.e. soft-delete.)
- Test: `web/e2e/app.spec.ts` (the two tests above ARE the tests).

- [ ] **Step 1: Make both tests, run the suite twice back-to-back (without DB cleanup between runs), watch the second run fail before the fix / pass after**

Run (first pass, to confirm the defect): reset tenants, run `npx playwright test app.spec.ts` twice in a row; the second run fails at first-run setup and/or employee creation.
Expected: second run FAILS (pre-fix evidence).

- [ ] **Step 2: Implement the two test changes**

- [ ] **Step 3: Run the suite twice back-to-back (no DB cleanup), then once from a clean DB**

Run: `npx playwright test app.spec.ts` twice, then `docker exec loans-postgres-1 psql -U talentos -d talentos -c "DELETE FROM tenants;"` + `npx playwright test app.spec.ts` a third time.
Expected: all three runs pass (serial suite, 56/56 across the three files on the clean-DB run; on the repeat runs the whole `web/e2e` suite — run `npx playwright test` for the full 56 on repeat-DB to catch state accumulation in other tests; if other tests also break on repeat-DB because of their own hardcoded codes, note it in the report as a limitation rather than rewriting the whole suite — the two tests in this plan are the scope).

- [ ] **Step 4: Gates + commit**

Run: `npm run build` in `web/`
Then: `git add web/e2e/app.spec.ts web/app/setup/page.tsx docs/BUILD_STATUS.md && git commit -m "e2e: make suite re-runnable without clean DB (idempotent setup + employee reset)"`
Also update the BUILD_STATUS.md note: replace the "pre-existing non-idempotent design" paragraph with "suite is re-runnable; clean-DB first-run still covered".

---

### Task 3: Final verification + docs

**Files:**
- Modify: `docs/BUILD_STATUS.md` (OIDC line: "id_token verified via JWKS" replacing the skeleton-verification caveat if present; test counts if they moved)

- [ ] **Step 1: Full gates**

Run: `cargo test --workspace` (dev env), `cargo clippy -p talentos-gateway --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `npm run build`, full `npx playwright test` (clean DB)
Expected: green (498+ Rust tests — new oidc tests raise the count; record the new number).

- [ ] **Step 2: Update BUILD_STATUS.md test counts + commit**

```bash
git add docs/BUILD_STATUS.md
git commit -m "docs: BUILD_STATUS — OIDC JWKS verification + re-runnable E2E"
```

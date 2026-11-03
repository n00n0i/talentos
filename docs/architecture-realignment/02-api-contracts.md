# 02 — API Contracts: Control Plane and Data Plane (WS1)

**Campaign:** TALENTOS-ARF-V1 · **Date:** 2026-11-03 · **Baseline HEAD:** `8709baa`

> **STATUS: entirely PROPOSED. No endpoint in this document is implemented.**
> Proof: `grep -rn "integration" --include=*.rs crates/` returns 5 hits, all
> comments about *integration tests* — no route and no handler. The only
> integration-adjacent routes that exist are `/api/v1/mcp` (MCP server, not a
> connector) and `/api/v1/plugins` (WASM, not a flow) — §6.

---

## 1. Conventions (VERIFIED_EXISTING — match these, do not invent a second style)

| Convention | Value | Evidence |
|---|---|---|
| Base path | `/api/v1` | `crates/api/gateway/src/router.rs` (186 routes) |
| Auth | `Authorization: Bearer <JWT>` | `crates/api/gateway/src/middleware/auth.rs` |
| Tenant | From the JWT only, never path/query/body | `middleware/auth.rs` → `TenantContext` |
| Correlation | `x-request-id`, echoed | `router.rs` `SetRequestIdLayer` + CORS allowlist |
| Errors | `ErrorResponse` | `crates/api/gateway/src/dto/responses.rs` |
| Pagination | `?limit=&offset=`, default 50, clamp 1..200 | `handlers/audit.rs` |
| RBAC | `require(ctx, resource, action)` → 403 | `middleware/auth.rs`; `crates/core/authz/src/rbac.rs` |

Error body is `ErrorResponse` from `crates/api/gateway/src/dto/responses.rs`:
`{ error, message, details[{field,message}], request_id, retry_after_seconds? }`,
with `details` and `retry_after_seconds` omitted when empty. Pagination envelope:
`{ "items": [], "total": 128, "limit": 50, "offset": 0, "next_offset": 50 }`.

**New headers (PROPOSED):** `Idempotency-Key` is required on every
creating/mutating `POST` (ADR-008), 24h window, same key + different body →
`409 idempotency_conflict`. `If-Match` guards metadata updates, `ETag` = `version`,
stale → `409 version_conflict`. `202` execution responses carry `x-talentos-run-id`.

**Prerequisite:** `router.rs` CORS `allow_headers` lists only `AUTHORIZATION`,
`CONTENT_TYPE`, `x-request-id`; the two new headers must be added or browser calls
fail preflight. One line — **not** changed in this WS.

**Scopes (PROPOSED — `rbac.rs` has no integration resource today):**
`integration.connector.manage`, `integration.connection.manage`,
`integration.flow.author`, `integration.flow.publish`,
`integration.execution.read`, `integration.execution.control`,
`integration.audit.read` — all `hr_admin`-only in v1, with author and publish split
so a flow author cannot push to production alone. Resources:
`connector → connection`; `flow → flow_version`; `mapping → mapping_version`;
`execution → attempt`.

---

## 2. Control Plane API — PROPOSED

All paths below are prefixed `/api/v1`.

### 2.1 Connectors

| Method | Path | Scope | Req → Resp | Errors |
|---|---|---|---|---|
| GET | `/integration/connectors` | connector.manage | — → `{items:[manifest]}` | 401 403 |
| GET | `/integration/connectors/:id` | connector.manage | — → manifest | 404 |
| POST | `/integration/connectors` | connector.manage | `{manifest}` → 201 `{id,version,state}` | 422 409 |
| POST | `/integration/connectors/:id/versions` | connector.manage | `{manifest}` → 201 | 422 409 |
| POST | `/integration/connectors/:id/validate` | connector.manage | — → `{valid,errors[]}` | 422 |
| POST | `/integration/connectors/:id/enable` | connector.manage | `{version}` → `{state}` | 422 404 |
| POST | `/integration/connectors/:id/deprecate` | connector.manage | `{version,force?}` → `{state}` | 409 |
| GET | `/integration/connectors/:id/operations` | connector.manage | — → `{operations[]}` | 404 |

```json
// request                                // 201
{ "manifest": { "id": "talentos.rest",    { "id": "talentos.rest", "version": "1.0.0",
  "version": "1.0.0", "protocol": "rest" }   "state": "validated", "manifest_hash": "sha256:9f2c…" }
```

Validate returns per-field errors, never a bare boolean:
`{ "valid": false, "errors": [{ "field": "execution.egress_allowlist", "code": "required" }] }`.

### 2.2 Connections

| Method | Path | Scope | Req → Resp | Errors |
|---|---|---|---|---|
| GET | `/integration/connections` | connection.manage | — → `{items}` (metadata only) | 403 |
| POST | `/integration/connections` | connection.manage | `{connector_id,name,endpoint,secret}` → 201 `{secret_ref,last4}` | 422 |
| POST | `/integration/connections/:id/rotate` | connection.manage | `{secret}` → `{secret_ref,last4}` | 404 502 |
| POST | `/integration/connections/:id/test` | connection.manage | — → `{ok,latency_ms,error?}` (30s cap) | 502 |
| DELETE | `/integration/connections/:id` | connection.manage | — → 204 | 409 (pinned) |

```json
{ "connector_id": "talentos.rest", "name": "HRIS prod", "endpoint": "https://api.example.com",
  "secret_ref": "secret/data/talentos/tenants/<tid>/connectors/<cid>", "last4": "9f2c" }
```

Secrets are never returned — same rule as provider keys
(`crates/api/gateway/src/secrets.rs`, ADR-003).

### 2.3 Flows, schemas, mappings, versions, deployment

| Method | Path | Scope | Req → Resp | Errors |
|---|---|---|---|---|
| GET/POST | `/integration/flows` | flow.author | — / `{flow}` → `{items}` / 201 | 422 |
| GET | `/integration/flows/:id` | flow.author | — → draft + versions | 404 |
| PUT | `/integration/flows/:id` | flow.author | `{flow}` + `If-Match` → `{flow}` | 409 |
| POST | `/integration/flows/:id/validate` | flow.author | — → `{valid,errors[]}` | 422 |
| POST | `/integration/flows/:id/versions` | flow.author | — → 201 `{version+1}` | 409 |
| POST | `/integration/flows/:id/publish` | flow.publish | `{version,note}` → `{state}` | 422 409 |
| POST | `/integration/flows/:id/unpublish` | flow.publish | `{version}` → `{state}` | 404 |
| GET | `/integration/flows/:id/versions/:v` | flow.author | — → immutable snapshot | 404 |
| GET/POST | `/integration/schemas` | flow.author | — / `{schema,$id}` → 201 | 422 |
| GET/POST | `/integration/mappings` | flow.author | — / `{mapping}` → 201 | 422 |
| POST | `/integration/mappings/:id/versions` | flow.author | `{mapping}` → 201 | 409 |
| POST | `/integration/mappings/:id/check` | flow.author | `{source_ref,target_ref}` → `{compatible,errors[]}` | 422 |
| GET | `/integration/deployments` | flow.publish | — → live version per flow | 403 |

```json
{ "version": 7, "note": "map SCIM userName" }
{ "flow_id": "flw_01", "published_version": 7, "state": "active", "published_at": "…" }
```

```json
{ "compatible": false, "errors": [
  { "code": "missing_required_target", "field": "userName", "message": "no mapping, no default" },
  { "code": "type_mismatch", "field": "active", "message": "target boolean, expr yields string" } ] }
```

### 2.4 Execution inspection, authorization, audit

| Method | Path | Scope | Req → Resp | Errors |
|---|---|---|---|---|
| GET | `/integration/executions` | execution.read | filters `flow_id,state,from,to` → `{items}` | 403 |
| GET | `/integration/executions/:id` | execution.read | — → run + attempts | 404 |
| GET | `/integration/executions/:id/attempts` | execution.read | — → `{items}` paginated | 404 |
| POST | `/integration/executions/:id/cancel` | execution.control | — → 202 `{state:"cancelling"}`, idempotent | 409 |
| POST | `/integration/executions/:id/replay` | execution.control | `{note}` → 202 new execution | 409 404 |
| GET | `/integration/dead-letters` | execution.read | — → `{items}` | 403 |
| POST | `/integration/dead-letters/:id/redrive` | execution.control | — → 202 | 409 |
| GET/PUT | `/integration/authorization/policies` | connector.manage | — / `{policies}` → `{policies}` | 409 |
| GET | `/integration/audit` | audit.read | `limit`/`offset` → `{items}` | 403 |

```json
{ "id": "exe_01H…", "flow_id": "flw_01", "flow_version": 7, "state": "failed",
  "trigger": { "kind": "schedule", "schedule_id": "sch_1" },
  "started_at": "…", "finished_at": "…",
  "attempts": [{ "n": 1, "node": "call_scim", "error": "connector.rate_limited",
                 "retryable": true }],
  "idempotency_key": "tid:flw_01:7:schedule:emp_123" }
```

---

## 3. Data Plane API — PROPOSED

Called by the Runtime, not the browser. Service identity, tenant-bound, inherits no
human RBAC role (ADR-009, ADR-010).

| Method | Path | Auth | Req → Resp | Errors |
|---|---|---|---|---|
| POST | `/runtime/executions` | service token + `Idempotency-Key` | `{flow_id,version,trigger,input}` → 202 `{execution_id,state}` | 422 409 |
| POST | `/runtime/executions/:id/cancel` | service token | — → 202 `{state:"cancelling"}` | 409 |
| GET | `/runtime/executions/:id` | service token | — → run + attempts (§2.4 shape) | 404 |
| POST | `/runtime/executions/:id/replay` | service token + new key | `{note}` → 202, references the original | 409 |
| POST | `/runtime/webhooks/:flow_id/:token` | webhook token only | raw provider payload → 202 | 404 429 |
| GET | `/runtime/health` | service token | — → `{state,lease_id,queue_depth,dlq_depth,version}` | 503 |

```json
{ "flow_id": "flw_01", "version": 7, "trigger": { "kind": "webhook", "token_id": "tok_1" },
  "input": { "employee": { "id": "emp_123" } } }
```

Rules: no `tenant_id` in any body (the token carries it); every mutation needs an
idempotency key; a disabled flow's webhook returns `404`, not `409`, so replay
cannot probe existence (`01-domain-boundaries.md` §5 test 4); `4xx` is terminal,
`5xx`/timeouts are retryable.

---

## 4. Versioning policy

1. **URL major version only:** `/api/v1/…`. Additive fields never bump it.
2. **No `v2` without a breaking change.** Announce with `Deprecation` + `Sunset`
   headers; minimum 90 days overlap.
3. **Field removal** = new major version, or a per-tenant flag during migration.
4. **Unknown enum values are rejected** (`422`) where the enum is closed — silently
   ignoring them hides connector drift.
5. **Metadata versions are tenant data, not API versions.** A published flow
   `version: 7` is immutable; a fix is a new version and a re-publish.

---

## 5. Error code registry (shared by both planes)

| HTTP | `error` | When |
|---|---|---|
| 400 | `invalid_request` | Malformed body or query |
| 401 | `token_malformed` / `token_expired` | VERIFIED wording, `middleware/auth.rs` |
| 403 | `forbidden` | Authenticated, missing permission |
| 404 | `not_found` | Includes disabled flows on the Data Plane |
| 409 | `version_conflict` / `idempotency_conflict` / `flow_version_published_immutable` | Stale `If-Match`, key reuse with a different body, in-place edit |
| 422 | `manifest_invalid` / `flow_invalid` / `schema_incompatible` | Connector manifest, flow graph, mapping contract |
| 429 | `rate_limited` | Includes `retry_after_seconds` |
| 502 | `connector_failure` | Remote failed; run retryable |
| 503 | `runtime_unavailable` | No lease holder; Control Plane stays up |

Codes are snake_case, matching existing practice (`invalid_request`, `not_found`,
`provider_unreachable` in `crates/api/gateway/src/handlers/ai_providers.rs`).

---

## 6. Existing routes that shaped this contract (VERIFIED_EXISTING)

| Route | File | Why it matters |
|---|---|---|
| `POST /api/v1/mcp` | `crates/api/mcp/src/lib.rs`; `router.rs` | JSON-RPC over HTTP, version negotiation, destructive-tool gate — the `mcp` connector pattern |
| `POST /api/v1/plugins/:name/invoke` | `handlers/plugins.rs` | Capability-gated invocation: closest existing "call a certified unit" |
| `GET /api/v1/audit` | `handlers/audit.rs` | The `limit`/`offset` clamp the new audit endpoint must copy |
| `POST /api/v1/ai/providers/:id/models` | `handlers/ai_providers.rs` | Existing "probe a remote system, normalise the result" |
| `/health`, `/metrics` | `handlers/health.rs`, `metrics.rs` | Liveness surface; do not overload for runtime health |

## 7. Implementation gap

Nothing in §2 or §3 exists. To ship any of it: a metadata-store migration, the crate
that owns the handlers, entries in `crates/core/authz/src/rbac.rs`, two CORS headers
in `router.rs`, and the Runtime process itself (ADR-001). Until then this is a
target contract, not a description.

# 02 — WS1 Documentation Report

**Campaign:** TALENTOS-ARF-V1 · **Date:** 2026-11-03
**Status:** documentation only. No code, migration, route or runtime change.
**Baseline HEAD:** `8709baa`

## Files written

| File | Contents |
|---|---|
| `02-connector-sdk.md` | Manifest schema, lifecycle, operation/error/idempotency semantics, v1 catalog, PROPOSED extension contracts, certification rule |
| `02-api-contracts.md` | Control Plane + Data Plane endpoint contracts, conventions, versioning policy, error registry |
| `03-flow-engine.md` | Flow schema, 11 node types, execution semantics, validation rules |
| `03-mapping-engine.md` | Mapping schema, transform set, determinism rule, compatibility checks, worked SCIM example |

## Verification performed

```
grep -c '\.route(' crates/api/gateway/src/router.rs          → 186
grep -rni 'connector' --include=*.rs crates/                 → 0
grep -rni 'flow_engine|node_graph' --include=*.rs crates/    → 0
grep -rni 'mapping_engine|field_mapping' --include=*.rs crates/ → 0
grep -rn  'integration' --include=*.rs crates/               → 5, all "integration test" comments
```

Every VERIFIED_EXISTING claim cites a repository path. Every new surface is marked
PROPOSED. No endpoint, table, crate or migration is described as built.

## Existing code that informed the contracts

- Route style, CORS header allowlist, `x-request-id`:
  `crates/api/gateway/src/router.rs`
- Auth, `require()` → 403: `crates/api/gateway/src/middleware/auth.rs`
- Error shape: `crates/api/gateway/src/dto/responses.rs`
- Pagination clamp: `crates/api/gateway/src/handlers/audit.rs`
- RBAC matrix (no integration resource today): `crates/core/authz/src/rbac.rs`
- MCP server + destructive-tool gate: `crates/api/mcp/src/lib.rs`
- Capability-gated invocation: `crates/ext/talentos-wasm-host/src/capabilities.rs`,
  `crates/api/gateway/src/handlers/plugins.rs`
- Deterministic DAG precedent: `crates/domain/talentos-payroll/src/dag.rs`
- Outbound retry precedent (3 attempts): `crates/api/gateway/src/notify_worker.rs`
- Employee source columns: `migrations/20261025000001_initial_schema.sql` (58–85)

## Open decisions handed forward

1. `Idempotency-Key` / `If-Match` must be added to `router.rs` CORS `allow_headers`
   before any browser call to the proposed API can pass preflight.
2. New RBAC resources (§1 of `02-api-contracts.md`) need matrix review before
   implementation — the current matrix has none.
3. Adapter certification (§5 of `02-connector-sdk.md`) is the gate that prevents an
   untested connector from being marked operational.
4. Vendor drivers (Oracle, SQL Server, MySQL) are new dependencies and should stay
   behind feature flags so v1 does not pull them in.

## What this report does not claim

No connector SDK, API, flow engine or mapping engine exists. The integration
platform remains entirely PROPOSED, consistent with
`01-target-architecture.md` §4.4–§4.6.

# WS2 Slice 1 — Control Plane: Connector Registry + Connection Management

**Status:** implemented, migrated and tested. **Date:** 2026-11-03.
**Repo:** `talentos`, branch `master`, baseline HEAD `ceb0d70`.

This slice builds the first real Control Plane store and API. It does not build a
Flow Engine, executor, connector transport or secret writer; connections only hold
an OpenBao **reference** and test **results** are recorded, not performed.

---

## 1. Migration

`migrations/20261217000001_integration_control_plane.sql` (applied, exit `0`).

| Table | Columns |
|---|---|
| `integration_connectors` | `id`, `tenant_id`, `connector_id`, `name`, `version`, `protocol`, `manifest JSONB`, `risk`, `status draft\|published\|disabled`, `created_at`, `updated_at`; `UNIQUE (tenant_id, connector_id, version)` |
| `integration_connections` | `id`, `tenant_id`, `connector_id`, `connector_version`, `name`, `endpoint`, `secret_ref`, `state active\|disabled`, `last_test JSONB`, `last_verified_at`, `created_at`, `updated_at`; `UNIQUE (tenant_id, name)`; composite FK `(tenant_id, connector_id, connector_version)` → connector version |

- **RLS:** `ENABLE` + `FORCE` + policy `tenant_id = current_tenant_id()` (USING and
  WITH CHECK) on both tables.
- **Grants:** no explicit grants were added. `scripts/init-db.sql` default
  privileges already grant `SELECT/INSERT/UPDATE/DELETE` to `talentos_app`; verified
  on the dev DB (`relacl` includes `talentos_app=arwd`).
- **Published immutability:** enforced by trigger
  `integration_connectors_immutable` (backstop) **and** by the service
  (`update_draft` returns `ConnectorPublished`). The trigger rejects changes to
  `connector_id`, `name`, `version`, `protocol`, `manifest` or `risk` while
  `OLD.status = 'published'`; status transitions such as `published → disabled`
  remain legal. Tested both through the service and with a direct SQL update.

## 2. Service

`crates/services/talentos-hr-runner/src/integration.rs` (module exported from
`lib.rs`). Every operation uses `TenantPool::begin_tenant` plus an explicit
`tenant_id` predicate; all failures are typed `IntegrationError` variants.

| Function | Behaviour |
|---|---|
| `create_connector` | validates id/name/version/protocol/risk + object manifest; duplicate version → `DuplicateConnectorVersion` |
| `list_connectors` | `include_disabled` filter (default hides `disabled`), `limit`/`offset`, returns `(items, total)` |
| `get_connector` | by `connector_id` + `version` |
| `update_draft` | edits draft only; `published` → `ConnectorPublished`, `disabled` → `ConnectorDisabled` |
| `publish_connector` | `draft → published`; second publish → `ConnectorAlreadyPublished` |
| `disable_connector` | `draft|published → disabled`; second disable → `ConnectorAlreadyDisabled` |
| `create_connection` | validates connector version exists in the tenant before insert |
| `list_connections` / `get_connection` | paginated / by id, tenant-scoped |
| `update_connection` | partial name/endpoint/secret_ref/state; duplicate name → `DuplicateConnectionName` |
| `delete_connection` | idempotent only in the sense that a second delete is `ConnectionNotFound` |
| `record_connection_test` | stores `{ok,latency_ms,error}` in `last_test`; `last_verified_at = now()` on success, `NULL` on failure |

`ConnectionView` has **no `secret_ref` field**, only `has_secret: bool`, so leaks are
structurally impossible on the read path.

## 3. Gateway endpoints

Handler `crates/api/gateway/src/handlers/integration.rs`; routes in
`crates/api/gateway/src/router.rs`.

| Method | Path | RBAC |
|---|---|---|
| GET | `/api/v1/integrations/connectors` (`limit`, `offset`, `include_disabled`) | `integration.read` |
| POST | `/api/v1/integrations/connectors` | `integration.manage` |
| GET | `/api/v1/integrations/connectors/:connector_id?version=` | `integration.read` |
| PATCH | `/api/v1/integrations/connectors/:connector_id?version=` | `integration.manage` |
| POST | `/api/v1/integrations/connectors/:connector_id/publish` | `integration.manage` |
| POST | `/api/v1/integrations/connectors/:connector_id/disable` | `integration.manage` |
| GET | `/api/v1/integrations/connections` (`limit`, `offset`) | `integration.read` |
| POST | `/api/v1/integrations/connections` | `integration.manage` |
| GET | `/api/v1/integrations/connections/:id` | `integration.read` |
| PATCH | `/api/v1/integrations/connections/:id` | `integration.manage` |
| DELETE | `/api/v1/integrations/connections/:id` | `integration.manage` |
| POST | `/api/v1/integrations/connections/:id/test` | `integration.manage` |

- RBAC added to `crates/core/authz/src/rbac.rs`: `integration.manage` = `hr_admin`;
  `integration.read` = `hr_admin`, `payroll_manager`.
- List responses use the documented envelope:
  `{items,total,limit,offset,next_offset}`; `limit` defaults 50 and clamps 1..200.
- Errors use the existing `ErrorResponse` shape. Status mapping: 404 not found,
  409 conflicts (`connector_published_immutable`, `already_published`,
  `already_disabled`, `version_conflict`, `name_conflict`, `connector_disabled`),
  422 validation, 500 internal (logged, message not leaked).
- Correlation id: the codebase has `x-request-id` request tracing but no
  handler-level passthrough into error envelopes; this slice keeps the existing
  behaviour rather than inventing one.

## 4. Test evidence

Commands (run from repo root with
`DATABASE_URL=postgres://talentos:talentos_dev_secret@localhost:5432/talentos`):

| Command | Result |
|---|---|
| `cargo test -p talentos-hr-runner --test integration` | **3 passed**, 0 failed |
| `cargo test -p talentos-gateway --test integration` | **3 passed**, 0 failed |
| `cargo test -p talentos-authz integration` | **1 passed** (RBAC matrix) |
| `cargo clippy -p talentos-gateway -p talentos-hr-runner -p talentos-authz -- -D warnings` | clean |
| `cargo clippy -p talentos-gateway -p talentos-hr-runner --tests -- -D warnings` | clean |
| `sqlx migrate run` | **exit 0** (`Applied 20261217000001`) |

Coverage:

- connector create/list/get, draft update, publish, publish twice (409), update
  published (409), disable, default list hides disabled, `include_disabled` shows it;
- DB trigger rejects a direct content update on a published version;
- connection CRUD, `has_secret` true/false, serialized views contain no
  `secret_ref` and no secret path, test result recording (success and failure);
- tenant isolation: tenant B gets 404/empty lists for tenant A connector and
  connection;
- RBAC: `employee` 403 on read and write; `payroll_manager` 200 on read and 403 on
  write; `hr_admin` allowed.

## 5. Limitations / not built

- No manifest JSON-Schema validation beyond "must be a JSON object"; the full
  §2 validation matrix is a later slice.
- No actual connector test execution; `/test` records an externally supplied
  result.
- `secret_ref` is accepted verbatim and not written/validated against OpenBao; no
  rotation endpoint. `update_connection` cannot clear `secret_ref` to NULL
  (`COALESCE` semantics).
- No `Idempotency-Key` or `If-Match`/`ETag` handling; no audit-log emission.
- RLS is enabled and forced, but the test suite connects as the migration owner
  (superuser in dev), so isolation tests exercise the explicit `tenant_id`
  predicates rather than the policy itself.
- No flow/mapping/execution/dead-letter tables or endpoints from
  `02-api-contracts.md`; those remain WS2+ work.
- No deployment performed.

# 02 — Connector SDK (WS1)

**Campaign:** TALENTOS-ARF-V1 · **Date:** 2026-11-03 · **Baseline HEAD:** `8709baa`
**Status:** contract documentation only. No crate, table, route or executor exists.
**Proof:** `grep -rni "connector" --include=*.rs crates/` → 0 results. Everything
here is PROPOSED unless a path is cited.

---

## 1. What a connector is

A connector is a **declarative manifest plus a certified transport** that turns one
external protocol into typed operations the Flow Engine can call
(`03-flow-engine.md` §2.7).

| It is | It is not |
|---|---|
| Versioned, reviewable metadata (ADR-002) | Per-tenant generated code |
| A fixed operation set with JSON Schema contracts | An arbitrary script host |
| Credential-blind: references an OpenBao path (ADR-003) | A place to store a password |
| Sandboxed with a per-connector egress allowlist | A component with HR Core DB grants (ADR-010) |

Closest existing pattern (VERIFIED_EXISTING): capability-gated plugin invocation in
`crates/ext/talentos-wasm-host/src/capabilities.rs` and
`crates/api/gateway/src/handlers/plugins.rs` — declared capabilities, checked before
the call, no ambient authority. The SDK copies that *shape*, not that code.

---

## 2. Manifest schema

One JSON document, stored in the Control Plane metadata store (PROPOSED), keyed by
`id@version`. Published versions are immutable (ADR-005 rule, restated here).

```json
{
  "id": "talentos.rest", "version": "1.0.0", "protocol": "rest",
  "display_name": "REST / OpenAPI",
  "auth": { "mode": "bearer|api_key|basic|oauth2_client_credentials|none",
            "secret_ref": "secret/data/talentos/tenants/{tid}/connectors/{cid}",
            "header": "Authorization" },
  "capabilities": ["http.json", "http.multipart", "openapi3"],
  "operations": [{
    "name": "create_employee", "kind": "write", "method": "POST", "path_template": "/employees",
    "input_schema_ref": "sch_in_1", "output_schema_ref": "sch_out_1",
    "idempotency": { "supported": true, "header": "Idempotency-Key" }, "retryable": true }],
  "execution": { "isolation": "connector_worker",
                 "egress_allowlist": ["api.example.com:443"],
                 "max_concurrency": 4,
                 "rate_limit": { "requests": 60, "window_seconds": 60 } },
  "timeouts": { "connect_ms": 5000, "request_ms": 30000, "total_ms": 120000 },
  "retry": { "max_attempts": 5, "backoff": "exponential", "initial_ms": 500,
             "max_ms": 30000, "jitter": true,
             "retry_on": ["connection_reset", "timeout", "http_429", "http_5xx"] },
  "data_classification": { "input": "pii", "output": "pii",
                           "log_payloads": false, "retention_days": 30 },
  "audit": { "emit": ["invoke", "result", "error"],
             "include_request_body": false, "correlation": "x-request-id" }
}
```

| Field | Rule |
|---|---|
| `id` | Stable across versions; `talentos.` prefix reserved for built-ins |
| `version` | semver; bumped on any contract change; published = immutable |
| `protocol` | `rest` \| `webhook` \| `mcp` in v1 (§5) |
| `auth` | `none`, or a `secret_ref`; never an inline secret (ADR-003) |
| `capabilities` | Declared and checked before execution; unknown = manifest invalid |
| `operations` | At least one; each names input and output schemas |
| `execution.egress_allowlist` | Mandatory, literal `host:port` |
| `timeouts` | All three present; `total_ms >= request_ms` |
| `retry` | `max_attempts >= 1`; `retry_on` from a closed enum |
| `data_classification` | `pii` forbids payload logging |
| `audit` | At minimum `emit: ["invoke","result"]` |

Validation is **total**: any failure rejects the manifest. There is no "warn and
continue".

---

## 3. Lifecycle

`discover → register → validate → enable → execute → deprecate`

| Stage | Gate | Failure state |
|---|---|---|
| discover | manifest enters the tenant catalog | `unregistered` |
| register | `id@version` unused, or byte-identical (idempotent) | `rejected` |
| validate | schema checks + auth ref resolves in OpenBao + allowlist is literal | `invalid` |
| enable | `integration.connector.manage` (PROPOSED) and validation passed | `disabled` |
| execute | enabled, flow version published, tenant quota available | run-scoped `error` |
| deprecate | no enabled flow pins it, or explicit force with a run-count warning | `deprecated` |

- `deprecate` never deletes; a version pinned by a published flow cannot be removed.
- Re-enabling `deprecated` is allowed; `invalid` is not.
- Every transition is audited with actor, tenant, correlation id, manifest hash.

---

## 4. Operations, errors, idempotency

| `kind` | Meaning | Retry default |
|---|---|---|
| `read` | No remote mutation | retryable |
| `write` | Creates/updates remote state | only if `idempotency.supported` |
| `delete` | Removes remote state | no, unless declared idempotent |
| `probe` | Health/metadata call | retryable, not counted against quota |

**Error taxonomy** — closed set; every adapter maps native errors into exactly one:

| Code | Retryable | Meaning |
|---|---|---|
| `connector.auth_failed` | no | Credential rejected; operator action |
| `connector.forbidden` | no | Credential valid, operation not permitted |
| `connector.not_found` | no | Target absent |
| `connector.contract_violation` | no | Response failed `output_schema` |
| `connector.invalid_request` | no | Mapping produced input the remote rejected |
| `connector.rate_limited` | yes | Honour `Retry-After` |
| `connector.remote_error` | yes | Remote 5xx |
| `connector.timeout` | yes | Bounded by `timeouts.*` |
| `connector.network` | yes | DNS/TCP/TLS failure |
| `connector.egress_denied` | no | Host not allowlisted; treat as a security event |
| `connector.quota_exceeded` | later | Tenant/connector budget exhausted |
| `connector.disabled` | no | Version not enabled for this tenant |
| `connector.internal` | no | Adapter bug; always logged with a correlation id |

`egress_denied` and `auth_failed` also emit an audit entry classified `security`.

**Idempotency** — declared per operation, never implied:

```json
"idempotency": { "supported": true, "scope": "operation", "header": "Idempotency-Key" }
```

`supported: true` means the remote dedupes on the header, so retries are safe.
`false` on a `write` forbids auto-retry; the attempt goes to the DLQ (ADR-008).
Key derivation is fixed by the runtime — `tenant + flow_version + trigger + record_id`;
the manifest only says where to send it.

---

## 5. Connector catalog

| Connector | Status | Evidence |
|---|---|---|
| REST / OpenAPI | **PROPOSED** | `reqwest` is in the workspace but the only client is LLM-specific (`crates/ai/talentos-model-gw/src/client.rs`). No generic adapter. |
| Webhook ingress | **PROPOSED** | No inbound webhook route. Outbound-only precedent: `crates/api/gateway/src/metrics_alert.rs`, `notify_worker.rs` (`MAX_ATTEMPTS = 3`). |
| MCP client | **PROPOSED** as connector; server **VERIFIED_EXISTING** | Gateway hosts MCP: `crates/api/mcp/src/lib.rs`, route `/api/v1/mcp`. Consuming a third-party server is unbuilt; study §3 proposes `mcp_servers`, ADR-004 fixes the stdio allowlist. |

Nothing else ships in v1. Below are extension contracts only — **all PROPOSED**,
interface sketches, no adapter code.

| Protocol | Operations | Hard constraint |
|---|---|---|
| `postgres` | `read_query`, `write_tx` | Read-only by default; no DDL; tenant-approved host |
| `oracle` | `read_query`, `write_tx`, `call_procedure` | New driver behind a feature flag |
| `sqlserver` | `read_query`, `write_tx`, `bulk_insert` | TLS required |
| `mysql` | `read_query`, `write_tx` | TLS; no `LOAD DATA LOCAL` |
| `ldap` | `search`, `bind` | Binds rate-limited as writes; never log passwords |
| `scim` | `users.*`, `groups.*` | Filter subset only; used by `03-mapping-engine.md` §5 |
| `sftp` | `list`, `get`, `put`, `rename`, `delete` | Host-key pinning; no `exec`; path confined to a declared root |
| `graphql` | `query`, `mutate` | Operation-name allowlist; no introspection in production |
| `queue` | `publish`, `consume` | Kafka/AMQP/SQS behind one interface; consumer offsets owned by the Runtime |

If an adapter trait is ever written (PROPOSED):

```rust
#[async_trait]
pub trait ConnectorAdapter: Send + Sync {
    fn protocol(&self) -> &'static str;
    async fn validate(&self, m: &Manifest) -> Result<(), Vec<ManifestError>>;
    async fn execute(&self, op: &Operation, input: Value, ctx: &CallContext)
        -> Result<Value, ConnectorError>;   // §4 taxonomy, never a string
}
```

`CallContext`: tenant id, correlation id, idempotency key, short-lived secret handle
— never the secret value.

### Certification rule

> **No adapter is "operational" without executable integration tests.**

"Operational" = listed in the Studio, enableable, callable from a flow. An adapter
covered only by unit tests with mocked transports stays `experimental` and cannot
be enabled by a tenant. Minimum CI set, against a disposable instance:

1. Happy path per declared operation.
2. Auth failure → `connector.auth_failed`, no retry.
3. A response failing `output_schema` → `connector.contract_violation`.
4. Timeout at `request_ms` → retried per policy.
5. Egress denial for a non-allowlisted host.
6. Idempotent replay sends the same key and the remote dedupes.
7. `log_payloads: false` leaves no PII in logs.

Same discipline as `crates/api/mcp/src/lib.rs`
(`guard_blocks_unknown_and_unapproved_destructive_tools`): the security decision is
split out so it is testable without a database.

---

## 6. Security boundaries

| Rule | Source |
|---|---|
| Secrets only in OpenBao; manifests hold a path | ADR-003; `crates/api/gateway/src/secrets.rs` |
| Tenant-scoped path `…/tenants/<tid>/connectors/<cid>` | Provider-path pattern in `secrets.rs` |
| No HR Core DB grants | ADR-010; `01-domain-boundaries.md` §2 |
| Egress allowlist per connector, not per process | `01-deployment-topology.md` |
| Destructive operations need an explicit grant | `X-TalentOS-Allow-Destructive`, `crates/api/mcp/src/lib.rs` |
| PII classification forbids payload logging | `handlers/privacy.rs`; `talentos-hr-runner/src/pdpa.rs` |

**Not built:** connector crate, manifest store, adapter trait, lifecycle state
machine, certification harness, egress policy engine. `01-target-architecture.md`
§4.6 lists all six as PROPOSED; this file fixes only the contract.

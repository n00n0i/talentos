# 03 — Flow Engine (WS1)

**Campaign:** TALENTOS-ARF-V1 · **Date:** 2026-11-03 · **Baseline HEAD:** `8709baa`

> **STATUS: PROPOSED.** No flow engine, schema, route or migration exists.
> `grep -rni "flow_engine\|node_graph" --include=*.rs crates/` → 0 results.

Scope: the contract for a metadata-defined, deterministic execution graph. The
engine interprets published metadata; it never generates or evaluates tenant code
(ADR-002).

**Precedent (VERIFIED_EXISTING):** the payroll DAG
`crates/domain/talentos-payroll/src/dag.rs` — fixed topological order, a
`StepResult { step, input, output, error }` trace, "Pure functions, deterministic,
no side effects", with a `deterministic_same_input_same_output` test. The Flow
Engine keeps that trace discipline but is metadata-driven.

---

## 1. Flow definition schema

```json
{
  "id": "flw_01H…", "name": "Employee → Identity Provider", "version": 7, "state": "published",
  "scope": { "tenant_id": "tnt_…", "entity_ids": ["ent_1"], "environment": "draft|production" },
  "trigger": { "kind": "schedule|webhook|manual|event", "cron": "0 2 * * *", "timezone": "Asia/Bangkok" },
  "contracts": { "input_schema_ref": "sch_employee_v3", "output_schema_ref": "sch_scim_user_v2" },
  "parameters": [{ "name": "batch_size", "type": "integer", "default": 100, "max": 500 }],
  "graph": { "entry": "n_trigger", "nodes": [], "edges": [] },
  "authorization": { "required_scope": "integration.execution.read",
                     "run_as": "service:integration-runtime",
                     "connector_allowlist": ["talentos.rest"] },
  "error_policy": { "on_node_failure": "retry_then_dlq", "max_attempts": 5,
                    "on_partial_failure": "continue_and_report", "dlq": true },
  "lifecycle": { "created_at": "…", "created_by": "usr_…", "published_at": "…",
                 "published_by": "usr_…", "supersedes": 6 },
  "audit": { "correlation_header": "x-request-id", "log_payloads": false }
}
```

| Field | Rule |
|---|---|
| `id` | Immutable for the life of the flow |
| `scope.tenant_id` | Must equal the JWT tenant; a mismatch is `403`, never an override |
| `version` | `>= 1`; incremented on publish; **published versions are immutable** |
| `trigger` | Exactly one trigger node, and it must be `graph.entry` |
| `contracts` | Both refs must resolve in the schema registry |
| `graph` | At least one node; validated per §4 |
| `authorization.connector_allowlist` | Subset of what the tenant has enabled |
| `error_policy` | Defaults stored explicitly, never implied at runtime |
| `lifecycle` | Append-only; `published_by` is never rewritten |
| `audit.log_payloads` | `false` whenever any node handles PII (`02-connector-sdk.md` §2) |

`environment: production` is required for a schedule or webhook to fire.

---

## 2. Node types

Every node is `{ id, type, config, input, output }`; `input`/`output` name a schema
ref or `@flow.input` / `@flow.output`. Edges are
`{ from, to, when: "success"|"failure"|"true"|"false" }`.

| # | `type` | Config | Semantics |
|---|---|---|---|
| 2.1 | `trigger` | `kind`, `cron`, `timezone`, `webhook_token_ref`, `event` | Exactly one per flow; must be `entry`; no incoming edge. A disabled flow's trigger does not fire and its webhook returns `404`. |
| 2.2 | `validate` | `schema_ref`, `on_violation: fail\|dlq\|continue` | Structural and required-field checks only; no coercion. `continue` is forbidden when the output feeds a `connector_action` write. |
| 2.3 | `transform` | `mode: mapping\|template`, `mapping_ref`, `mapping_version` | Calls the Mapping Engine (`03-mapping-engine.md`). Deterministic only — ADR-007. An LLM is not a valid mode. |
| 2.4 | `map` | `over`, `concurrency`, `on_item_failure: collect\|fail_fast\|skip` | Applies the downstream subgraph per element. `collect` (default) means the output is a set; ordering is not guaranteed. Concurrency is capped by tenant quota. |
| 2.5 | `condition` | `expression` (closed AST: compare/boolean/arithmetic on typed paths) | Exactly two outgoing edges: `when: "true"` and `when: "false"`. No scripting language. |
| 2.6 | `route` | `on`, `cases{}`, `default` | Multi-branch dispatch on one field; exactly one edge fires. `default` is mandatory so an unknown value cannot drop a record. |
| 2.7 | `connector_action` | `connection_id`, `operation`, `idempotency_key`, `timeout_ms`, `on_error` | The only I/O node. `operation` must exist in the manifest and be enabled; the response is checked against `output_schema`. |
| 2.8 | `wait` | `mode: duration\|until`, `duration_ms`, `until`, `max_wait_ms` | `max_wait_ms` mandatory so a flow cannot pin a worker. A long wait is a scheduling hint, not a sleeping process. |
| 2.9 | `retry` | `target`, `max_attempts`, `backoff`, `initial_ms`, `max_ms`, `retry_on` | Wraps one node. If the inner node also retries, the **outer** policy wins — a double budget is rejected. `retry_on` may only name codes retryable in `02-connector-sdk.md` §4. |
| 2.10 | `error_handler` | `catch`, `action: dlq\|compensate\|emit_audit`, `compensate_node` | Reached by a `when: "failure"` edge. Catches only listed codes; anything else escalates to `error_policy.on_node_failure`. Compensation runs once, with the original input; its own failure is terminal. |
| 2.11 | `completion` | `result: success\|partial\|noop`, `emit_schema_ref` | Exactly one per terminal path. `partial` is the normal exit for a `map` that collected failures. |

Eleven node types. None executes tenant-supplied code.

---

## 3. Execution semantics

1. **Immutability.** Publishing snapshots the whole document into a new version.
   Editing a published flow creates a draft; in-place edit →
   `409 flow_version_published_immutable`.
2. **Version binding.** An execution is bound to the version it started with for its
   whole life — retries, replays and compensation included. Publishing v8 does not
   change an in-flight v7 run.
3. **At-least-once.** Every run and attempt carries
   `tenant + flow_id + version + trigger + record_id`. Retries reuse the key;
   consumers dedupe (ADR-008).
4. **Timeouts.** Node timeout → flow timeout → run lease. A node breach is
   `connector.timeout`; a flow breach cancels remaining nodes and ends
   `failed_timeout`.
5. **Cancellation.** Best-effort and idempotent: stop new nodes, let an in-flight
   connector call finish or time out, record `cancelled`. A cancelled run is never
   resumed, only replayed.
6. **Partial failure.** `continue_and_report` collects failed items, ends `partial`,
   and lists each failed record id with its error code. `fail_fast` stops at the
   first failure.
7. **Ordering.** Guaranteed only per idempotency key. Parallel `map` items and
   independent branches have no ordering contract.
8. **Determinism.** Same input + same flow version + same connector responses →
   same output and trace shape. Clock and random values are injected, never read by
   an expression.

Trace entries record `input_hash`/`output_hash`, not bodies, because PII must not
land in logs (`handlers/privacy.rs`, `talentos-hr-runner/src/pdpa.rs`):

```json
{ "node": "n_scim", "attempt": 1, "started_at": "…",
  "input_hash": "sha256:…", "output_hash": "sha256:…", "error": null }
```

---

## 4. Validation rules

Run by `POST /api/v1/integration/flows/:id/validate` (`02-api-contracts.md` §2.3)
before a version can be created, and again at publish. All violations are returned
together.

| Code | Rule |
|---|---|
| `no_trigger` / `multiple_triggers` | Exactly one `trigger` node |
| `entry_mismatch` | `graph.entry` is the trigger |
| `cycle` | Graph is acyclic; loops are expressed as `retry`, not edges |
| `unreachable_node` | Every node reachable from `entry` |
| `dead_end` | Every terminal path ends in a `completion` |
| `missing_contract` | Flow contracts and every node `input`/`output` resolve |
| `schema_mismatch` | Source output schema satisfies target input schema |
| `missing_operation` | Every `connector_action.operation` exists and is enabled |
| `connector_not_allowlisted` | Connection's connector is in `authorization.connector_allowlist` |
| `double_retry_budget` | A `retry` wrapper around an already-retrying node |
| `unbounded_wait` | `wait` without `max_wait_ms` |
| `route_without_default` | `route` declares `default` |
| `condition_bad_arity` | `condition` has exactly two outgoing edges |
| `error_handler_unknown_code` | `catch` pattern is not in the error taxonomy |
| `pii_logging_enabled` | `audit.log_payloads: true` on a PII flow |

```json
{ "valid": false, "errors": [
  { "code": "cycle", "field": "graph.edges", "node_ids": ["n_a", "n_b"] },
  { "code": "unreachable_node", "field": "graph.nodes", "node_ids": ["n_orphan"] } ] }
```

---

## 5. What the engine must never do

| Forbidden | Reason |
|---|---|
| Write HR Core tables | ADR-010; `01-domain-boundaries.md` §2 |
| Call an LLM during execution | ADR-007 |
| Evaluate tenant scripts or side-effecting templates | ADR-002 |
| Retry a `connector_action` whose operation declares `idempotency.supported: false` | ADR-008 |
| Read a secret directly | ADR-003 |
| Continue after `connector.egress_denied` | Security event |

**Not built:** flow table, node interpreter, scheduler, run store, DLQ,
cancellation path, trace writer. `01-target-architecture.md` §4.4–§4.5 lists all of
it as PROPOSED; this file is the contract the interpreter must satisfy and the
checklist its tests must cover.

# 01 — Architecture Decision Records (WS1)

**Campaign:** TALENTOS-ARF-V1 · **Date:** 2026-11-03
**Status convention:** `Accepted` = decision fixed for the target architecture. Implementation status is stated separately; no ADR claims the integration runtime is built.

---

## ADR-001 — Embedded Integration Studio, independent Integration Runtime

**Status:** Accepted · Runtime PROPOSED (not built)
**Context.** Tenants need to build integrations next to their HR data, but the HR/payroll gateway must stay available when an integration is broken, upgrading, or looping. Coupling execution to the gateway makes every connector bug an HR outage.
**Decision.** Author in an embedded Studio (web + gateway metadata). Execute in a separate Runtime service with its own process, queue, credentials and release cadence. Gateway↔Runtime is a publish/status contract only.
**Consequences.** TalentOS stays up when the Runtime is down; two deployables, runbooks and observability surfaces; the Runtime needs its own DB role and secret policy; authoring keeps the same login, session and tenant.
**Alternatives rejected.** All-in-gateway execution (one crash domain, no independent scaling); fully external iPaaS (data leaves the tenant boundary and HR write control is lost).
**Evidence.** `crates/api/gateway/src/router.rs`, `crates/api/gateway/src/main.rs`; no runtime crate/unit exists (`00-baseline-audit.md` §2).

---

## ADR-002 — Metadata-driven integrations over generated code

**Status:** Accepted · Implementation PROPOSED
**Context.** Per-tenant codegen creates unbounded code, review load, migration risk and supply-chain surface. Most HR integrations are "fetch → map → call domain API" pipelines.
**Decision.** Store connectors, mappings, schedules and pipeline versions as tenant-scoped metadata. A fixed interpreter executes published versions. New behaviour ships as metadata or a certified connector build, never per-tenant generated code.
**Consequences.** One engine to test, audit and sandbox; mapping changes become reviewable data; the engine must be expressive enough; escape hatches require a connector SDK; metadata needs versioning, validation and rollback.
**Alternatives rejected.** Per-tenant codegen (unreviewable); arbitrary tenant scripts (code execution without a sandbox model); hardcoded connectors only (every tenant request becomes a product release).
**Evidence.** No metadata tables or engine exist. Closest pattern is the capability-gated plugin registry (`migrations/20261028000005_tenant_plugins.sql`, `crates/ext/talentos-wasm-host/src/capabilities.rs`).

---

## ADR-003 — OpenBao is the only secret store

**Status:** Accepted · OpenBao client VERIFIED_EXISTING
**Context.** Provider keys, connector credentials and MCP env secrets must not land in Postgres, logs, API responses or browser storage. An existing pattern already stores AI provider keys in OpenBao KV v2.
**Decision.** Every integration/provider/MCP secret is written to OpenBao under a tenant-scoped path. Postgres stores only a reference/path and a boolean. Runtime and connectors receive short-lived, path-scoped credentials.
**Consequences.** Secrets survive DB backups without being in them; OpenBao is a hard dependency for secret-bearing operations and must fail closed; path policy and token lifecycle become deployment concerns; no Postgres fallback.
**Alternatives rejected.** Encrypted columns in Postgres (key management still in the app); per-tenant env vars (no rotation, leaks in process dumps); cloud-KMS-only (not portable to on-prem).
**Evidence.** `crates/api/gateway/src/secrets.rs` ("no Postgres key fallback"); `scripts/openbao-bootstrap.sh`; container `openbao` on `127.0.0.1:8200`.

---

## ADR-004 — MCP stdio is allowlisted, never arbitrary shell

**Status:** Accepted · Implementation PROPOSED
**Context.** MCP stdio servers are local processes, i.e. code execution. Users want local MCP tooling, but a tenant admin must not gain shell access to the gateway host.
**Decision.** v1 permits stdio only with a command-prefix allowlist (`npx`, `node`, `python3`), no shell metacharacters, a per-tenant working directory, the `opc` user, and `ai.settings.manage`. Remote transports are preferred. Everything not allowlisted is rejected at creation and spawn.
**Consequences.** Some legitimate servers are unusable until allowlisted; spawn paths must be audited and resource-limited; HTTP transports stay the default.
**Alternatives rejected.** Arbitrary commands (host compromise); HTTP only (blocks local/offline tooling); container-per-server in v1 (operational surface before demand is proven).
**Evidence.** Study §3/§8: `docs/superpowers/specs/2026-11-03-ai-providers-mcp-skills-agents-study.md`. Current MCP server is HTTP-only: `crates/api/mcp/src/lib.rs`; route `/api/v1/mcp`.

---

## ADR-005 — Skills are versioned; published versions are immutable

**Status:** Accepted · Implementation PROPOSED
**Context.** A skill is a prompt + tool allowlist + model role + KG scope. If editing silently changes agent behaviour, audits and rollbacks become impossible.
**Decision.** Every edit creates `version + 1`. Published versions are immutable. Agents pin an exact skill version. Built-in skills are seeded and copy-on-write, not edited in place.
**Consequences.** Reproducible behaviour and clean rollback; more rows and "in use by N agents" UI; deleting a referenced version is forbidden (deprecate instead).
**Alternatives rejected.** Mutable skills (unreproducible); git-style content hashes (heavier than needed for v1); global skills without tenant scope (cross-tenant leakage).
**Evidence.** Proposed schema in study §4; no `ai_skills` table exists (`00-baseline-audit.md` §2: Skills MISSING).

---

## ADR-006 — One default HR Assistant per tenant

**Status:** Accepted · Implementation PROPOSED
**Context.** Chat works today without an agent object. Requiring explicit agent selection on every request adds friction for the common HR question.
**Decision.** Seed one `HR Assistant` agent per tenant and use it when `agent_id` is absent. Tenants may create more agents and change the default, but cannot end up with no usable chat path.
**Consequences.** Chat works immediately after provider setup; a bad default prompt affects every unqualified request, so keep it conservative and read-oriented; explicit `agent_id` still wins.
**Alternatives rejected.** Require explicit agent (breaks current chat UX); no default and return 400 (unnecessary friction); a global shared agent (no tenant isolation).
**Evidence.** Current implicit path: `crates/api/gateway/src/handlers/ai.rs` (`body.agent.as_deref().unwrap_or("chat")`); no `ai_agents` table exists.

---

## ADR-007 — Deterministic mapping; no LLM in the execution path

**Status:** Accepted · Constraint on all future runtime code
**Context.** Payroll and HR mutations must be auditable and repeatable. An LLM inside a mapping or payroll step makes results non-deterministic, adds latency/cost, and creates a prompt-injection path into money movement.
**Decision.** Integration mappings are deterministic functions over typed inputs. LLMs may assist **authoring** (suggest a mapping, explain an error) but never execute inside a run. Payroll remains the pure DAG already implemented.
**Consequences.** Same input + same mapping version → identical output; mapping expressiveness must be designed explicitly with no "AI fixes it" escape; AI-assisted authoring stays outside the execution trust boundary.
**Alternatives rejected.** LLM per record (non-deterministic, costly, unsafe); LLM fallback on mapping failure (hides data errors); no authoring assistance (unnecessary usability loss).
**Evidence.** `crates/domain/talentos-payroll/src/dag.rs` ("Pure functions, deterministic, no side effects"); tool gating in `crates/ai/talentos-tools/src/registry.rs`.

---

## ADR-008 — At-least-once delivery with idempotency keys

**Status:** Accepted · Implementation PROPOSED
**Context.** Networks fail after the remote system committed. Exactly-once delivery across arbitrary external APIs is not achievable without remote support.
**Decision.** Deliver at least once. Every run/attempt carries a stable idempotency key derived from tenant + pipeline version + trigger + record identity. Retries reuse the key. Consumers dedupe; HR domain writes accept the key and are safe to replay.
**Consequences.** No silent loss on transient failure; duplicate attempts are possible, so domain APIs need idempotent write semantics; DLQ and replay tooling are mandatory; ordering is per-key, not global.
**Alternatives rejected.** At-most-once (data loss); exactly-once claims (false across third parties); in-memory retry only (lost on restart — today's `notify_worker` caps at 3 attempts and has no integration DLQ).
**Evidence.** Retry precedent: `crates/api/gateway/src/notify_worker.rs` (`MAX_ATTEMPTS = 3`). No integration outbox/DLQ exists.

---

## ADR-009 — Preserve RLS, RBAC, and payroll determinism

**Status:** Accepted · Inviolable
**Context.** These are the platform's core safety properties. An integration plane that weakens them turns a feature into a cross-tenant breach or a wrong-payroll incident.
**Decision.** Every new component uses the same tenant context and RBAC matrix and never bypasses RLS. Integration service identities are tenant-bound. Payroll stays LLM-free and deterministic. No runtime gets `BYPASSRLS`, superuser, or table ownership.
**Consequences.** Runtime performance is bounded by tenant-scoped queries (use a read model, not raw bypass); every new permission enters the central matrix and gets reviewed/tested; some operational shortcuts are permanently unavailable.
**Alternatives rejected.** A shared integration superuser (one bug leaks all tenants); a separate schema with weaker policy (drift and blind spots); letting AI approve payroll (destroys determinism and accountability).
**Evidence.** `crates/infra/db/src/lib.rs` (`begin_tenant`, `with_tenant_tx`); `migrations/20261025000002_rls_policies.sql` (FORCE RLS); `crates/core/authz/src/rbac.rs`; `crates/domain/talentos-payroll/src/dag.rs`.

---

## ADR-010 — HR writes only through domain APIs

**Status:** Accepted · Runtime enforcement PROPOSED
**Context.** The fastest integration is direct SQL. It is also the fastest way to skip validation, audit, approval rules, RLS assumptions and payroll invariants.
**Decision.** Control Plane and Runtime never write HR Core tables. All HR mutations go through TalentOS domain APIs with tenant auth, RBAC, validation, audit and idempotency. The Runtime DB role has no write grants on HR tables; it owns only run/attempt/DLQ state.
**Consequences.** Throughput depends on domain API capacity (batch endpoints may be needed later); domain APIs become a versioned compatibility surface; incident recovery cannot "just fix the row" from the runtime side.
**Alternatives rejected.** Direct SQL through a restricted view (still bypasses domain rules); stored procedures owned by the runtime (schema coupling and privilege creep); dual writes (divergence and double audit).
**Evidence.** Domain write paths: `crates/services/talentos-hr-runner/src/*.rs`, `crates/services/talentos-payroll-runner/`; RLS contract: `crates/infra/db/src/lib.rs`. No runtime exists yet; see `01-domain-boundaries.md` §2 and §5.

---

## Decision log summary

| ADR | Decision | Implementation |
|---|---|---|
| 001 | Embedded Studio + independent Runtime | Runtime PROPOSED |
| 002 | Metadata-driven over codegen | PROPOSED |
| 003 | OpenBao as sole secret store | VERIFIED_EXISTING |
| 004 | MCP stdio allowlist | PROPOSED |
| 005 | Immutable skill versions | PROPOSED |
| 006 | Default HR Assistant per tenant | PROPOSED |
| 007 | Deterministic mapping, no LLM execution | payroll verified |
| 008 | At-least-once + idempotency | PROPOSED |
| 009 | Preserve RLS/RBAC/payroll determinism | VERIFIED_EXISTING |
| 010 | HR writes only through domain APIs | enforcement PROPOSED |

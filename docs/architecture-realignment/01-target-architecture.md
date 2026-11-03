# 01 — Target Architecture (WS1)

**Campaign:** TALENTOS-ARF-V1 · **Date:** 2026-11-03
**Repo:** `/home/opc/content-creator/talentos` · **Baseline HEAD:** `fc3c02f`
**Scope:** architecture documentation only. No implementation, no schema change.

**Evidence rule:** every existing component cites a repository path. Anything not
traceable to code is labelled **PROPOSED** and must not be described as built.

---

## 1. C4 Context

```
 HR admin / payroll ─┐                        ┌─ employee / manager
                     ▼                        ▼
              ┌──────────────────────────────────────┐
              │ TalentOS · HR · Payroll · AI · Studio│
              └───────┬──────────────────────┬───────┘
                      │ outbound HTTPS       │ outbound HTTPS
                      ▼                      ▼
        LLM providers (OpenAI-compatible,    Thai HR/payroll services
        Anthropic/Google catalogs)           (bank, SSO, email) [PROPOSED connectors]
                      │                      │
        ┌─────────────┴──────────────────────┴─────────────┐
        │ Integration boundary (PROPOSED): Control Plane,  │
        │ Runtime, Connector/Adapter Framework             │
        └──────────────────────────────────────────────────┘
 Platform services: PostgreSQL (tenant data) · Redis · OpenBao (secrets)
```

Actors map to the verified RBAC roles `hr_admin`, `payroll_manager`, `manager`,
`employee` (`crates/core/authz/src/rbac.rs`).

---

## 2. C4 Container diagram

```
┌──────────┐ HTTPS :3000  ┌──────────────────────────┐
│ Browser  │─────────────▶│ Next.js web (VERIFIED)   │
└──────────┘              │ web/ · session proxy     │
                          └────────────┬─────────────┘
                                       │ HTTP :18479
┌──────────────────────────────────────▼──────────────────────────────────┐
│ Axum gateway (VERIFIED)  crates/api/gateway/src/router.rs (186 routes)  │
│ auth · RBAC · REST · GraphQL · MCP · plugins · AI · health              │
└──┬─────────────┬──────────────┬────────────────┬────────────────────────┘
   ▼             ▼              ▼                ▼
 HR runner    Payroll        AI crates        Extism WASM host
 (VERIFIED)   DAG + runner   model-gw/rag     tenant_plugins
              (VERIFIED)     agent/tools/mem  (VERIFIED)
   │             │              │                │
   └─────────────┴──────────────┴────────────────┘
                  ▼
 PostgreSQL (RLS) · Redis · OpenBao KV v2   [all VERIFIED]
┌──────────────────────────────────────────────────────────────┐
│ PROPOSED, NOT BUILT: Control Plane (embedded in gateway)     │
│   → Independent Runtime (separate process/service)           │
│   → Connector/Adapter Framework (sandboxed workers)          │
└──────────────────────────────────────────────────────────────┘
```

The dashed conceptual boundary is deliberate: the runtime is a **separate
failure domain**, not another handler inside the gateway.

---

## 3. Layer model

Six runtime layers plus one cross-cutting layer. "Six" counts runtime planes;
security/governance cuts across all of them.

| # | Layer | Current state |
|---|---|---|
| 1 | HR/Payroll Core | VERIFIED_EXISTING |
| 2 | HR Extension Platform | VERIFIED_EXISTING (WASM only); no-code metadata PROPOSED |
| 3 | AI Intelligence Platform | VERIFIED_EXISTING crates; user-authored MCP/skills/agents PROPOSED |
| 4 | Integration Control Plane | PROPOSED (no crate, no routes) |
| 5 | Independent Integration Runtime | PROPOSED (no crate, no process) |
| 6 | Connector/Adapter Framework | PROPOSED |
| — | Shared security/governance | VERIFIED_EXISTING primitives; integration policy PROPOSED |

---

## 4. Component inventory

### 4.1 HR/Payroll Core (layer 1)

| Component | Status | Evidence |
|---|---|---|
| Next.js web app | VERIFIED_EXISTING | `web/`, `web/app/api/gw/[...path]/route.ts` |
| Axum gateway, 186 routes | VERIFIED_EXISTING | `crates/api/gateway/src/router.rs`; `grep -c '\.route('` = 186 |
| Auth, refresh rotation, 2FA, OIDC/JWKS | VERIFIED_EXISTING | `crates/api/gateway/src/handlers/auth.rs`, `oidc.rs`; `crates/core/authz/src/jwt.rs` |
| RBAC 4-role matrix | VERIFIED_EXISTING | `crates/core/authz/src/rbac.rs` |
| Tenant RLS transaction pool | VERIFIED_EXISTING | `crates/infra/db/src/lib.rs` (`begin_tenant`, `with_tenant_tx`) |
| RLS policies, FORCE RLS | VERIFIED_EXISTING | `migrations/20261025000002_rls_policies.sql` |
| PII encryption | VERIFIED_EXISTING | `crates/infra/db/src/pii.rs` |
| HR services (employee, leave, OT, claims, loans, documents, onboarding, recruitment, training, performance, goals, privacy) | VERIFIED_EXISTING | `crates/services/talentos-hr-runner/src/*.rs` |
| Attendance domain | VERIFIED_EXISTING | `crates/domain/talentos-attendance/src/lib.rs`, `shift.rs` |
| Payroll deterministic DAG | VERIFIED_EXISTING | `crates/domain/talentos-payroll/src/dag.rs` ("Pure functions, deterministic, no side effects") |
| Payroll run/approve/payslip/PDF | VERIFIED_EXISTING | `crates/services/talentos-payroll-runner/`; routes `/api/v1/payroll/*` |
| Compliance (tax, SSO, PF, severance, OT) | VERIFIED_EXISTING | `crates/domain/talentos-compliance/src/*.rs` |
| Audit log | VERIFIED_EXISTING | `migrations/20261025000001_initial_schema.sql` (`audit_log`); `handlers/audit.rs` |
| GraphQL API | VERIFIED_EXISTING | `crates/api/graphql/`; `POST /api/v1/graphql` |
| Health and metrics | VERIFIED_EXISTING | `crates/api/gateway/src/handlers/health.rs`; `metrics.rs`; routes `/health`, `/metrics` |
| Notification delivery worker (retry, 3 attempts) | VERIFIED_EXISTING | `crates/api/gateway/src/notify_worker.rs` |
| PostgreSQL 16 + pgvector | VERIFIED_EXISTING | `docker-compose.yml`; container `loans-postgres-1` |
| Redis | VERIFIED_EXISTING | `crates/api/gateway/src/main.rs` requires `REDIS_URL`; health ping in `handlers/health.rs` |

### 4.2 HR Extension Platform (layer 2)

| Component | Status | Evidence |
|---|---|---|
| Extism WASM sandbox host | VERIFIED_EXISTING | `crates/ext/talentos-wasm-host/src/lib.rs` |
| Capability grants (tenant_context, audit_log, payslip_template) | VERIFIED_EXISTING | `crates/ext/talentos-wasm-host/src/capabilities.rs` |
| Plugin registry and invocation endpoints | VERIFIED_EXISTING | `crates/api/gateway/src/handlers/plugins.rs`; routes `/api/v1/plugins`, `/api/v1/plugins/:name/invoke` |
| `tenant_plugins` storage with RLS | VERIFIED_EXISTING | `migrations/20261028000005_tenant_plugins.sql` |
| No-code HR extension metadata contracts | PROPOSED | Baseline audit: "no metadata contracts" (`docs/architecture-realignment/00-baseline-audit.md` §2) |

### 4.3 AI Intelligence Platform (layer 3)

| Component | Status | Evidence |
|---|---|---|
| Model gateway (OpenAI-compatible client, cost) | VERIFIED_EXISTING | `crates/ai/talentos-model-gw/src/client.rs`, `cost.rs` |
| RAG chunking, embeddings, hybrid search | VERIFIED_EXISTING | `crates/ai/talentos-rag/src/*.rs` (no HTTP surface yet) |
| Agent runner Observe→Plan→Act→Reflect | VERIFIED_EXISTING | `crates/ai/talentos-agent/src/workflow.rs` (in-process) |
| Built-in tool registry | VERIFIED_EXISTING | `crates/ai/talentos-tools/src/registry.rs` |
| Memory layers and recall | VERIFIED_EXISTING | `crates/ai/talentos-memory/src/lib.rs` |
| Providers + five model roles | VERIFIED_EXISTING | `crates/api/gateway/src/handlers/ai_providers.rs`; `migrations/20261216000001_ai_providers.sql` |
| Knowledge graph (12 classes, sync, neighbours) | VERIFIED_EXISTING | `crates/services/talentos-hr-runner/src/kg.rs`, `kg_sync.rs`; `migrations/20261215000001_ai_kg.sql` |
| Grounded chat with citations | VERIFIED_EXISTING | `POST /api/v1/ai/chat` in `handlers/ai.rs` |
| MCP server over streamable HTTP (built-in tools) | VERIFIED_EXISTING | `crates/api/mcp/src/lib.rs`; route `/api/v1/mcp` |
| OpenBao secret client for provider keys | VERIFIED_EXISTING | `crates/api/gateway/src/secrets.rs` |
| OpenBao service, KV v2, auto-unseal | VERIFIED_EXISTING | `scripts/openbao-bootstrap.sh`; container `openbao` on `127.0.0.1:8200` |
| User-created MCP servers + tool sync | PROPOSED | Only `mcp_enabled` boolean exists (`handlers/ai.rs`); no `mcp_servers` table |
| User-created skills with versions | PROPOSED | Only JSON toggle list in `ai_settings`; no `ai_skills` table |
| User-created agents + default HR Assistant | PROPOSED | Single implicit chat path; no `ai_agents` table; `agents` JSONB is a list, not an object model |

### 4.4 Integration Control Plane (layer 4) — all PROPOSED

| Component | Status | Note |
|---|---|---|
| Control Plane backend/API | PROPOSED | No crate, no route, no migration |
| Embedded Integration Studio UI | PROPOSED | No page under `web/`; `docs/architecture-realignment/00-baseline-audit.md` marks "Integration Runtime / Studio" MISSING |
| Integration metadata store (connectors, mappings, pipelines, versions, schedules) | PROPOSED | No tables |
| Deterministic mapping engine | PROPOSED | No code |
| Connector credential broker | PROPOSED | OpenBao primitive exists; connector path/policy does not |
| Deploy/version/publish contract | PROPOSED | No code |

### 4.5 Independent Integration Runtime (layer 5) — all PROPOSED

| Component | Status | Note |
|---|---|---|
| Runtime worker service | PROPOSED | No crate or systemd unit |
| Job queue/scheduler | PROPOSED | Redis exists; no integration queue/stream |
| Webhook ingress | PROPOSED | No route; notification handlers are outbound-only |
| Integration outbox / dead-letter queue | PROPOSED | `notify_worker` is email/LINE only, not a general integration outbox |
| Runtime health, lease, heartbeat | PROPOSED | No code |
| Runtime observability (runs, retries, DLQ) | PROPOSED | No code |

### 4.6 Connector/Adapter Framework (layer 6) — all PROPOSED

| Component | Status | Note |
|---|---|---|
| Connector SDK / adapter trait | PROPOSED | No code |
| Sandboxed connector workers | PROPOSED | Extism host exists for tenant plugins, not connectors |
| Protocol adapters (REST, SFTP, CSV, bank files) | PROPOSED | No code |
| Per-connector egress policy | PROPOSED | No code |
| Connector certification/allowlist | PROPOSED | No code |

### 4.7 Shared security/governance (cross-cutting)

| Component | Status | Evidence |
|---|---|---|
| JWT auth middleware | VERIFIED_EXISTING | `crates/api/gateway/src/middleware/auth.rs` |
| RBAC enforcement helper | VERIFIED_EXISTING | `require(...)` used across handlers; matrix in `crates/core/authz/src/rbac.rs` |
| Tenant scoping / RLS | VERIFIED_EXISTING | `crates/infra/db/src/lib.rs`; `migrations/20261025000002_rls_policies.sql` |
| Audit trail | VERIFIED_EXISTING | `audit_log` table; `handlers/audit.rs` |
| Secret storage | VERIFIED_EXISTING | `crates/api/gateway/src/secrets.rs`; no Postgres key fallback |
| Integration policy (egress allowlist, per-tenant quotas, connector RBAC) | PROPOSED | No code |
| Runtime isolation/lease policy | PROPOSED | No code |

**Counts:** 38 VERIFIED_EXISTING components, 23 PROPOSED components (see
`01-ws1-summary.md` for the roll-up). Rows in §4.1–§4.7 are the counted unit;
the layer table in §3 is a summary and is not double-counted.

---

## 5. Flows: verified current, PROPOSED target

Current: `Browser → Next session proxy → gateway → auth/RBAC → tenant RLS →
Postgres`, with gateway fan-out to `hr-runner`, the payroll DAG, AI crates,
MCP/tools and the Extism host. Chat retrieves KG subgraphs and calls registered
tools; the payroll **calculation** never calls an LLM
(`crates/domain/talentos-payroll/src/dag.rs`). Baseline exception: the built-in
`run_payroll` tool inserts `payroll_runs` directly (`crates/ai/talentos-tools/src/tools/mod.rs`),
which the target contract in ADR-010 replaces with a domain-API call.

Target (PROPOSED): `Studio → Control Plane (metadata, validate, publish) →
Runtime (schedule/webhook, execute) → Connector worker (egress) → HR domain API
(only write path) → run log + DLQ`. The Runtime never holds write credentials
for HR Core; see `01-domain-boundaries.md`.

## 6. What this document does not claim

No Integration Runtime, Studio, connector SDK, mapping engine, integration queue,
user-authored MCP server, skill, agent object, or no-code HR extension metadata
model exists today. An AI provider key is still required before chat works; at
baseline it is unset (`handoff.md` §8.4).

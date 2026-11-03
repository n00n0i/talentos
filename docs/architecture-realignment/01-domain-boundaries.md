# 01 — Domain Boundaries and Ownership (WS1)

**Status:** target contract. Existing items cite code; proposed items are labelled
**PROPOSED**. This document defines who owns which data and which calls are legal.

---

## 1. Domain ownership matrix

| Domain | Owns (system of record) | May read | May write | Evidence |
|---|---|---|---|---|
| **HR Core** | `tenants`, `entities`, `users`, `roles`, `user_roles`, `employees`, `audit_log`, attendance/leave/OT/claims/loans/documents/onboarding/recruitment/training/performance/goals tables | its own tenant rows | its own tables only, through `talentos-hr-runner` | `migrations/20261025000001_initial_schema.sql`; `crates/services/talentos-hr-runner/src/*.rs` |
| **Payroll Core** | `payroll_runs`, `payslips`, `payslip_lines`, `payroll_events`, bank/payout artifacts | employee, attendance, leave, OT, claim, loan inputs for the run | payroll tables only, through `talentos-payroll-runner` | `migrations/20261026000001_payroll_core.sql`; `crates/domain/talentos-payroll/src/dag.rs`; `crates/services/talentos-payroll-runner/` |
| **HR Extension Platform** | `tenant_plugins` (WASM bytes, capability grants, enabled flag) | caller tenant/entity/user via injected `TenantContextPayload`; capability-gated payslip template | audit entries only if `audit_log` capability granted; never HR business tables | `migrations/20261028000005_tenant_plugins.sql`; `crates/ext/talentos-wasm-host/src/capabilities.rs`, `lib.rs` |
| **AI Intelligence Platform** | `ai_providers`, `ai_model_roles`, `ai_settings`, `ai_conversations`, `ai_agent_runs`, `ai_tool_calls`, `ai_cost_tracking`, `ai_documents`, `ai_document_chunks`, `ai_memory`, `kg_nodes`, `kg_edges` | HR data through tenant-scoped queries and the tool registry; KG is derived | AI tables. **Target:** no HR/payroll tables. **Baseline exception:** `RunPayrollTool` inserts `payroll_runs` directly (`crates/ai/talentos-tools/src/tools/mod.rs`), to be replaced by a domain API call | `migrations/20261027000001_ai_core.sql`, `20261027000002_ai_embeddings.sql`, `20261215000001_ai_kg.sql`, `20261216000001_ai_providers.sql`; `crates/ai/*` |
| **Integration Control Plane (PROPOSED)** | PROPOSED: connector definitions, mapping definitions, pipeline versions, publish state, schedules, tenant integration policy | HR schema catalog and domain API metadata, read-only | its own metadata store only | no crate, no route, no table today |
| **Independent Integration Runtime (PROPOSED)** | PROPOSED: run state, attempt state, delivery state, dead-letter state, runtime lease | immutable published pipeline versions; HR domain API responses | **no HR Core table, ever**; only its own run/attempt store | no crate, no process today |
| **Connector/Adapter Framework (PROPOSED)** | PROPOSED: adapter code, per-connector config, egress policy | runtime job payload | external system + runtime result only | no code today |
| **Shared security/governance** | authn/authz, RLS, audit, secret paths | identity and policy metadata | auth/audit only | `crates/core/authz/src/rbac.rs`; `crates/infra/db/src/lib.rs`; `crates/api/gateway/src/secrets.rs` |

**Rule:** a domain owns writes to its tables. Another domain may hold a derived
copy only if it is explicitly marked derived and rebuildable (the KG is the
existing example: `crates/services/talentos-hr-runner/src/kg_sync.rs`).

---

## 2. The hard rule for the Integration Runtime

> **The Integration Runtime must never write HR Core tables.**
> Every HR mutation goes through a TalentOS domain API under tenant auth, RBAC
> and RLS.

Implications:

| Allowed | Forbidden |
|---|---|
| Call an HR domain API with a tenant-scoped service identity | `INSERT/UPDATE/DELETE` on `employees`, `payroll_runs`, `payslips`, `attendance*`, `leave*`, `claims`, `loans` |
| Read HR data through the same API, or a read-only replica/view explicitly granted | Connecting with a `BYPASSRLS` role, superuser, or table owner |
| Keep run/attempt/DLQ state in its own store | Writing `audit_log` directly; audit is emitted by the domain API |
| Publish status back to the Control Plane | Running schema migrations or DDL |
| Retry idempotently using a domain-provided idempotency key | Inferring an id and writing a “shadow” HR row |

This is a **contract**, not a current implementation: no runtime exists yet.
Enforcement must be by database grants (separate role), not convention
(PROPOSED).

---

## 3. Service dependency matrix

Rows call columns. `✔` = allowed; `—` = forbidden; `R` = read-only.

| Caller ↓ / Callee → | Web | Gateway | HR runner | Payroll | AI crates | Extism host | Control Plane | Runtime | Connector | Postgres | OpenBao |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Web | — | ✔ | — | — | — | — | — | — | — | — | — |
| Gateway | — | — | ✔ | ✔ | ✔ | ✔ | ✔ (PROPOSED) | ✔ deploy/status (PROPOSED) | — | ✔ via tenant tx | ✔ |
| HR runner | — | — | — | — | — | — | — | — | — | ✔ own tables | — |
| Payroll | — | — | — | — | — | — | — | — | — | ✔ payroll tables | — |
| AI crates | — | — | R via tenant queries; gated payroll write is a baseline exception | — | ✔ | — | — | — | — | R tenant-scoped | ✔ via gateway client |
| Extism host | — | — | — | — | — | — | — | — | — | — | — |
| Control Plane (PROPOSED) | — | — | R via API | R via API | — | — | ✔ own store | ✔ publish | — | own metadata store only | ✔ connector refs |
| Runtime (PROPOSED) | — | — | ✔ via API | ✔ via API | — | — | ✔ status | ✔ own store | ✔ dispatch | own run store only | ✔ runtime secrets |
| Connector (PROPOSED) | — | — | — | — | — | — | — | ✔ result | ✔ | — | ✔ scoped secret |

### Allowed call directions

1. **Web → Gateway** only. The browser never reaches Postgres, OpenBao, AI
   crates or the runtime (`web/app/api/gw/[...path]/route.ts` proxies to
   `TALENTOS_API_URL`).
2. **Gateway → domain services/crates**. The gateway is the composition root
   (`crates/api/gateway/Cargo.toml` depends on hr-runner, payroll, AI, MCP,
   GraphQL, wasm host).
3. **Domain → infra primitives** (`talentos-db`, `talentos-kernel`,
   `talentos-authz`). Domains never depend on the gateway.
4. **AI → tools/RAG/memory/model-gw**, and read-only HR queries through
   tenant-scoped functions. AI never runs payroll arithmetic.
5. **Control Plane → Runtime** is a publish/status contract only (PROPOSED).
6. **Runtime → HR domain API** is the only HR mutation path (PROPOSED).

### Forbidden directions

- Runtime/Connector → HR tables by direct SQL.
- Any component → `BYPASSRLS` connection except the documented login bootstrap
  (`TenantPool::acquire_raw`, `crates/infra/db/src/lib.rs`).
- Extension/WASM → filesystem, network, environment (Extism denies all;
  `crates/ext/talentos-wasm-host/src/lib.rs`).
- AI → `payroll_runs`/`payslips` writes in the target contract; payroll
  calculation remains deterministic and LLM-free. (The current `run_payroll`
  tool is a documented baseline exception, ADR-010.)
- Gateway → another tenant's context: every query starts from
  `Authed(ctx)` + `begin_tenant`/`with_tenant_tx`.

---

## 4. TalentOS HR Core protection contracts

What extensions, agents, and integrations **may** and **may not** call.

| Contract | May | May not | Enforcement today |
|---|---|---|---|
| **C1 Read** | Tenant-scoped REST/GraphQL reads allowed by RBAC | Cross-tenant reads; raw SQL; schema inspection beyond published metadata | JWT + `require()` + RLS (`middleware/auth.rs`, `rbac.rs`, `20261025000002_rls_policies.sql`) |
| **C2 Write** | Domain API writes with a valid tenant context and permission | Direct table writes; DDL; migrations; `BYPASSRLS` | Handlers call `hr_runner`/`payroll_runner`; no runtime exists |
| **C3 Payroll** | Read status/results through the domain API | Recompute, mutate, approve, or export payroll outside `payroll_run.calculate/approve` permissions. Baseline exception: `RunPayrollTool` writes `payroll_runs` directly, gated by `allow_destructive`; target contract removes this | `rbac.rs` entries; `crates/services/talentos-payroll-runner/`; `crates/ai/talentos-tools/src/tools/mod.rs` |
| **C4 AI** | Retrieve KG, call allowlisted tools, answer with citations | Bypass tool allowlists; run destructive tools without a gate; receive raw provider keys | `handlers/ai.rs`, `talentos-tools/src/registry.rs`, `secrets.rs` (keys never returned) |
| **C5 Extension** | Use only granted capabilities (`tenant_context`, `audit_log`, `payslip_template`) | Network, filesystem, env, arbitrary host calls | `capabilities.rs`; Extism sandbox config |
| **C6 Integration (PROPOSED)** | Call documented HR domain APIs; hold its own run state | Write HR tables; share a DB role with the gateway; exceed tenant quotas | **not implemented**; requires separate DB role + grant review |
| **C7 Audit** | Emit audit through the domain layer | Suppress or forge audit rows | `audit_log` table; domain handlers write entries |
| **C8 Secrets** | Read its own tenant-scoped OpenBao path | Read another tenant's path; persist secrets in Postgres | `secrets.rs` path `secret/data/talentos/tenants/<tenant>/providers/<provider>` |
| **C9 Data residency/retention** | Respect PDPA retention and purge rules | Copy PII into connector logs or LLM prompts beyond policy | `handlers/privacy.rs`; `talentos-hr-runner/src/privacy.rs`, `pdpa.rs` |

Contract C6 is the gating item for WS2+: it must be enforced by a dedicated
database role with no write grants on HR tables, and by an API-only service
identity. Until then, no runtime deployment is authorized.

---

## 5. Boundary tests required (PROPOSED)

These tests do not exist yet; they are acceptance criteria for WS2.

1. Runtime role attempting `INSERT INTO employees` fails with `permission denied`.
2. Runtime role with `SET app.current_tenant_id` to another tenant reads zero rows.
3. A connector cannot resolve another tenant's OpenBao path.
4. A disabled connector/pipeline cannot be triggered by webhook replay.
5. Payroll output is byte-identical with AI disabled and enabled.
6. Every runtime-originated HR mutation carries an audit row and an idempotency key.

**Verdict:** ownership is clear for existing domains; the integration boundary
is specified but **not built or enforced**.

# AI Settings + Organizational Knowledge Graph — Design

- **Date:** 2026-11-02
- **Status:** awaiting approval
- **Spec authority:** `docs/design/phase-4-spec.md` (AI orchestrator) — this doc fills the two largest gaps: no `/api/v1/ai/chat`, no settings surface, and no organization model for the agent.
- **Scope:** one new migration, one new service crate module, gateway endpoints, an OpenBao secret backend, one settings page, one knowledge-graph page, the chat widget, tests.

## 1. Why

Phase 4 promised an AI layer that "automates HR workflows end-to-end". What exists today:

| Spec'd | Built |
|---|---|
| `POST /api/v1/ai/chat` (P0) | ❌ |
| `/ai/agents/:id/status|approve`, `/ai/documents`, `/ai/documents/search`, `/ai/cost` | ❌ |
| `ai.chat` permission | ❌ (endpoints guard on `employee.read_all`) |
| Tables `ai_conversations`, `ai_agent_runs`, `ai_tool_calls`, `ai_cost_tracking`, `tenant_api_keys` | ✅ created, **never read/written** |
| `HuakatiClient::chat()` | ✅ |
| Tools registry (`check_compliance`, `calculate_tax`, …) | ✅ |
| Organization model the agent can reason over | ❌ (memory metadata only) |

Two consequences: (a) an admin cannot configure a provider/model at all, and (b) even with a provider, the agent knows *tools* but not *the organization* — no departments, positions, policies, or how they relate.

## 2. Goals

1. **AI settings** page where an admin sets provider, model, key, MCP exposure, agent skills, and sees knowledge-graph status — all tenant-scoped.
2. **Organizational knowledge graph (KG)** with an explicit ontology + taxonomy, populated from existing HR data, queryable by the agent.
3. **Chat** endpoint + widget that answers with the KG as context and cites what it used.
4. Nothing leaks across tenants: every read/write is RLS-scoped like the rest of the app.
5. Keys are encrypted at rest and never returned by the API.

## 3. Non-goals (v1)

- No general graph database. Postgres tables + recursive CTEs + pgvector are enough for tenant-scale orgs (< 50k nodes).
- No automatic ontology learning from text. The ontology is authored; data populates it.
- No fine-tuning, no agent marketplace, no per-user API keys.
- No cross-tenant benchmarking graph.
- Not building a Temporal workflow for KG sync — a synchronous rebuild endpoint with a row cap.

## 4. Ontology (classes)

12 classes, bilingual labels. Each node = one real thing in the tenant.

| Class | Thai | Source | Key properties |
|---|---|---|---|
| `Organization` | องค์กร | tenant | name, tax_id |
| `LegalEntity` | นิติบุคคล/สาขา | `entities` | name, tax_id |
| `Department` | แผนก | `employees.department` (distinct) | name |
| `Position` | ตำแหน่ง | `employees.position_title` (distinct) | title, level |
| `Employee` | พนักงาน | `employees` | code, status, hire_date |
| `Skill` | ทักษะ | manual + `training_records` | name, category |
| `Policy` | นโยบาย | manual/forms | kind (leave/payroll/ot), effective_from |
| `Benefit` | สวัสดิการ | manual | kind, amount/rate |
| `PayrollItem` | รายการเงินเดือน | `payroll_constants` + forms | code, kind (earning/deduction) |
| `Cycle` | รอบ/งวด | `payroll_cycles` | period, status |
| `Document` | เอกสาร | `documents` | doc_type, expiry |
| `Process` | กระบวนการ | authored | name, step order |

**Rules the agent can rely on:** an `Employee` *belongs to* exactly one `Department` and one `Position`, under one `LegalEntity`, inside the `Organization`. A `Policy` *applies to* a scope (org / entity / department / position). A `PayrollItem` *derives from* a `Policy` or `Benefit`.

## 5. Taxonomy (relations / edges)

Directed edges, each with a Thai label:

```
Organization  --has_entity-->      LegalEntity
LegalEntity   --has_department-->  Department
Department    --has_position-->    Position
Position      --held_by-->         Employee        (inverse: Employee --holds--> Position)
Employee      --member_of-->       Department      (denormalised for fast traversal)
Employee      --has_skill-->       Skill
Policy        --applies_to-->      Organization | LegalEntity | Department | Position
Benefit       --granted_by-->      Policy
PayrollItem   --derived_from-->    Policy | Benefit
Cycle         --contains-->        PayrollItem
Document      --belongs_to-->      Employee | Department
Process       --step_of-->         Process
```

Hierarchies v1: `Organization > LegalEntity > Department > Position > Employee`; `Skill > SkillCategory`; `Policy > PolicyKind`. (These are the "taxonomy" — typed containment, not free-form links.)

## 6. Storage

One migration `20261102000001_ai_kg.sql`:

```sql
CREATE TABLE kg_nodes (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  class TEXT NOT NULL,                 -- ontology class above
  key TEXT NOT NULL,                   -- stable business key (employee_code, dept name…)
  label_th TEXT NOT NULL,
  label_en TEXT,
  properties JSONB NOT NULL DEFAULT '{}',
  embedding vector(1536),
  source TEXT NOT NULL DEFAULT 'sync', -- sync | manual
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, class, key)
);

CREATE TABLE kg_edges (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  from_id UUID NOT NULL REFERENCES kg_nodes(id) ON DELETE CASCADE,
  to_id   UUID NOT NULL REFERENCES kg_nodes(id) ON DELETE CASCADE,
  relation TEXT NOT NULL,
  properties JSONB NOT NULL DEFAULT '{}',
  UNIQUE (tenant_id, from_id, relation, to_id)
);

CREATE TABLE ai_settings (
  tenant_id UUID PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
  provider TEXT NOT NULL DEFAULT 'huakati',
  base_url TEXT,
  model TEXT NOT NULL DEFAULT 'qwen2.5-72b-instruct',
  api_key_enc BYTEA,                   -- AES-GCM, key from APP_ENCRYPTION_KEY
  mcp_enabled BOOLEAN NOT NULL DEFAULT true,
  skills JSONB NOT NULL DEFAULT '[]',  -- [{id, enabled, allowed_tools[]}]
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

Indexes: `(tenant_id, class)`, `(tenant_id, relation)`, GIN on `properties`, ivfflat on `embedding`.

`tenant_api_keys` from phase 4 stays unused; `ai_settings` supersedes it (it also holds model/base_url/skills). Record that as a ruling in the migration comment.

## 7. Sync (populating the graph)

`POST /api/v1/ai/kg/rebuild` (admin): one transaction, capped at 5,000 nodes.

1. Upsert `Organization`, entities, departments (distinct from employees), positions, employees.
2. Upsert edges for the hierarchy.
3. Upsert `Policy`/`Benefit`/`PayrollItem` from authored seed data + forms.
4. Embed `label_th + key + class` for each node via `model-gw` (batched, best-effort: a missing key leaves `embedding NULL`, graph still works).
5. Return counts per class.

Re-running is idempotent (`UNIQUE (tenant_id, class, key)`).

## 8. Agent use

`POST /api/v1/ai/chat` `{ message, agent? }`:

1. Resolve settings (provider/model/key/decrypt) or 503 `ai_unavailable`.
2. Extract candidate entities from the message (exact/substring match on node labels) → `kg_subgraph(node, depth=2)`.
3. Build system prompt: ontology summary + the retrieved subgraph as triples + available tools + Thai-HR instruction.
4. Call `model_gw.chat()`; persist `ai_conversations` + `ai_cost_tracking`.
5. Return `{ reply, citations: [{class, label, relation}], usage }`.

New agent tools (registered in `talentos-tools`): `kg_search(query, class?)`, `kg_neighbors(node_key, relation?)`.

## 9. API surface

| Endpoint | Method | Permission | Notes |
|---|---|---|---|
| `/api/v1/ai/settings` | GET/PUT | `ai.settings.manage` (hr_admin) | key never returned; `api_key_set: bool`; secrets in OpenBao |
| `/api/v1/ai/settings/test` | POST | same | one cheap completion; returns ok/latency/model |
| `/api/v1/ai/chat` | POST | `ai.chat` (hr_admin, payroll_manager, manager) | the widget's endpoint |
| `/api/v1/ai/kg` | GET | `ai.chat` | counts by class + last rebuild |
| `/api/v1/ai/kg/rebuild` | POST | `ai.settings.manage` | idempotent sync |
| `/api/v1/ai/kg/neighbors` | GET | `ai.chat` | graph inspector for the UI |

RBAC additions: `ai.chat`, `ai.settings.manage` in `crates/core/authz/src/rbac.rs`.

## 10. UI

**Settings → AI** (`/settings/ai`, admin only): provider select, base URL, model, API key (write-only, shows "ตั้งค่าแล้ว"), *ทดสอบการเชื่อมต่อ* button with result, MCP toggle, skills list with toggles, KG status card (node/edge counts + *สร้างกราฟใหม่*).

**Knowledge graph** (`/settings/ai/graph`): class counts, search box, node list, and a neighbours panel showing subject → relation → object. Read-only v1; manual node creation is a follow-up.

**Chat head**: floating button bottom-right on every app page; panel with messages, citations rendered as chips, and states: `ai_unavailable` (ยังไม่ได้ตั้งค่า — link to settings), 403 (no permission), empty.

## 11. Security

- API key: AES-GCM with `APP_ENCRYPTION_KEY`; never logged, never returned; only `api_key_set` boolean.
- All KG/settings queries run inside `begin_tenant` (RLS), same as the rest.
- Chat runs the existing agent guard: destructive tools still need `allow_destructive`.
- Audit: `ai.settings.update`, `ai.kg.rebuild`, `ai.chat` events into `audit_log`.

## 12. Testing

- Rust: settings CRUD (key never echoed), KB rebuild idempotency, subgraph traversal depth, chat 503 without key, chat 200 with a mock provider, RBAC 403s.
- vitest: prompt builder (ontology + subgraph → text), citation mapper.
- E2E: settings page saves without echoing the key; graph page shows counts; chat head opens, shows the "not configured" state when key absent, and answers when present (skipped if no key at runtime).

## 13. Decisions (answered)

1. **Encryption key:** new `APP_ENCRYPTION_KEY` (AES-256-GCM) for anything that must be encrypted in the database.
2. **KG scope:** all 12 classes.
3. **Provider secrets:** stored in **OpenBao** (KV v2), not in Postgres, with **auto-unseal** so the service starts unattended.

## 14. OpenBao integration

OpenBao 2.7.1 CLI and the `openbao/openbao:2.6.0` image are already present on this host; no OpenBao instance is running yet.

### 14.1 Storage layout

```
secret/data/talentos/tenants/<tenant_id>/ai
  { provider, base_url, model, api_key }
```

- One KV entry per tenant; `ai_settings` keeps only non-secret fields (provider, base_url, model, mcp_enabled, skills) plus `secret_ref` and `api_key_set`.
- The gateway reads the entry at request time through a 60-second in-process cache; writes go through the settings endpoint (PUT), never from the browser directly.
- Removing a key = delete the KV entry + set `api_key_set = false`.

### 14.2 Auto-unseal

OpenBao starts sealed and must unseal before serving secrets. `seal "static"` gives unattended start from a key file:

```hcl
# /etc/openbao/openbao.hcl
storage "file" { path = "/openbao/data" }
listener "tcp" { address = "0.0.0.0:8200" tls_disable = 1 }
seal "static" {
  current_key_id = "2026-11"
  current_key    = "file:///openbao/seal.key"   # 32-byte hex, 0600
}
api_addr = "http://127.0.0.1:8200"
ui = false
```

- `seal.key` lives on the host (mode 0600) and is injected as a file, not an env var.
- Production can swap `seal "static"` for `seal "awskms" | "gcpckms" | "transit"` with no application change — the gateway only talks to the OpenBao API.
- Losing `seal.key` loses the stored provider keys (they must be re-entered). Document this in the runbook; it is the standard static-seal trade-off.

### 14.3 Gateway configuration

| Env | Meaning |
|---|---|
| `OPENBAO_ADDR` | e.g. `http://127.0.0.1:8200` |
| `OPENBAO_TOKEN` | token with `read`/`create`/`update` on `secret/data/talentos/*` |
| `OPENBAO_ENABLED` | `true` in deployment; unset in tests |

When `OPENBAO_ENABLED` is unset the settings endpoint reports `secrets_backend: "none"` and chat returns `503 ai_unavailable` — never falls back to writing keys to Postgres.

### 14.4 Bootstrap

`scripts/openbao-bootstrap.sh`: start the container with the config above, wait for unsealed status, enable `kv-v2` at `secret/`, write a scoped policy + token, and print the values to export. Idempotent; safe to re-run.

## 15. Deferred (explicitly out of v1)

- Rotating provider keys automatically.
- Per-user OpenBao identities (AppRole per tenant is a follow-up; v1 uses one scoped token).
- Graph visualisation (the v1 page is a list + neighbours panel).
- RAG over documents feeding the graph (documents are nodes only).

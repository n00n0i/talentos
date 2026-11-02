# AI Settings + Organizational Knowledge Graph — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Ship the Phase-4 AI surface: tenant AI settings backed by OpenBao, an organizational knowledge graph (12 classes), a chat endpoint grounded in that graph, and the UI for all three.

**Architecture:** New Postgres tables for graph + non-secret settings; provider secrets in OpenBao KV v2 (static auto-unseal); gateway endpoints under `/api/v1/ai/*` guarded by new RBAC permissions; Next.js settings pages and a floating chat widget.

**Tech Stack:** Rust (axum, sqlx, reqwest), Postgres 16 + pgvector, OpenBao 2.x, Next.js 15, Playwright, vitest.

**Spec:** `docs/superpowers/specs/2026-11-02-ai-settings-knowledge-graph-design.md`

## Global Constraints

- No new Rust or npm dependencies beyond what the workspace already has (`reqwest` and `chrono` are present; pgvector is already used by `talentos-rag`).
- Every DB access goes through `begin_tenant` (RLS). No raw pool queries in new code.
- Provider secrets never touch Postgres, logs, or API responses; only `api_key_set: bool` is returned.
- New RBAC permissions: `ai.chat` (hr_admin, payroll_manager, manager), `ai.settings.manage` (hr_admin).
- `cargo test -p talentos-gateway` and `web` `npx tsc --noEmit`, `npx vitest run`, Playwright 57/57 stay green.
- Existing route URLs unchanged.

## Review Focus

- **OpenBao unreachable** — settings save must not silently write a plaintext key anywhere; chat must return `503 ai_unavailable`.
- **Tenant isolation** — one tenant's graph or provider key must never appear for another; every new query is RLS-scoped.
- **Key never echoed** — `GET /ai/settings` returns `api_key_set` only, including on error paths.
- **Rebuild idempotency** — running `/ai/kg/rebuild` twice produces the same node/edge counts.
- **Chat without a key** — widget shows the settings link, not a stack trace or a spinner forever.

---

### Task 1: Schema + KG primitives

**Files:** `migrations/20261102000001_ai_kg.sql`, `crates/services/talentos-hr-runner/src/kg.rs`, `crates/services/talentos-hr-runner/src/lib.rs`, `crates/services/talentos-hr-runner/tests/kg.rs`

**Interfaces produced:** `pub enum NodeClass`, `pub struct KgNode`, `pub struct KgEdge`, `pub async fn upsert_node(pool, tenant, node) -> Uuid`, `pub async fn upsert_edge(...)`, `pub async fn neighbors(pool, tenant, node_id, depth) -> (Vec<KgNode>, Vec<KgEdge>)`, `pub async fn counts(pool, tenant) -> BTreeMap<NodeClass, i64>`.

- [ ] Migration: `kg_nodes`, `kg_edges`, `ai_settings` exactly as spec §6, plus indexes.
- [ ] Failing test: upsert the same (class,key) twice → one row; neighbors(depth=2) returns the grandparent; counts match inserted rows.
- [ ] Implement; run `cargo test -p talentos-hr-runner kg`.
- [ ] Commit `feat(kg): schema and graph primitives`.

---

### Task 2: OpenBao secret backend

**Files:** `crates/api/gateway/src/secrets.rs` (new), `crates/api/gateway/src/main.rs`, `scripts/openbao-bootstrap.sh`

**Interfaces produced:** `pub struct SecretsClient`; `pub async fn read_ai_secret(&self, tenant: Uuid) -> Result<AiSecret, SecretError>`; `pub async fn write_ai_secret(&self, tenant: Uuid, s: &AiSecret) -> Result<(), SecretError>`; `AiSecret { provider, base_url, model, api_key }`.

- [ ] Failing test (unit, no network): path builder returns `secret/data/talentos/tenants/<id>/ai`; `OPENBAO_ENABLED` unset → `SecretsClient::from_env()` returns `None` and callers must treat it as unconfigured.
- [ ] Implement with `reqwest`, 60s cache, `X-Vault-Token` header; never log the token or the key.
- [ ] `scripts/openbao-bootstrap.sh`: write config with `seal "static"`, start container, wait unsealed, enable kv-v2, apply scoped policy, print env exports. Idempotent.
- [ ] Run `cargo test -p talentos-gateway secrets` and the bootstrap script twice in a row.
- [ ] Commit `feat(ai): OpenBao KV secret backend with auto-unseal`.

---

### Task 3: AI settings API + RBAC

**Files:** `crates/api/gateway/src/handlers/ai.rs`, `dto/requests.rs`, `router.rs`, `crates/core/authz/src/rbac.rs`

**Interfaces produced:** `GET /api/v1/ai/settings` → `{provider, base_url, model, mcp_enabled, skills, api_key_set, secrets_backend}`; `PUT` accepts optional `api_key` (write-only); `POST /api/v1/ai/settings/test` → `{ok, latency_ms, model}`.

- [ ] Add `ai.chat`, `ai.settings.manage` to RBAC with the role lists above; failing test asserts employee gets 403 and hr_admin 200.
- [ ] Failing test: PUT with a key returns `api_key_set: true` and the echoed body contains no key; GET never contains the key; secrets backend `none` → PUT returns 503 and writes nothing.
- [ ] Implement, using Task 2 for the secret and `ai_settings` for the rest.
- [ ] Commit `feat(ai): tenant AI settings API`.

---

### Task 4: KG sync + endpoints

**Files:** `crates/services/talentos-hr-runner/src/kg_sync.rs`, `handlers/ai.rs`, `router.rs`

**Interfaces produced:** `POST /api/v1/ai/kg/rebuild` → `{nodes: {class: n}, edges: n}`; `GET /api/v1/ai/kg` → counts + `last_rebuilt_at`; `GET /api/v1/ai/kg/neighbors?node_id=&depth=`.

- [ ] Failing test: empty tenant → rebuild produces org + entity nodes only; two employees in one department → 2 Employee nodes, 1 Department, 2 `member_of` edges; second rebuild leaves counts unchanged.
- [ ] Implement the 12-class sync per spec §7 (cap 5,000 nodes); embeddings optional and best-effort.
- [ ] Commit `feat(kg): idempotent org graph sync and endpoints`.

---

### Task 5: Chat endpoint grounded in the graph

**Files:** `crates/api/gateway/src/handlers/ai.rs`, `crates/ai/talentos-tools/src/tools/mod.rs` (kg tools), `test/chat-prompt.test.ts`

**Interfaces produced:** `POST /api/v1/ai/chat` `{message, agent?}` → `{reply, citations[], usage}`; tools `kg_search`, `kg_neighbors`.

- [ ] Failing Rust test: no OpenBao/secret → 503 `ai_unavailable`; hr_admin with a mock client → 200 with `reply` non-empty.
- [ ] Failing vitest for the prompt builder: given 3 triples, the system prompt contains the ontology header and every triple once.
- [ ] Implement retrieval (label match → `neighbors(depth=2)`), prompt, `model_gw.chat`, persist `ai_conversations` + `ai_cost_tracking`.
- [ ] Commit `feat(ai): chat endpoint with knowledge-graph grounding`.

---

### Task 6: Settings UI

**Files:** `web/app/(app)/settings/ai/page.tsx`, `web/lib/i18n-dicts.ts`, `web/app/globals.css`

- [ ] Failing E2E: `/settings/ai` shows provider/model fields, "ตั้งค่าแล้ว"/"ยังไม่ได้ตั้งค่า" for the key, and a working "ทดสอบการเชื่อมต่อ" that renders the returned latency.
- [ ] Implement with `gw()`; key field is `type=password`, cleared after save, never prefilled.
- [ ] Commit `feat(web): AI settings page`.

---

### Task 7: Knowledge-graph UI

**Files:** `web/app/(app)/settings/ai/graph/page.tsx`

- [ ] Failing E2E: seed 2 employees via the existing helpers → rebuild from the page → class counts render → clicking a node shows its neighbours as `subject → relation → object`.
- [ ] Implement read-only v1 per spec §10.
- [ ] Commit `feat(web): knowledge-graph inspector`.

---

### Task 8: Chat widget on every page

**Files:** `web/components/ChatHead.tsx`, `web/app/(app)/layout.tsx`, `web/lib/i18n-dicts.ts`, `web/app/globals.css`

- [ ] Failing E2E: launcher visible on `/` and `/employees`; opening with no configured key shows the settings link; with a key (env-gated) sends a message and renders the reply.
- [ ] Implement; mount inside the app layout so it appears on every page; Esc closes (same pattern as the sidebar drawer).
- [ ] Commit `feat(web): chat head`.

---

## Verification (after every task)

```bash
cd crates && cargo test -p talentos-gateway -p talentos-hr-runner
cd web && npx tsc --noEmit && npx vitest run && npm run build
docker exec loans-postgres-1 psql -U talentos -d talentos -c "DELETE FROM tenants;"
E2E_BASE_URL=http://127.0.0.1:3000 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test
# then re-seed the demo tenant (E2E wipes it)
```

## Open item for the builder

The OpenBao token and seal key are host-specific. Task 2 ships a bootstrap script; the implementer must not invent a token in code — read `OPENBAO_TOKEN` from env and fail closed when absent.

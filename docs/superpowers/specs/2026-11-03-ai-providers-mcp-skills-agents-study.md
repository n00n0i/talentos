# AI Providers, MCP, Skills, Agents — Design Study

- **Date:** 2026-11-03
- **Status:** study + design, awaiting approval before implementation
- **Builds on:** `2026-11-03-ai-provider-model-roles-design.md` (providers + model roles), `2026-11-02-ai-settings-knowledge-graph-design.md` (KG, OpenBao)

## 1. What is wrong with the current cut

Today a tenant can add providers and pick five model roles, but:

- provider presets assume every vendor speaks OpenAI `/models` with a bearer token;
- MCP is a single boolean, not a set of servers a user adds;
- "skills" is a hardcoded list of tool names, not something a user creates;
- agents do not exist as objects — the chat handler is one implicit agent.

The user's ask: **providers, MCP servers, skills and agents should all be creatable by the user, then composed.**

## 2. Providers — what actually differs

| Concern | Reality | Design response |
|---|---|---|
| Auth | bearer (`OpenAI`, `OpenRouter`, `Groq`, `Together`, `DeepSeek`, `xAI`, `Mistral`), `x-api-key` (`Anthropic`), query param (`Google`), none (`Ollama`, `vLLM`, `LM Studio`) | `auth_style` column: `bearer` \| `x-api-key` \| `query` \| `none` |
| Catalog | OpenAI-compatible `GET /models`; Anthropic `GET /v1/models` (header auth); Google `GET /v1beta/models`; Ollama `GET /api/tags` (not `/v1`); vLLM `/v1/models` | `catalog_style`: `openai` \| `anthropic` \| `google` \| `ollama`; one normaliser per style |
| Protocol | ChatGPT-style `/chat/completions` everywhere except native Anthropic (`/v1/messages`) and Google (`:generateContent`) | v1 supports `openai` chat only; native styles are catalog-only + marked "not chat-capable" |
| Capability | Only the vendor knows; model ids hint (`-embed`, `-vision`, `whisper`, `tts`) | infer from id + optional user override stored per model |
| Limits | rate limits, context length, price differ per model | cache `context_window`, `max_output`, input/output price when the catalog exposes it; otherwise user-editable |

**Provider row (extends §2 of the earlier design):**

```sql
ALTER TABLE ai_providers
  ADD COLUMN auth_style    TEXT NOT NULL DEFAULT 'bearer'
    CHECK (auth_style IN ('bearer','x-api-key','query','none')),
  ADD COLUMN catalog_style TEXT NOT NULL DEFAULT 'openai'
    CHECK (catalog_style IN ('openai','anthropic','google','ollama')),
  ADD COLUMN chat_style    TEXT NOT NULL DEFAULT 'openai'
    CHECK (chat_style IN ('openai','anthropic','google')),
  ADD COLUMN headers       JSONB NOT NULL DEFAULT '{}',   -- extra static headers
  ADD COLUMN health        JSONB NOT NULL DEFAULT '{}';   -- last test result

ALTER TABLE ai_providers ALTER COLUMN models_cache TYPE JSONB;
-- cached model entry: {id, owned_by, context_window?, max_output?,
--                      capabilities: {chat, embedding, vision, ocr, tools},
--                      pricing?, source: "catalog"|"manual"}
```

Provider presets become data, not code: a seeded table `ai_provider_presets`
(kind, label, base_url, auth_style, catalog_style, chat_style, docs_url).

## 3. MCP — what it is in practice

MCP is JSON-RPC with three discovery methods: `tools/list`, `resources/list`,
`prompts/list`, plus transports:

| Transport | Use | Notes |
|---|---|---|
| `stdio` | local processes (`npx @modelcontextprotocol/server-filesystem`) | server-side only; we run it in the gateway host, not the browser |
| `sse` (legacy) | HTTP event stream | still common |
| `streamable-http` | current HTTP transport | preferred for remote |
| `ws` | rare | out of scope v1 |

**User-creatable model:**

```sql
CREATE TABLE mcp_servers (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  transport TEXT NOT NULL CHECK (transport IN ('stdio','sse','streamable-http')),
  endpoint TEXT,                 -- URL for http transports
  command TEXT,                  -- argv for stdio
  env_ref TEXT,                  -- OpenBao path for env/secrets
  enabled BOOLEAN NOT NULL DEFAULT true,
  last_sync_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, name)
);

CREATE TABLE mcp_tools (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  server_id UUID NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
  name TEXT NOT NULL,            -- raw tool name from the server
  title TEXT,
  description TEXT,
  input_schema JSONB NOT NULL,
  enabled BOOLEAN NOT NULL DEFAULT true,
  UNIQUE (tenant_id, server_id, name)
);
```

- "เพิ่มเซิร์ฟเวอร์" → transport + endpoint/command → **ซิงก์เครื่องมือ** (`tools/list`) → user toggles which tools are allowed.
- Tools are namespaced `mcp:<server>:<tool>` so two servers cannot collide.
- Secrets: env/API keys per server live in OpenBao at `secret/data/talentos/tenants/<tid>/mcp/<server_id>`.
- Safety: stdio servers are a code-execution surface. v1 restricts them to an allowlist of `command` prefixes (`npx`, `node`, `python3` in a sandbox dir) and requires `ai.settings.manage`; a tenant admin cannot run arbitrary shell.
- Existing built-in tools (`calculate_tax`, …) stay in the Rust registry; MCP tools are an additional source, merged at agent runtime.

## 4. Skills — user-authored capabilities

A skill is **a named prompt + a tool allowlist + a model role + an output contract**, reusable across agents. Not code.

```sql
CREATE TABLE ai_skills (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT,
  system_prompt TEXT NOT NULL,
  model_role TEXT NOT NULL DEFAULT 'chat'
    CHECK (model_role IN ('chat','embedding','ocr','vision','graph_extract')),
  tools JSONB NOT NULL DEFAULT '[]',      -- ["calculate_tax","mcp:hr:lookup"]
  kg_scope JSONB NOT NULL DEFAULT '{}',   -- {classes:[], depth:2}
  output_schema JSONB,                    -- optional JSON schema for structured output
  version INT NOT NULL DEFAULT 1,
  enabled BOOLEAN NOT NULL DEFAULT true,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, name, version)
);
```

- UI: **Skills** tab → list → "สร้างสกิล" (name, description, prompt, tool multi-select from built-ins + enabled MCP tools, model role, KG scope, optional schema).
- Versioning: editing creates `version + 1`; agents pin a version so behaviour changes are explicit.
- Built-in skills ship as seeded rows (e.g. `leave-policy-answer`, `payroll-anomaly-explain`) and are editable (copy-on-write, `is_builtin` flag).

## 5. Agents — composition

An agent is the binding of a model role, a prompt, skills and tools:

```sql
CREATE TABLE ai_agents (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT,
  model_role TEXT NOT NULL DEFAULT 'chat',
  system_prompt TEXT,
  skill_ids UUID[] NOT NULL DEFAULT '{}',
  tool_allowlist JSONB NOT NULL DEFAULT '[]',  -- extra built-in/MCP tools not covered by skills
  kg_scope JSONB NOT NULL DEFAULT '{"depth":2}',
  max_cycles INT NOT NULL DEFAULT 3,
  allow_destructive BOOLEAN NOT NULL DEFAULT false,
  enabled BOOLEAN NOT NULL DEFAULT true,
  UNIQUE (tenant_id, name)
);
```

Runtime resolution for a chat request:

```
agent (default: seeded "HR Assistant")
  → model_role → provider + model (ai_model_roles → ai_providers)
  → skills → prompts + tool allowlists (union, deduped)
  → tools = builtin ∪ enabled mcp_tools ∪ skills.tools
  → kg_scope → subgraph retrieval
  → one loop: retrieve → plan → call tools (gated) → answer with citations
```

`POST /ai/chat` gains `agent_id`; when absent it uses the tenant's default agent.

## 6. API surface (additions)

| Endpoint | Method | Permission |
|---|---|---|
| `/api/v1/ai/providers` (extended fields) | GET/POST/PUT/DELETE | `ai.settings.manage` |
| `/api/v1/ai/providers/:id/models` | POST (fetch + normalise per catalog_style) | `ai.settings.manage` |
| `/api/v1/ai/providers/:id/test` | POST | `ai.settings.manage` |
| `/api/v1/ai/mcp/servers` | GET/POST/PUT/DELETE | `ai.settings.manage` |
| `/api/v1/ai/mcp/servers/:id/sync` | POST (`tools/list`) | `ai.settings.manage` |
| `/api/v1/ai/mcp/tools` | GET (joined, namespaced) | `ai.chat` |
| `/api/v1/ai/skills` | GET/POST/PUT (new version)/DELETE | `ai.settings.manage` |
| `/api/v1/ai/agents` | GET/POST/PUT/DELETE | `ai.settings.manage` |
| `/api/v1/ai/chat` | POST `{agent_id?, message}` | `ai.chat` |

## 7. UI shape

```
/settings/ai
├─ Providers   list → add/edit (preset auto-fills auth/catalog style)
│              → fetch models → per-model capability override
├─ Model roles 5 selects (from cached catalogs)
├─ MCP         servers list → add → sync tools → per-tool toggles
├─ Skills      list → create/edit (prompt, tools, model role, KG scope)
├─ Agents      list → create/edit (model role, skills, extra tools, KG depth)
└─ Knowledge   graph inspector (existing)
```

Each list has create/edit/delete and an "ทดสอบ" action where it makes sense (provider → completion; MCP → sync; agent → dry-run one turn without tools).

## 8. Security and limits

- Every secret (provider key, MCP env) in OpenBao, never in Postgres or responses.
- stdio MCP is code execution: allowlist command prefixes, run as the `opc` user, no shell metacharacters, per-tenant working dir.
- Tool calls are gated by `allow_destructive` and audited into `ai_tool_calls` (table already exists).
- A skill cannot grant a tool its author could not use — tool lists intersect with the caller's permissions at runtime.
- Quotas: max 10 providers, 20 MCP servers, 50 skills, 20 agents per tenant (v1).

## 9. Phasing

1. **Providers done right** — auth/catalog styles, per-vendor normalisers, capability inference, provider test. (small, unblocks real keys)
2. **MCP servers + tool sync + toggles.**
3. **Skills CRUD + versioning.**
4. **Agents CRUD + chat `agent_id` + runtime composition.**
5. **UI tabs** as in §7, then E2E.

## 10. Decisions needed

1. **stdio MCP**: allow it in v1 (with the command allowlist) or HTTP transports only?
2. **Built-in skills**: seed a starter set (leave policy, payroll anomaly, onboarding checklist) or start empty?
3. **Agent default**: one seeded "HR Assistant" per tenant, or chat requires an explicit agent?

# AI Provider & Model Roles — Design Addendum

- **Date:** 2026-11-03
- **Status:** approved direction, implementation in progress
- **Supersedes:** §6 `ai_settings` single-provider row and §10 AI settings UI from
  `2026-11-02-ai-settings-knowledge-graph-design.md`.

## 1. Why

The first cut stored one provider (base URL + key + model) per tenant. Real HR AI
needs several providers (a commercial one for chat, a local one for embeddings or
OCR), and different models for different jobs:

- **chat** — the assistant
- **embedding** — KG nodes / RAG
- **ocr** — scanned documents and receipts
- **vision** — images on claims and documents
- **graph_extract** — turning free text into KG triples

## 2. Data model

```sql
CREATE TABLE ai_providers (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  name TEXT NOT NULL,                    -- "OpenAI", "Ollama (local)"
  kind TEXT NOT NULL,                    -- preset id, see §3
  base_url TEXT NOT NULL,
  api_key_ref TEXT,                      -- OpenBao path; NULL for local providers
  models_cache JSONB NOT NULL DEFAULT '[]',
  models_fetched_at TIMESTAMPTZ,
  enabled BOOLEAN NOT NULL DEFAULT true,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, name)
);

CREATE TABLE ai_model_roles (
  tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK (role IN ('chat','embedding','ocr','vision','graph_extract')),
  provider_id UUID NOT NULL REFERENCES ai_providers(id) ON DELETE CASCADE,
  model TEXT NOT NULL,
  params JSONB NOT NULL DEFAULT '{}',    -- temperature, dimensions, max_tokens
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id, role)
);
```

`ai_settings` keeps only cross-cutting flags (`mcp_enabled`, `skills`, `agents`)
and drops `provider/base_url/model/api_key_set` — those move to the tables above.
Migration renames the old columns out; no data to preserve (keys were never stored).

## 3. Provider presets (v1 = OpenAI-compatible)

| kind | label | default base_url | key required |
|---|---|---|---|
| `openai` | OpenAI | `https://api.openai.com/v1` | yes |
| `anthropic-proxy` | Anthropic (via OpenAI-compatible gateway) | — | yes |
| `google-proxy` | Google (via OpenAI-compatible gateway) | — | yes |
| `openrouter` | OpenRouter | `https://openrouter.ai/api/v1` | yes |
| `groq` | Groq | `https://api.groq.com/openai/v1` | yes |
| `together` | Together | `https://api.together.xyz/v1` | yes |
| `mistral` | Mistral | `https://api.mistral.ai/v1` | yes |
| `deepseek` | DeepSeek | `https://api.deepseek.com/v1` | yes |
| `xai` | xAI | `https://api.x.ai/v1` | yes |
| `ollama` | Ollama (local) | `http://127.0.0.1:11434/v1` | no |
| `vllm` | vLLM | `http://127.0.0.1:8000/v1` | optional |
| `custom` | Custom OpenAI-compatible | user fills | optional |

Native Anthropic/Google SDKs are out of scope; their OpenAI-compatible gateways
cover them without a second client.

## 4. Fetch models

`POST /api/v1/ai/providers/:id/models` (guard `ai.settings.manage`):

1. decrypt the key from OpenBao (skip the header when none),
2. `GET {base_url}/models`,
3. normalise to `[{id, owned_by, created}]`, cache in `ai_providers.models_cache`,
4. return the list.

On failure return `{error: "provider_unreachable", message}` with the upstream
status; never return the key or the raw upstream body.

## 5. Model roles UI

After fetching, each of the five roles gets a select populated from the cached
model list, with a **capability filter** and a "แสดงทั้งหมด" toggle:

- `embedding`: ids containing `embed` or `bge`/`e5`/`nomic`
- `vision`: `vision`, `vl`, `gpt-4o`, `claude-3`, `gemini`, `llava`
- `ocr`: `ocr`, `dots`, `got`, `nougat`
- `graph_extract`: models with a large context hint, default = the chat model
- `chat`: everything else

Filters are heuristics; the toggle makes them advisory, not a gate.

## 6. Settings page layout

```
/settings/ai
├─ tab: Providers      list + add/edit/delete, "ดึงรายการโมเดล" per provider
├─ tab: Model roles    five selects + params, saved per role
├─ tab: MCP            enable, allowed servers/tools
├─ tab: Skills         per-tool toggles (slides over the tool registry list)
├─ tab: Agents         agent definitions (PolicyQuery, PayrollAnomaly, …) with
│                      allowed tools + default model role
└─ link: Knowledge graph → /settings/ai/graph
```

Tabs are client-side; each saves independently.

## 7. Runtime

`ai_model_roles` is the only source of truth at request time:

- chat handler → role `chat`
- KG rebuild embeddings → role `embedding`
- document upload / claim receipt → `ocr`, then `vision` when the file is an image
- graph extraction job → `graph_extract`

`talentos-model-gw` gains `client_for(provider)`: build an OpenAI-compatible
client from `base_url` + decrypted key, reusing the existing `HuakatiClient`
machinery (`with_config`). No new HTTP stack.

## 8. Migration from the first cut

`ai_settings.provider/base_url/model` → seed one `ai_providers` row named
"Default" when they are non-empty, and a `chat` role row. `api_key_set` is
dropped; the OpenBao ref moves to `ai_providers.api_key_ref`.

## 9. Testing

- Rust: preset base URLs; fetch-models normalisation with a fake upstream;
  provider unreachable → 502 with no key leakage; role upsert; RBAC.
- vitest: capability filter mapping per role.
- E2E: add provider → fetch (stubbed) → pick models for two roles → save →
  reload shows the selection; chat uses the `chat` role.

## 10. Out of scope (v1)

- Native Anthropic / Google protocols.
- Per-provider rate limits and cost tables (cost tracking stays on the chat call).
- Automatic model benchmarking.

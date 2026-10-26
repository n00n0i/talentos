# TalentOS Phase 4 — AI Orchestrator Specification

> **Scope**: huakati.online model gateway, RAG pipeline (pgvector), Temporal agent framework (Observe→Plan→Act→Reflect), built-in tools registry, human-in-the-loop protocol, cost tracking, and new API endpoints.
> **Dependency**: Phase 1–3 must be resolved — all P0 blockers from phase-3-report.md (GraphQL directives, PII masking, leave/payslip stubs, MCP SQL) are prerequisites.
> **Design principle**: AI-Native = AI agents automate HR workflows end-to-end, not just a chatbot sidebar. Every agent run is auditable, reversible until approved, and tenant-isolated.

---

## 1. Architecture Overview

### 1.1 System Diagram (textual)

```
┌─────────────────────────────────────────────────────────────────┐
│                       API Gateway (Axum)                        │
│  /api/v1/ai/chat, /agents/:id/run, /documents, /cost            │
└──────┬──────────────────────────┬───────────────────────────────┘
       │                          │
       ▼                          ▼
┌──────────────┐         ┌──────────────────┐
│ Model Gateway│         │   RAG Pipeline    │
│(talentos-    │         │ (talentos-rag)    │
│ model-gw)    │         │ pgvector + chunks │
└──────┬───────┘         └────────┬─────────┘
       │                          │
       ▼                          ▼
┌──────────────────────────────────────────┐
│        Agent Framework (Temporal)         │
│  AgentWorkflow → Activities → MCP tools  │
└────────────────┬─────────────────────────┘
                 │
      ┌──────────┼──────────┐
      ▼          ▼          ▼
┌─────────┐ ┌────────┐ ┌──────────────┐
│ PostgreSQL │ pgvector │ MCP (existing) │
│ (new tables) │  (1536 │ crates/api/mcp │
│             │  dims) │               │
└───────────┘ └────────┘ └──────────────┘
```

### 1.2 Core Concepts

| Concept | Description |
|---------|-------------|
| **Model Gateway** | Single abstraction for all LLM calls (huakati.primary → Ollama fallback → error). Handles rate limiting, circuit breaking, token counting, per-tenant cost tracking. |
| **RAG Pipeline** | Document ingestion (PDF/MD/TXT/JSON) → chunking → embedding → pgvector storage. Hybrid search: vector cosine similarity + keyword tsvector with RRF reranking. |
| **Agent** | Temporal state machine: OBSERVE → PLAN → ACT → REFLECT. Each run is a durable workflow with signals for human approval. |
| **Built-in Tools** | MCP tool wrappers with agent-specific pre/post logic (validation, formatting, citation). |
| **Human-in-the-loop** | All irreversible actions require explicit `HumanApprove` signal before ACT proceeds. |

---

## 2. Module Boundaries (New Crates)

| Crate | Responsibility | Depends On |
|-------|---------------|------------|
| `crates/ai/talentos-model-gw/` | huakati.online client: chat completion, embeddings, function calling, token counting, cost tracking | `reqwest`, `tokio`, `serde` |
| `crates/ai/talentos-rag/` | pgvector ingestion: chunking, embedding, semantic search, hybrid search | `talentos-model-gw`, `sqlx`, `pgvector` |
| `crates/ai/talentos-agent/` | Temporal workflow: `AgentWorkflow`, `AgentActivity`; Observe→Plan→Act→Reflect state machine | `talentos-model-gw`, `talentos-rag`, `talentos-tools`, `temporal-sdk` |
| `crates/ai/talentos-tools/` | Built-in tools registry: each tool = MCP wrapper + agent-specific logic | `talentos-mcp`, `talentos-agent` |
| `crates/api/mcp/` | (existing, updated) Expose tools to agents via MCP protocol; enforce PII masking (post Phase 3 fix) | `talentos-authz`, `sqlx` |

**Workspace Cargo.toml** additions:

```toml
[workspace]
members = [
    "crates/core/kernel",
    "crates/core/authz",
    "crates/infra/db",
    "crates/api/gateway",
    "crates/api/graphql",
    "crates/domain/talentos-compliance",
    "crates/domain/talentos-payroll",
    "crates/services/talentos-worker",
    "crates/api/mcp",
    "crates/ai/talentos-model-gw",
    "crates/ai/talentos-rag",
    "crates/ai/talentos-agent",
    "crates/ai/talentos-tools",
]
```

---

## 3. Model Gateway (`talentos-model-gw`)

### 3.1 Overview

huakati.online client implementing the OpenAI-compatible API contract. All LLM calls route through this crate so that fallback, rate limiting, and cost tracking are centralized.

**Endpoint**: `https://llmapi.huakati.online/v1`
**Auth**: `HUAKATI_API_KEY` from env; optional per-tenant key override via `tenant_api_keys` table (added in migration).
**Primary model**: `qwen2.5-72b-instruct`
**Fallback model**: `gpt-4o-mini`
**Embedding model**: `text-embedding-3-large` equivalent (1536 dims)

### 3.2 Public API Surface

```rust
pub struct ModelGateway {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    primary_model: String,
    fallback_model: String,
    embedding_model: String,
    rate_limiter: Arc<Mutex<TenantRateLimiter>>,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl ModelGateway {
    pub fn new(config: &GatewayConfig) -> Self;

    /// Single-turn chat with optional function calling.
    /// Returns ChatCompletionResponse with usage (input/output/cached tokens).
    pub async fn chat(
        &self,
        tenant_id: TenantId,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDef>>,
    ) -> Result<ChatResponse, GatewayError>;

    /// Generate embedding vector (1536 dims).
    pub async fn embed(
        &self,
        tenant_id: TenantId,
        text: &str,
    ) -> Result<Vec<f32>, GatewayError>;

    /// Get token counts and estimated cost for a request.
    pub fn estimate_cost(&self, input_tokens: usize, output_tokens: usize) -> CostEstimate;

    /// Records actual token usage for cost tracking.
    pub async fn record_usage(
        &self,
        tenant_id: TenantId,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        cached_tokens: u64,
    ) -> Result<(), DbError>;
}
```

### 3.3 Rate Limiting

- **Algorithm**: Token bucket, 60 req/min per tenant, burst capacity = 10
- **Implementation**: `fred` (Redis) distributed lock-free counter per `tenant_id`
- **Response on limit**: `429 Too Many Requests` with `Retry-After` header (seconds)

### 3.4 Circuit Breaker

- **Failure threshold**: 5xx errors > 10% of requests in sliding 1-minute window
- **Open state**: Auto-fallback to `gpt-4o-mini` for subsequent requests
- **Half-open**: After 30s, allow one probe request; if successful, close circuit
- **Metrics**: Prometheus-style counters exposed at `/internal/metrics`

### 3.5 Fallback Chain

```
huakati.online (primary, qwen2.5-72b)
    ↓ on 5xx or timeout > 8s
local Ollama (if HUAKATI_LOCAL_OLLAMA_URL set, model: local-qwen)
    ↓ if unavailable
return GatewayError::ModelUnavailable
```

---

## 4. RAG Pipeline (`talentos-rag`)

### 4.1 Overview

Converts unstructured documents into searchable embeddings stored in pgvector. Supports re-indexing on document change via Temporal workflow trigger.

### 4.2 Document Ingestion

**Supported formats**:
- **PDF**: payslips (encrypted PDF), policy documents → extract text via `lopdf` + fallback to `pdf-extract` CLI
- **Markdown**: handbooks, procedural docs → raw text with header hierarchy preserved
- **TXT**: regulations, notices → direct text
- **JSON**: API specs, config exports → flatten to text chunks

**Chunking strategy**:
```rust
pub struct ChunkingConfig {
    pub chunk_size: usize,         // 1000 chars
    pub overlap: usize,            // 200 chars
    pub separator: &'static str,   // "\n\n" (paragraph boundary)
}
```
Recursive splitting: split by separator → if chunk > `chunk_size`, split on whitespace → if still too large, truncate.

### 4.3 Embedding

- Via `ModelGateway::embed()` → 1536-dim `vector` column in pgvector
- Batch size: 32 documents per embed call ( huakati limit: ~50 concurrent)
- Idempotency: `content_hash` (SHA-256) prevents duplicate embeddings

### 4.4 Database Schema (New Tables)

```sql
-- Migrations/20261027000001_ai_core.sql

-- RAG document store
CREATE TABLE ai_documents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    entity_id UUID REFERENCES entities(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL,
    doc_type TEXT NOT NULL CHECK (doc_type IN ('policy', 'regulation', 'payslip', 'api_spec', 'handbook')),
    source_path TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(tenant_id, content_hash)
);

-- Semantic search index (pgvector extension required)
CREATE EXTENSION IF NOT EXISTS vector;
CREATE TABLE ai_document_chunks (
    id BIGSERIAL PRIMARY KEY,
    document_id UUID NOT NULL REFERENCES ai_documents(id) ON DELETE CASCADE,
    chunk_index INT NOT NULL,
    content TEXT NOT NULL,
    embedding vector(1536),
    word_count INT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(document_id, chunk_index)
);

-- Hybrid search: tsvector for keyword + pgvector for semantic
CREATE INDEX idx_ai_chunks_embedding ON ai_document_chunks USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);
CREATE INDEX idx_ai_chunks_doc ON ai_document_chunks(document_id);
CREATE INDEX idx_ai_documents_tenant_type ON ai_documents(tenant_id, doc_type);
CREATE INDEX idx_ai_documents_hash ON ai_documents(content_hash);
```

### 4.5 Search API

```rust
pub struct HybridSearchResult {
    pub chunks: Vec<RankedChunk>,
    pub query: String,
    pub filters: SearchFilters,
    pub total_hits: usize,
}

pub struct RankedChunk {
    pub chunk: DocumentChunk,
    pub vector_score: f32,       // cosine similarity
    pub keyword_score: f32,      // BM25/ts_rank
    pub rrf_score: f32,          // reciprocal rank fusion
}

impl ModelGateway {
    pub async fn hybrid_search(
        &self,
        filters: &SearchFilters,
        query: &str,
        top_k: usize,
    ) -> Result<HybridSearchResult, RAGError>;
}
```

**RRF formula**: `rrf_score = Σ(1 / (k + rank_i))` where `k=60`, `rank_i` = position in each result list.

### 4.6 Citation Format

Every RAG result returns chunks with grounding metadata for LLM context injection:

```rust
pub struct Citation {
    pub chunk_text: String,
    pub source_doc: DocReference,  // tenant_id, doc_id, source_path
    pub similarity: f32,
    pub chunk_index: usize,
}
```

---

## 5. Agent Framework (`talentos-agent`)

### 5.1 State Machine

Each agent run follows a deterministic four-phase loop:

```
  ┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐
  │ OBSERVE  │────▶│   PLAN   │────▶│   ACT    │────▶│ REFLECT  │
  │  Fetch   │     │ Generate │     │ Execute  │     │ Evaluate │
  │  context │     │  action  │     │  tools   │     │  result  │
  └──────────┘     └──────────┘     └──────────┘     └──────────┘
       ▲                                                       │
       │              ┌─────────────────────┐                  │
       └──────────────│      REPLAN          │◀── failure/     │
                      │  (insufficient info) │    low score    │
                      └─────────────────────┘                  │
                                                               ▼
                                                          ┌──────────┐
                                                          │   DONE   │
                                                          └──────────┘
```

**Phase descriptions**:

| Phase | Activity | Responsibility |
|-------|----------|----------------|
| **OBSERVE** | `ObserveActivity` | Query RAG for context, fetch employee/payroll state via MCP, build observation summary |
| **PLAN** | `PlanActivity` | LLM generates structured action plan (tool calls, parameters, expected outcome) |
| **ACT** | `ActActivity` | Execute tool calls via MCP; pause for `HumanApprove` signal on irreversible actions |
| **REFLECT** | `ReflectActivity` | Evaluate outcome against plan; if score < threshold or plan incomplete → REPLAN |

### 5.2 Temporal Workflow Definition

```rust
#[workflow(name = "agent_workflow")]
pub struct AgentWorkflow {
    agent_id: AgentId,
    task: String,
    context: AgentContext,
}

#[workflow(query(name = "get_state"))]
pub fn get_state(&self) -> WorkflowState;

#[workflow(query(name = "get_plan"))]
pub fn get_plan(&self) -> Option<ActionPlan>;

#[workflow(query(name = "get_history"))]
pub fn get_history(&self) -> Vec<StepSnapshot>;

#[workflow(signal(name = "human_approve"))]
pub fn human_approve(&mut self, signal: HumanApprovalSignal);

#[workflow(signal(name = "human_reject"))]
pub fn human_reject(&mut self, signal: HumanRejectionSignal);

#[workflow(signal(name = "provide_context"))]
pub fn provide_context(&mut self, signal: AdditionalContextSignal);
```

### 5.3 Workflow Execution Logic

```rust
#[workflow(handle]
pub async fn execute(mut self) -> AgentResult {
    let mut cycle = 0usize;
    let max_cycles = 3;
    let cycle_timeout = Duration::from_secs(600); // 10 min per cycle

    loop {
        workflow::timeout(cycle_timeout, async {
            // OBSERVE
            let observation = activity::observe(&self.agent_id, &self.task, &self.context).await?;
            self.record_step("OBSERVE", Observation { ... });

            // PLAN
            let plan = activity::plan(&observation, &self.task).await?;
            self.record_step("PLAN", plan.clone());

            // ACT (may pause for human approval)
            let result = activity::act(&plan).await?;
            self.record_step("ACT", result.clone());

            // REFLECT
            let score = activity::reflect(&plan, &result)?;
            self.record_step("REFLECT", Reflection { score, ... });

            if score >= 0.8 && result.is_complete() {
                return Ok(AgentResult::Success(result));
            }
            if score < 0.3 && cycle >= max_cycles {
                return Err(AgentError::MaxCyclesExceeded);
            }
            // If score between 0.3-0.8, continue to next cycle (REPLAN)
        }).await?;

        cycle += 1;
    }
}
```

### 5.4 Retry Policy

| Error Type | Retry | Backoff | Notes |
|------------|-------|---------|-------|
| Tool validation error | 0 | — | Non-retryable; return error to caller |
| MCP timeout | 3 | Exponential (1s, 2s, 4s) | Per-tool retry within ACT phase |
| LLM rate limit | 3 | Exponential (2s, 4s, 8s) | With jitter |
| Circuit breaker open | 0 | — | Fail fast, escalate to human |
| Human approval timeout | Auto-reject | — | See §6 |

---

## 6. Agent Types

| Agent | Purpose | Key Tools | Approval Required |
|-------|---------|-----------|-------------------|
| `PayrollAnomalyAgent` | Detect anomalies in payroll data → explain root cause → suggest fix | `detect_anomalies`, `query_employee`, `calculate_tax` | No (read-only analysis) |
| `PolicyQueryAgent` | Search policy documents → summarize → cite sources | `search_policy`, `query_employee` | No |
| `PayrollRunAgent` | End-to-end payroll run: start → monitor → detect anomalies → draft report → request approval → disburse | `run_payroll`, `summarize_payroll_run`, `detect_anomalies` | Yes (start run, disburse) |
| `LeavePolicyAgent` | Draft leave policy → check compliance → propose to HR | `draft_leave_policy`, `search_policy` | Yes (propose to HR) |
| `ComplianceMonitorAgent` | Monitor Royal Gazette for regulation changes → alert → draft updates | `search_policy`, `check_compliance` | No (alert only) |

---

## 7. Built-in Tools (`talentos-tools`)

Each tool wraps an MCP tool with agent-specific validation, formatting, and error handling.

### 7.1 Tool Registry

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn AgentTool>>,
}

pub trait AgentTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult;
    fn is_destructive(&self) -> bool;  // triggers human-in-the-loop
}
```

### 7.2 Tool Definitions

| Tool Name | MCP Source | Description | Destructive |
|-----------|------------|-------------|-------------|
| `run_payroll` | `tools/payroll::run_payroll` | Validate period → start payroll run → monitor → return summary. Wraps MCP `run_payroll` with confirmation logic. | ✅ Yes |
| `query_employee` | `tools/employee::query_employees` | Filter + format for agent consumption; applies PII masking per role. | ❌ No |
| `search_policy` | `tools/compliance::search_policy` | Hybrid RAG search + citation formatting; returns structured citations. | ❌ No |
| `calculate_tax` | `tools/compliance::calculate_tax_preview` | Input validation + formatted tax breakdown; pure calculation, no DB write. | ❌ No |
| `detect_anomalies` | Internal | Runs anomaly rules on payroll run (variance > 2σ, missing SSO, etc.); returns ranked list. | ❌ No |
| `draft_leave_policy` | Internal | LLM drafts policy → compliance check against Thai labor law → returns structured draft. | ❌ No |
| `summarize_payroll_run` | `get_payroll_status` + `list_payslips` | Aggregate totals + top anomalies + narrative summary. | ❌ No |
| `check_compliance` | `calculate_tax_preview` + `sso_contribution_check` | Cross-check tax + SSO rules; returns compliance report. | ❌ No |

### 7.3 Tool Execution Context

```rust
pub struct ToolContext {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub entity_id: Option<EntityId>,
    pub run_id: Uuid,  // current agent run
    pub db: TenantPool,  // tenant-scoped connection
    pub model_gw: Arc<ModelGateway>,
}
```

---

## 8. Human-in-the-Loop Protocol

### 8.1 Overview

All irreversible actions (payroll start, disbursement, policy proposal) require explicit human approval via Temporal signal before proceeding. This is enforced at the `ACT` phase level.

### 8.2 Signal Handling

```rust
// On irreversible tool call, workflow pauses and emits PendingApproval event
#[derive(Signal)]
pub struct HumanApprovalSignal {
    pub run_id: Uuid,
    pub approver_user_id: UserId,
    pub tool_name: String,
    pub decision: ApprovalDecision,  // Approve | Reject
    pub comment: Option<String>,
    pub timestamp: DateTime<Utc>,
}

pub enum ApprovalDecision {
    Approve,
    Reject,
}
```

### 8.3 Timeout & Escalation

| Condition | Action |
|-----------|--------|
| `HumanApprove` signal received → | Resume ACT with approved tool execution |
| `HumanReject` signal received → | Fail workflow with `AgentError::ApprovalRejected(comment)` |
| No signal within **4 hours** → | Auto-reject; emit `approval_timeout` event; notify HR admin via alert |
| Approver's role expired/unavailable → | Escalate to next user with `hr_admin` or `payroll_manager` role; re-start 4h timeout |

### 8.4 Audit Trail

Every approval event is recorded in `ai_agent_runs.human_approver` and `approved_at` columns. Full signal history is queryable via `GetHistory`.

---

## 9. Database Schema (New Migrations)

```sql
-- Migrations/20261027000002_ai_agent_schema.sql

-- Agent conversation/store
CREATE TABLE ai_conversations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id TEXT NOT NULL,           -- agent type identifier
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    messages JSONB NOT NULL DEFAULT '[]',
    state TEXT NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'paused', 'completed', 'failed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Agent run tracking
CREATE TABLE ai_agent_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_type TEXT NOT NULL CHECK (agent_type IN (
        'PayrollAnomalyAgent', 'PolicyQueryAgent', 'PayrollRunAgent',
        'LeavePolicyAgent', 'ComplianceMonitorAgent'
    )),
    task TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'running' CHECK (state IN (
        'running', 'paused_for_approval', 'completed', 'failed', 'timeout'
    )),
    context JSONB NOT NULL DEFAULT '{}',
    result JSONB,
    human_approver UUID REFERENCES users(id),
    approved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_ai_agent_runs_tenant ON ai_agent_runs(tenant_id, created_at DESC);
CREATE INDEX idx_ai_agent_runs_type ON ai_agent_runs(agent_type, state);

-- Tool call audit log
CREATE TABLE ai_tool_calls (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES ai_agent_runs(id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    input JSONB NOT NULL,
    output JSONB,
    duration_ms BIGINT NOT NULL DEFAULT 0,
    status TEXT NOT NULL CHECK (status IN ('success', 'failed', 'pending_approval')),
    error JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_ai_tool_calls_run ON ai_tool_calls(run_id);

-- Cost tracking per tenant per day
CREATE TABLE ai_cost_tracking (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    input_tokens BIGINT NOT NULL DEFAULT 0,
    output_tokens BIGINT NOT NULL DEFAULT 0,
    cached_tokens BIGINT NOT NULL DEFAULT 0,
    cost_usd NUMERIC(12,6) NOT NULL DEFAULT 0,
    date DATE NOT NULL DEFAULT current_date,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(tenant_id, model, date)
);

CREATE INDEX idx_ai_cost_tenant_date ON ai_cost_tracking(tenant_id, date DESC);

-- Per-tenant API key override (optional)
CREATE TABLE tenant_api_keys (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE UNIQUE,
    api_key TEXT NOT NULL,          -- encrypted at rest
    provider TEXT NOT NULL DEFAULT 'huakati',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

---

## 10. API Contracts (New Endpoints)

All endpoints live under `/api/v1/ai/` with tenant-scoped RLS via `app.current_tenant_id()`.

| Endpoint | Method | Permission | Handler | Description |
|----------|--------|------------|---------|-------------|
| `/api/v1/ai/chat` | POST | `ai.chat` | `ai_chat_handler` | Single-turn chat with configurable agent (defaults to `PolicyQueryAgent`) |
| `/api/v1/ai/agents/:id/run` | POST | `ai.agent.run` | `start_agent_run` | Start a new agent task; returns `run_id` |
| `/api/v1/ai/agents/:id/status` | GET | `ai.agent.read` | `get_agent_status` | Get workflow state, plan, history |
| `/api/v1/ai/agents/:id/approve` | POST | `ai.agent.approve` | `signal_approval` | Send HumanApprove/HumanReject signal |
| `/api/v1/ai/documents` | POST | `ai.document.upload` | `upload_document` | Upload and trigger RAG re-index |
| `/api/v1/ai/documents/search` | POST | `ai.document.search` | `search_documents` | Hybrid search with filters and citations |
| `/api/v1/ai/cost` | GET | `ai.cost.read` | `get_cost_report` | Cost aggregation per tenant/day/month |

### 10.1 Request/Response Schemas

**POST `/api/v1/ai/chat`**:
```json
{
  "agent": "PolicyQueryAgent",
  "message": "How much is the parental leave?",
  "context": { "entity_id": "uuid-here" }
}
```

**POST `/api/v1/ai/agents/:id/run`**:
```json
{
  "task": "Run payroll for 2026-10",
  "agent": "PayrollRunAgent",
  "params": {
    "period_start": "2026-10-01",
    "period_end": "2026-10-31",
    "entity_id": "uuid-here"
  }
}
```

**POST `/api/v1/ai/agents/:id/approve`**:
```json
{
  "decision": "approve",
  "comment": "Approved after manual review"
}
```

**POST `/api/v1/ai/documents/search`**:
```json
{
  "query": "parental leave policy",
  "filters": {
    "doc_type": "policy",
    "date_range": { "from": "2024-01-01" }
  },
  "top_k": 5
}
```

---

## 11. Dependencies

Add to workspace `Cargo.toml`:

```toml
# crates/ai/talentos-model-gw/Cargo.toml
[dependencies]
reqwest = { workspace = true, features = ["json", "stream"] }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
futures-util = { workspace = true }
tiktoken-rs = "0.4"        # token counting
lopdf = "0.33"             # PDF text extraction (fallback to pdf-extract crate)
async-trait = "0.1"

# crates/ai/talentos-rag/Cargo.toml
[dependencies]
talentos-model-gw = { path = "../talentos-model-gw" }
sqlx = { workspace = true }
uuid = { workspace = true }
sha2 = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
rand = { workspace = true }

# crates/ai/talentos-agent/Cargo.toml
[dependencies]
talentos-model-gw = { path = "../talentos-model-gw" }
talentos-rag = { path = "../talentos-rag" }
talentos-tools = { path = "../talentos-tools" }
talentos-mcp = { path = "../../api/mcp" }
temporal-sdk = { workspace = true }
temporal-client = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }

# crates/ai/talentos-tools/Cargo.toml
[dependencies]
talentos-mcp = { path = "../../api/mcp" }
talentos-agent = { path = "../talentos-agent" }
talentos-model-gw = { path = "../talentos-model-gw" }
talentos-payroll = { path = "../../domain/talentos-payroll" }
talentos-compliance = { path = "../../domain/talentos-compliance" }
serde = { workspace = true }
serde_json = { workspace = true }
sqlx = { workspace = true }
thiserror = { workspace = true }
```

---

## 12. Security Considerations

| Concern | Mitigation |
|---------|------------|
| **Tenant isolation in RAG** | `ai_documents` and `ai_document_chunks` both have `tenant_id` column; RLS policies enforced via `app.current_tenant_id()` in every query |
| **PII leakage in LLM context** | RAG chunks filtered by `doc_type`; employee PII fields excluded from policy/regulation documents; `query_employee` tool applies masking |
| **Token cost manipulation** | Cost tracked server-side via `talentos-model-gw::record_usage()`, not trusted LLM response; per-tenant daily budget cap configurable |
| **Malicious tool output injection** | Agent validates tool output schema before passing to LLM; regex/structure validation on `calculate_tax`, `detect_anomalies` outputs |
| **Circuit breaker bypass** | Fallback chain uses separate API key; Ollama local mode disabled by default (`HUAKATI_LOCAL_OLLAMA_URL` unset) |
| **Audit log tampering** | `ai_tool_calls` and `ai_agent_runs` use append-only design; `created_at` set by server, not client; RLS prevents cross-tenant access |

---

## 13. Cross-Phase References

| Phase 1–3 Reference | Relevance |
|---------------------|-----------|
| `phase-1-spec.md` §3 RBAC matrix | New permissions `ai.chat`, `ai.agent.run`, `ai.agent.read`, `ai.agent.approve`, `ai.document.upload`, `ai.document.search`, `ai.cost.read` |
| `phase-1-spec.md` §4 TenantContext | All agent operations require `TenantContext` from gateway middleware |
| `phase-2-spec.md` §2 Payroll DAG | `PayrollRunAgent` consumes payroll DAG outputs; `detect_anomalies` tool uses `talentos-payroll` |
| `phase-2-spec.md` §4 Compliance rules | `check_compliance` tool uses `talentos-compliance` rulebook |
| `phase-3-report.md` P0 blockers | Must resolve GraphQL directives, PII masking, leave/payslip stubs, MCP SQL before Phase 4 implementation |
| `crates/api/mcp/` existing tools | `talentos-tools` wraps MCP tools; new agent tools extend existing MCP tool surface |

---

## 14. Out of Scope (Deferred)

- Multi-agent collaboration (agent A calling agent B)
- Real-time streaming responses (SSE)
- Agent memory / long-term context across sessions
- Fine-tuned model support
- Webhook notifications for agent completion

# TalentOS Phase 4 — Test Cases

> All tests are **integration-level** where possible (real Model Gateway mock, real pgvector, Temporal workflow). Unit tests cover pure logic (chunking, token counting, RRF reranking).
>
> Base URL: `http://localhost:8080/api/v1`
> Mock huakati endpoint: `http://localhost:9090/v1` (wiremock fixture)

---

## Test Case Table

| TC-ID | Category | Description | Given | When | Then | Priority |
|-------|----------|-------------|-------|------|------|----------|
| **MODEL GATEWAY** |
| MG-01 | Happy Path | Chat completion returns valid response | huakati mock returns `{"choices":[{"message":{"content":"Parental leave is 98 days"}}]}` with usage | `POST /api/v1/ai/chat` with `{"agent":"PolicyQueryAgent","message":"What is parental leave?"}` | `200`; response contains `reply` field; `usage.input_tokens > 0`; `usage.output_tokens > 0` | P0 |
| MG-02 | Happy Path | Embedding returns correct dimension vector | huakati mock returns embedding array of 1536 floats | `POST /api/v1/ai/documents` with a PDF → triggers embed | Response includes `embedding_id`; DB `ai_document_chunks.embedding` has `vector(1536)` type and non-null value | P0 |
| MG-03 | Function Calling | Tool definition passed to LLM correctly | huakati mock returns tool call in response | Agent run starts with `tools: [run_payroll, search_policy]` | `ai_tool_calls` table has `tool_name = "run_payroll"`; LLM message history includes tool definitions | P0 |
| MG-04 | Token Counting | tiktoken-rs counts tokens within 5% of huakati response | Huakati response usage: input=120 tokens, output=45 tokens | `POST /api/v1/ai/chat` | `estimate_cost()` returns `input_tokens ≈ 120 ± 6`; `cost_usd` calculated correctly per pricing table | P1 |
| MG-05 | Rate Limiting | 429 after 60 req/min exceeded | Tenant A sends 61 requests in 60s window | 61st request | `429 Too Many Requests`; `Retry-After` header present; `ai_cost_tracking` not updated for rejected request | P0 |
| MG-06 | Circuit Breaker | Fallback activates when 5xx > 10% | Simulate 12 consecutive 500s from huakati mock (out of 100 recent) | 13th request | Response uses `gpt-4o-mini` model; `circuit_breaker_state = open` logged; fallback request succeeds | P0 |
| MG-07 | Fallback Chain | Ollama fallback when huakati unavailable | `HUAKATI_LOCAL_OLLAMA_URL=http://localhost:11434`; huakati mock returns connection refused | Request to chat endpoint | Falls back to Ollama model; response contains content from local model; no 500 error to caller | P1 |
| MG-08 | Cost Tracking | Usage recorded per tenant per day | 3 chat requests from tenant A, 2 from tenant B | After all requests | `ai_cost_tracking` has 2 rows (one per tenant); `input_tokens` and `output_tokens` sum correctly; `cost_usd` = Σ(tokens × rate) | P0 |
| MG-09 | Error Response | Invalid API key returns 401 | huakati mock returns 401 Unauthorized | Any request with bad key | `401`; error code `"model_unauthorized"`; no stack trace leaked | P0 |
| MG-10 | Error Response | Timeout returns retryable error | huakati mock delays response by 15s (config timeout = 8s) | Request to chat | `GatewayError::Timeout`; retried with exponential backoff (up to 3 attempts); overall error returned after exhausted retries | P1 |
| **RAG PIPELINE** |
| RAG-01 | Document Ingestion (PDF) | PDF payslip ingested and chunked | Valid PDF file (encrypted, text layer extracted) | `POST /api/v1/ai/documents` with PDF | `ai_documents` row created with `doc_type = "payslip"`; `ai_document_chunks` has 3–8 chunks; each chunk has non-null embedding | P0 |
| RAG-02 | Document Ingestion (MD) | Markdown handbook chunked with hierarchy | Valid `.md` file with `#`, `##`, `###` headers | `POST /api/v1/ai/documents` with MD | Chunks preserve header context in metadata (`hierarchy: ["## Policies", "### Leave"]`); chunk_size ≤ 1000 chars | P0 |
| RAG-03 | Document Ingestion (TXT) | Plain text regulation ingested | `.txt` file with Thai labor regulation text | `POST /api/v1/ai/documents` with TXT | Chunks split on `\n\n`; overlap = 200 chars verified by checking consecutive chunk content | P1 |
| RAG-04 | Duplicate Detection | Re-uploading same file is idempotent | Same PDF uploaded twice (same content) | Second upload with identical file | `content_hash` matches; no duplicate chunks inserted; `updated_at` refreshed | P0 |
| RAG-05 | Hybrid Search | Vector + keyword search returns relevant results | Policy document about "parental leave" indexed | Search query: `"how many days parental leave"` | Top result includes chunk mentioning parental leave; `rrf_score` computed; vector_score + keyword_score both present | P0 |
| RAG-06 | Citation Accuracy | Citations include correct source metadata | Document with `source_path = "/policies/leave_2024.md"` indexed | Search returns result | Citation contains `source_doc.source_path`; `similarity` score in `[0, 1]`; chunk text matches database | P0 |
| RAG-07 | Filter Correctness | Search respects `doc_type` filter | Mix of policy and payslip documents indexed | Search with `filters.doc_type = "policy"` | Zero payslip chunks in results; all results have `doc_type = "policy"` | P0 |
| RAG-08 | Tenant Isolation | Cross-tenant RAG search impossible | Tenant A and Tenant B each have different policies | Tenant A user searches for "overtime" | Zero results from Tenant B documents; `ai_documents.tenant_id` filter enforced | P0 |
| RAG-09 | Re-index Trigger | Document update rebuilds chunks | Existing document updated with new content | `POST /api/v1/ai/documents` with same `content_hash` changed | Old chunks deleted; new chunks inserted with new `content_hash`; `updated_at` reflects change | P1 |
| RAG-10 | Empty Result | No match returns empty array, not error | Search for non-existent term | Query `"quantum computing policy"` | `200`; `chunks = []`; `total_hits = 0`; no exception thrown | P1 |
| **AGENT FRAMEWORK** |
| AGT-01 | Full Cycle | Observe→Plan→Act→Reflect completes successfully | PolicyQueryAgent with task "What is parental leave?" | Start agent run | `ai_agent_runs.state = "completed"`; `result` contains narrative summary with citations; max 1 cycle used | P0 |
| AGT-02 | Replan on Failure | Agent replans when reflection score is low | Act phase tool returns incomplete data | Agent run with missing employee record | Second cycle executes; plan adjusted to use different data source; eventually completes or hits max cycles | P1 |
| AGT-03 | Max Cycles Exceeded | Workflow fails after 3 failed cycles | Artificially break tool output in reflection | Agent run configured with failing reflect activity | `state = "failed"`; `error = "MaxCyclesExceeded"`; `ai_tool_calls` shows 3 ACT attempts | P0 |
| AGT-04 | Query State | GetState returns current workflow state | Agent run in progress | `GET /api/v1/ai/agents/:id/status` | `200`; `state = "running"`; `history` contains OBSERVE and PLAN steps; `plan` field populated | P0 |
| AGT-05 | Query History | GetHistory returns full step trace | Completed agent run | `GET /api/v1/ai/agents/:id/status` with `include_history=true` | Returns array of steps with `phase`, `duration_ms`, `input_summary`, `output_summary` | P1 |
| AGT-06 | PayrollRunAgent Happy Path | End-to-end payroll run with anomaly detection | Valid payroll period, 5 active employees | `POST /api/v1/ai/agents/:id/run` with `PayrollRunAgent` | Run transitions: `draft → calculating → pending_approval → (after approve) → completed`; `ai_agent_runs.result` contains total_gross, total_net, anomaly count | P0 |
| AGT-07 | PayrollAnomalyAgent | Detects salary variance > 2σ | Payroll run with one employee salary 3x normal | Agent run with `PayrollAnomalyAgent` | Anomaly flagged with `type = "salary_variance"`, `employee_id`, `expected_range`, `actual_value`; report generated | P0 |
| AGT-08 | LeavePolicyAgent | Drafts policy and checks compliance | Task: "Draft paternity leave policy" | Agent run with `LeavePolicyAgent` | Draft includes Thai labor law references; compliance check marks `paternity_leave_days = 15` as compliant; structured draft returned | P1 |
| AGT-09 | ComplianceMonitorAgent | Monitors for regulation changes | Mock Royal Gazette RSS feed with new SSO rate | Agent run with `ComplianceMonitorAgent` | Alert generated; compares old vs new rate; draft update suggestion returned | P1 |
| **BUILT-IN TOOLS** |
| TL-01 | run_payroll | Wraps MCP tool with confirmation | Valid payroll period params | Tool execution | Calls MCP `run_payroll`; validates `period_start ≤ period_end`; returns `PayrollRun` summary with totals | P0 |
| TL-02 | query_employee | Returns masked PII for non-admin | User with `employee` role queries colleague | Tool execution | `national_id` masked as `***-****-*****`; `bank_account_number` masked; other fields returned | P0 |
| TL-03 | search_policy | Returns citations with RRF scores | Multiple policy docs indexed | Tool search for "overtime calculation" | Results sorted by `rrf_score`; each result has `citation.chunk_text`, `citation.source_doc`, `citation.similarity` | P0 |
| TL-04 | calculate_tax | Pure calculation, no DB side effect | Input: base_salary=50000, filing_status=1 | Tool execution | Returns structured tax breakdown; `total_tax` matches `talentos-compliance` rulebook; no rows written to DB | P0 |
| TL-05 | detect_anomalies | Finds statistical outliers | Payroll run with one anomalous deduction | Tool execution | Returns ranked anomalies with `score`, `type`, `employee_id`, `explanation`; empty array if no anomalies | P0 |
| TL-06 | draft_leave_policy | Generates compliant draft | Task: "Draft maternity leave policy" | Tool execution | Draft includes legal basis (Thai Labor Protection Act); compliance check passes; structured JSON returned | P1 |
| TL-07 | summarize_payroll_run | Aggregates totals and highlights anomalies | Completed payroll run with 3 anomalies | Tool execution | Narrative summary includes `total_gross`, `total_net`, top 3 anomalies, comparison to previous period | P0 |
| TL-08 | check_compliance | Cross-checks tax + SSO rules | Payroll run with incorrect SSO contribution | Tool execution | Returns compliance report with violations; flags SSO underpayment; suggests correction | P1 |
| TL-09 | Tool Authorization | Employee cannot execute destructive tools | User with `employee` role attempts `run_payroll` | Tool execution | `403 InsufficientPermission`; no MCP call made; audit log records denied attempt | P0 |
| TL-10 | Tool Input Validation | Rejects malformed input before MCP call | `run_payroll` with `period_start > period_end` | Tool execution | Returns validation error `InvalidPeriod`; no MCP call; `duration_ms = 0` | P0 |
| **HUMAN-IN-THE-LOOP** |
| HITL-01 | Approve Signal | Workflow resumes after approval | PayrollRunAgent paused at `pending_approval` | `POST /api/v1/ai/agents/:id/approve` with `decision: "approve"` | Workflow resumes; payroll disbursement proceeds; `human_approver` and `approved_at` recorded | P0 |
| HITL-02 | Reject Signal | Workflow fails after rejection | PayrollRunAgent paused at `pending_approval` | `POST /api/v1/ai/agents/:id/approve` with `decision: "reject"`, `comment: "Incorrect period"` | Workflow fails with `ApprovalRejected`; `state = "failed"`; `result.comment` contains rejection reason | P0 |
| HITL-03 | Timeout Auto-Reject | No response within 4 hours → auto-reject | Agent run started, no approve signal sent | Wait 4h+ (or simulate with Temporal clock advancement) | `state = "timeout"`; `human_approver = NULL`; `approved_at = NULL`; HR admin notified | P0 |
| HITL-04 | Escalation | Approver unavailable → escalate to admin | Approver's role revoked mid-pause | Timeout trigger or manual escalation signal | Next `hr_admin` user receives approval request; new 4h timeout starts | P1 |
| HITL-05 | Audit Trail | Full approval history preserved | Agent run with approve + reject + re-approve sequence | Query `ai_agent_runs` and `ai_tool_calls` | All signals logged with `approver_user_id`, `timestamp`, `comment`; chronological order preserved | P0 |
| HITL-06 | Non-destructive Tools Skip Approval | Read-only tools execute without pause | PolicyQueryAgent searching policy | Agent run completes | `ai_tool_calls` shows no `pending_approval` status; workflow runs to completion without signal pause | P0 |
| **RAG SEARCH** |
| RGS-01 | Hybrid RRF Ranking | RRF re-ranks mixed results correctly | Keyword search returns doc A (rank 1) and doc B (rank 3); vector search returns doc B (rank 1) and doc A (rank 2) | Hybrid search query | `rrf_score` for doc B > doc A; result order reflects fusion, not just vector or keyword alone | P1 |
| RGS-02 | Date Range Filter | Results respect `date_range.from` | Policies dated 2023, 2024, 2025 indexed | Search with `date_range.from = "2024-01-01"` | Zero results from 2023 policy; 2024 and 2025 results present | P0 |
| RGS-03 | Entity Scope | Results scoped to entity when provided | Multiple entities under same tenant | Search with `entity_id = E1` | Zero results from E2; all results have `entity_id = E1` | P1 |
| RGS-04 | Empty Chunk Handling | Document with no extractable text handled gracefully | Empty or scan-only PDF uploaded | Upload succeeds | `ai_documents` row created with `chunk_count = 0`; search returns empty; no error thrown | P1 |
| **COST TRACKING** |
| CT-01 | Token Accuracy | Recorded tokens match LLM response usage | Huakati mock returns `usage: {input_tokens: 100, output_tokens: 50}` | Chat request | `ai_cost_tracking.input_tokens = 100`; `ai_cost_tracking.output_tokens = 50`; cost = `100 * input_rate + 50 * output_rate` | P0 |
| CT-02 | Cached Token Discount | Cached input tokens billed at reduced rate | Huakati returns `usage: {cached_tokens: 80, input_tokens: 20}` | Chat request | `cached_tokens = 80` recorded; cost uses discounted cached rate (e.g., 50% of input rate) | P1 |
| CT-03 | Daily Aggregation | Costs roll up to daily total per tenant | 5 requests across models in one day | Query `ai_cost_tracking` for date | Single row per model per tenant; `input_tokens` = sum of all requests; `cost_usd` = sum | P0 |
| CT-04 | Cross-Tenant Isolation | Tenant A cannot see Tenant B costs | Tenant A user queries `/api/v1/ai/cost` | GET cost endpoint | Returns only Tenant A costs; Tenant B rows invisible (RLS enforced) | P0 |
| **SECURITY** |
| SEC-01 | RLS Enforcement | All AI tables respect tenant_id RLS | Tenant A connection with `app.current_tenant_id = A` | Query `SELECT * FROM ai_documents` | Returns only rows with `tenant_id = A`; zero rows from tenant B | P0 |
| SEC-02 | No PII in LLM Context | Employee PII excluded from RAG chunks | Payslip PDF contains national_id | Upload payslip for RAG | Chunk content does NOT contain national_id format (`***-****-*****` pattern absent); only non-PII text indexed | P0 |
| SEC-03 | Tool Authorization | Agent cannot invoke unauthorized tools | Agent with `PolicyQueryAgent` type attempts `run_payroll` | Tool execution | `403`; tool not in agent's allowed list; audit event logged | P0 |
| SEC-04 | Request Logging (No PII) | Audit log contains no PII | Chat request with employee name in message | Request completes | `ai_conversations.messages` stored but PII fields (national_id, bank_account) redacted in log view; raw messages accessible only to admin | P1 |
| SEC-05 | Malicious Tool Output | Agent does not execute arbitrary LLM instructions | Huakati mock returns tool output with shell command | `calculate_tax` tool executes | Output validated against schema; shell command string rejected; tool returns validation error | P0 |
| **END-TO-END** |
| E2E-01 | Payroll Anomaly Detection Flow | Full detect → explain → suggest fix | Payroll run with one anomaly (salary 3x normal) | Agent run with `PayrollAnomalyAgent` | Anomaly detected → explained with root cause (likely data entry error) → suggested fix (correct salary to X) → report generated | P0 |
| E2E-02 | Payroll Run to Disbursement | End-to-end run with human approval | Valid payroll period, 5 employees | `POST /agents/run` → `POST /agents/approve` | Run completes; approval recorded; disbursement status updated; total_net matches DAG calculation | P0 |
| E2E-03 | Policy Query with Citation | Agent finds and cites correct policy | Leave policy document indexed | Chat: "How much is parental leave?" | Response includes quoted policy text + citation with `source_path`; similarity score > 0.7 | P0 |
| E2E-04 | Failed Run Recovery | Agent persists state and can be retried | Agent fails mid-Act due to MCP timeout | Retry agent run with same `run_id` | Workflow resumes from last checkpoint; only failed tool retried; previous successful steps not re-executed | P1 |
| E2E-05 | Multi-Agent Handoff | PolicyQueryAgent delegates to PayrollRunAgent | Task requires both policy lookup and payroll action | Complex task submitted | Agent identifies need for payroll run; transfers context; PayrollRunAgent executes; combined result returned | P2 |

---

## Coverage Summary

| Category | Count | IDs |
|----------|-------|-----|
| Model Gateway | 10 | MG-01 – MG-10 |
| RAG Pipeline | 10 | RAG-01 – RAG-10 |
| Agent Framework | 9 | AGT-01 – AGT-09 |
| Built-in Tools | 10 | TL-01 – TL-10 |
| Human-in-the-Loop | 6 | HITL-01 – HITL-06 |
| RAG Search | 4 | RGS-01 – RGS-04 |
| Cost Tracking | 4 | CT-01 – CT-04 |
| Security | 5 | SEC-01 – SEC-05 |
| End-to-End | 5 | E2E-01 – E2E-05 |
| **Total** | **57** | |

### Priority Distribution

| Priority | Count | Notes |
|----------|-------|-------|
| P0 | 38 | Core functionality, security, tenant isolation, human-in-the-loop |
| P1 | 16 | Important but deferred if blocked by P0; covers fallback, edge cases, escalation |
| P2 | 1 | Multi-agent handoff — nice-to-have, can be post-MVP |

### Boundary Pairing (mirroring Phase 1 methodology)

Every security boundary in `phase-4-spec.md` §12 has paired valid/invalid tests:

| Boundary | Happy | Failure |
|----------|-------|---------|
| Tenant isolation | SEC-01 (RLS works) | RAG-08 (cross-tenant search blocked) |
| PII protection | TL-02 (masked for employee) | SEC-02 (no PII in RAG chunks) |
| Tool authorization | TL-09 (denied correctly) | SEC-03 (unauthorized tool blocked) |
| Human approval | HITL-01 (approve works) | HITL-02 (reject works), HITL-03 (timeout works) |
| Circuit breaker | MG-06 (fallback activates) | MG-09 (invalid key rejected) |
| Rate limiting | MG-01 (within limit) | MG-05 (exceeded → 429) |
| Cost tracking | CT-01 (accurate tokens) | CT-04 (cross-tenant isolation) |
| Document ingestion | RAG-01/02/03 (all formats) | RAG-10 (empty result, no crash) |
| Agent lifecycle | AGT-01 (full cycle success) | AGT-03 (max cycles exceeded) |
| End-to-end | E2E-02 (run→approve→disburse) | E2E-04 (failed run recovery) |

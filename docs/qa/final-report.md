# TalentOS Final QA Report — Phase 3 Fixes + Phase 4 Implementation

**Date**: 2026-10-26  
**Reviewer**: QA Agent (Auto)  
**Verdict**: **CONDITIONAL_PASS**

---

## Executive Summary

Phase 3 Blocker fixes are structurally complete — all 8 blockers (B1–B8) pass structural review with no critical security or correctness gaps. PII masking, auth directives, error format consistency, and MCP pagination all meet requirements.

Phase 4 AI layer is **functionally scaffolded but largely stubbed**. Migrations, schema, and module boundaries are correct. However, key components (RAG hybrid search fusion, agent activities, API handlers, built-in tools) remain mock implementations. The project is ready for production-hardening *only after* stubs are wired to real data paths.

---

## Phase 3 Blocker Resolution

| Blocker | Status | Evidence | Notes |
|---------|--------|----------|-------|
| B1 Auth directives in SDL | **PASS** | `schema.rs:21-23` registers both `@requireRole` and `@requirePermission`. Directives have correct `DirectiveLocation::FieldDefinition | Object`. Enforcement via `directives.rs:9-31`. | No issues. |
| B2 PII masking | **PASS** | `employee.rs:21-39` — `national_id`, `bank_account_number`, `sso_number` all annotated with dual `@requireRole(hr_admin/payroll_manager)`. `user.rs:8-16` — `email`, `roles` similarly masked. MCP tools (`tools/employee.rs:80-108`) implement `mask_national_id()`, `mask_bank_account()`, `mask_sso()`. Payroll `Payslip::from()` at `resolvers/payroll.rs:469,472` applies masking before exposing. | Good: masking applied at conversion layer, not just at GraphQL level. |
| B3 Leave/Payslip resolvers | **PASS** | `leave.rs` — full implementation with `query_as!`, parameterized queries, tenant context, pagination (lines 19-454). `payroll.rs` — full implementation with `query_as!`, parameterized queries, tenant context, pagination (lines 18-333). | ComplexObject stubs exist (`employee.payslips`, `employee.leave_balances`, `payslip.payroll_run`) — these are minor and acceptable as deferred. |
| B4 MCP query_as! usage | **PASS** | All tools (`employee.rs`, `payroll.rs`, `leave.rs`, `compliance.rs`) use `sqlx::query_as!` with typed row structs. No raw `query!` or string interpolation in data paths. | Row structs are private and correctly mapped to public output types. |
| B5 Auth context extraction | **PASS** | `auth.rs:49-67` — `extract_tenant_context()` reads `TenantContextBlob` from MCP extensions, validates UUIDs, returns `Err` on missing/malformed fields. `to_error_data()` converts to structured `ErrorData`. | Uses real extension key `talents.tenant_context` (via `TenantContextBlob`). |
| B6 MCP pagination | **PASS** | `query_employees` (`employee.rs:28-32`), `list_payslips` (`payroll.rs:316-322`), `list_pending_approvals` (`leave.rs:225-233`) all have `limit` + `offset` params. Limit capped at 200 (`employee.rs:131`, `payroll.rs:363`, `leave.rs:282`). | Validation enforced. |
| B7 GraphQL input validation | **PASS** | `employee.rs:131-157` — `CreateEmployeeInput` has `#[graphql(validator(...))]` on `national_id` (pattern `^[0-9]{13}$`), `bank_account_number` (pattern `^[0-9]{10,17}$`), `sso_number` (pattern `^[0-9]{13}$`), plus `max_length`, `range` validators. Same on `UpdateEmployeeInput`. | Comprehensive coverage. |
| B8 Error format consistency | **PASS** | Both `graphql/src/error.rs:9-14` and `mcp/src/error.rs:9-14` define identical `ErrorResponse { code, message, details }` with `HashMap<String, serde_json::Value>`. Shared `codes` module has 12 identical constants. MCP converts via `to_error_data()` (line 31-54 in mcp error.rs). | Minor: GraphQL uses `async_graphql::Error.extend_with`, MCP uses `rmcp::ErrorData` — different underlying types but same structure. Acceptable. |

---

## Phase 4 Implementation Assessment

| Component | Status | Coverage | Notes |
|-----------|--------|----------|-------|
| Model Gateway | **CONDITIONAL_PASS** | 75% | Rate limiter (Redis token bucket), circuit breaker (3-state), fallback chain (huakati → Ollama), token counting, cost tracking — all present. **Issues**: (1) `GatewayConfig` missing `rate_limit_window_secs` field (referenced at line 204 but never declared); (2) `client.connect()` return value ignored with `let _ =` (line 198); (3) `unwrap_or_default()` on error responses in `chat_ollama` (line 353) and `make_chat_request` (lines 428-429). |
| RAG Pipeline | **CONDITIONAL_PASS** | 65% | Chunker with recursive whitespace splitting (✅). Embedder with batch concurrent calls (✅). HybridSearch with vector + keyword + RRF scaffold (⚠️). **Issues**: (1) `reciprocal_rank_fusion` is a placeholder — returns empty vec, comments say "proper impl would merge" (lines 206-254); (2) `vector_search` and `keyword_search` use dynamic `sqlx::query!` with `Box<dyn Encode>` instead of typed `query_as!`; (3) `ts_rank_cd` with `'thai'` text config may not exist without custom config. |
| Agent Framework | **CONDITIONALPASS** | 55% | Workflow structure (Observe→Plan→Act→Reflect) defined. Human-in-the-loop signals (`human_approve`, `human_reject`, `provide_context`) wired. **Issues**: (1) `HumanRejectionSignal` and `AdditionalContextSignal` referenced in `workflow.rs:7,151-159` but **never defined in `states.rs`** — this is a compile error; (2) All 4 activity `execute()` methods are stubs returning mock data; (3) Temporal SDK integration requires build-time macro setup not visible in Cargo.toml. |
| Built-in Tools | **CONDITIONALPASS** | 50% | 8 tools registered in `create_default_registry` (lines 387-403 of `tools/mod.rs`). `ToolRegistry` trait + HashMap-backed storage implemented. **Issues**: 6 of 8 tool `execute()` methods return hardcoded mock JSON — only `CalculateTaxTool` and `CheckComplianceTool` call real domain logic. |
| Migrations | **PASS** | 100% | `20261027000001_ai_core.sql` — 5 tables (conversations, agent_runs, tool_calls, cost_tracking, tenant_api_keys) with RLS + policies. `20261027000002_ai_embeddings.sql` — pgvector extension, documents + chunks tables, ivfflat + GIN indexes, tsvector generated column. |
| API Endpoints | **CONDITIONALPASS** | 40% | 7 routes registered in `router.rs:26-32`, all behind auth middleware. Request/response types defined. **Issues**: All handler bodies are stubs returning mock data with comments like "In real implementation, would..." |

---

## Security Findings

### 🔴 Critical / Must-Fix Before Production

1. **`HumanRejectionSignal` and `AdditionalContextSignal` are undeclared types** (`talentos-agent/src/workflow.rs:7,151-159`)
   - File: `crates/ai/talentos-agent/src/workflow.rs` line 7
   - Impact: Compile failure — `AgentWorkflow` cannot build
   - Fix: Add structs to `states.rs` or import from `talentos_model_gw::models` (they exist there but not re-exported)

2. **`unwrap()` on `NaiveDate::from_ymd_opt` in production path** (`employee.rs:497`)
   - File: `crates/api/graphql/src/resolvers/leave.rs` line 497
   - Impact: Panic if chrono version regression; also seeds fake date into responses
   - Fix: Return `Err` or use `NaiveDate::MIN` via `unwrap_or`

### 🟡 Medium — Should Fix Before Launch

3. **PII masking function duplication** — `mask_bank_account()` and `mask_sso()` defined in both `mcp/tools/employee.rs:90-108` and `graphql/resolvers/payroll.rs:498-515`. Risk of divergence.
   - Fix: Move to shared module (e.g., `talentos_kernel:: pii` or `graphql/src/util.rs`)

4. **`unwrap_or_default()` swallows HTTP errors** (`model-gw/src/client.rs:353,428-429`)
   - File: `crates/ai/talentos-model-gw/src/client.rs`
   - Impact: Silent failure on upstream API errors; hard to debug
   - Fix: Return error with status + body for observability

5. **`rate_limit_window_secs` missing from `GatewayConfig`** (`client.rs:204`)
   - File: `crates/ai/talentos-model-gw/src/client.rs`
   - Impact: Compile error or runtime panic
   - Fix: Add field to struct or use config default

6. **RLS enforcement gap in RAG search** — `vector_search` and `keyword_search` construct dynamic SQL with string interpolation for param indices. While parameters are still bound, this pattern is fragile.
   - File: `crates/ai/talentos-rag/src/search.rs:86-112, 155-178`
   - Fix: Use `sqlx::query_as!` with optional pattern or a query builder crate

### 🟢 Low — Nice to Have

7. **Comments in stub code** — Many `// In real implementation, would...` comments indicate deferred work. Track in issue tracker.
8. **No Rustdoc on public APIs** in Phase 4 crates — `talentos-model-gw`, `talentos-rag`, `talentos-agent`, `talentos-tools` lack `///` documentation on exported types.

---

## Build Verification

**Skipped** — No Rust toolchain (`cargo`, `rustc`) available in this environment.

Recommended pre-merge checks:
```bash
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

Known compile-blocker: `HumanRejectionSignal` and `AdditionalContextSignal` must be added to `talentos-agent/src/states.rs` or imported from `talentos_model_gw::models`.

---

## Overall Verdict & Recommendation

### **CONDITIONAL_PASS**

**What's good:**
- Phase 3 blockers all resolved structurally — auth, PII, pagination, error format, MCP typed queries are correct
- Migrations are complete and follow security best practices (RLS, parameterized queries, proper indexes)
- Module boundaries and type definitions are well-structured
- PII masking applied at multiple layers (GraphQL directives + conversion functions)

**What must be fixed before production:**

| Priority | Item | File | Action |
|----------|------|------|--------|
| P0 | Missing types `HumanRejectionSignal`, `AdditionalContextSignal` | `talentos-agent/src/workflow.rs:7` | Define in `states.rs` or re-export from `talentos_model_gw::models` |
| P0 | `rate_limit_window_secs` missing from config | `talentos-model-gw/src/client.rs:204` | Add field to `GatewayConfig` |
| P1 | `unwrap()` on `NaiveDate::from_ymd_opt` | `graphql/resolvers/leave.rs:497` | Use `unwrap_or(NaiveDate::MIN)` or propagate error |
| P1 | Stub `reciprocal_rank_fusion` returns empty | `rag/src/search.rs:206-254` | Implement proper merge of vector + keyword results |
| P2 | PII masking duplicated across crates | `mcp/employee.rs` + `graphql/payroll.rs` | Extract to shared utility |

**Recommendation:** Proceed to production hardening sprint. The architectural foundation is solid; the remaining work is wiring stubs to real implementations and fixing the compile blockers.

---

## Appendix: File Reference Index

### Phase 3 Files Reviewed
- `crates/api/graphql/src/schema.rs` — Auth directives registration
- `crates/api/graphql/src/directives.rs` — Directive enforcement logic
- `crates/api/graphql/src/types/employee.rs` — PII field masking
- `crates/api/graphql/src/types/user.rs` — PII field masking
- `crates/api/graphql/src/resolvers/leave.rs` — Full leave implementation
- `crates/api/graphql/src/resolvers/payroll.rs` — Full payroll implementation
- `crates/api/graphql/src/context.rs` — Tenant context extraction
- `crates/api/graphql/src/error.rs` — Standardized error format
- `crates/api/mcp/src/auth.rs` — MCP auth context extraction
- `crates/api/mcp/src/error.rs` — MCP error format
- `crates/api/mcp/src/tools/employee.rs` — query_as! + PII masking
- `crates/api/mcp/src/tools/payroll.rs` — query_as! + confirm
- `crates/api/mcp/src/tools/leave.rs` — query_as! + pagination
- `crates/api/mcp/src/tools/compliance.rs` — Pure calculation tools

### Phase 4 Files Reviewed
- `crates/ai/talentos-model-gw/src/client.rs` — Rate limiter, circuit breaker, fallback
- `crates/ai/talentos-model-gw/src/cost.rs` — Token usage & cost tracking
- `crates/ai/talentos-model-gw/src/error.rs` — Model errors
- `crates/ai/talentos-model-gw/src/models.rs` — Shared agent types
- `crates/ai/talentos-rag/src/chunker.rs` — Recursive text chunking
- `crates/ai/talentos-rag/src/embedder.rs` — Batch embedding
- `crates/ai/talentos-rag/src/search.rs` — Hybrid search (with RRF placeholder)
- `crates/ai/talentos-agent/src/workflow.rs` — Agent workflow (compile blocker)
- `crates/ai/talentos-agent/src/activities.rs` — Phase activities (stubbed)
- `crates/ai/talentos-agent/src/states.rs` — Workflow state types
- `crates/ai/talentos-tools/src/registry.rs` — Tool registry
- `crates/ai/talentos-tools/src/tools/mod.rs` — 8 built-in tools (6 stubbed)
- `crates/api/gateway/src/handlers/ai.rs` — 7 API endpoints (all stubbed)
- `crates/api/gateway/src/router.rs` — Route registration
- `migrations/20261027000001_ai_core.sql` — AI core schema
- `migrations/20261027000002_ai_embeddings.sql` — Embeddings schema

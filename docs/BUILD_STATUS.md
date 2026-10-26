# Build status — verified locally

Last verified: 2026-10-26 on Oracle Linux 9.8 (glibc 2.34), Rust 1.99.1, PostgreSQL 16 + pgvector.

## Gate results

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --all -- --check` | pass |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | pass (0 warnings) |
| Tests | `cargo test --workspace` | **92 passed**, 0 failed (7 DB-backed, 6 render, 5 leave, 6 wasm, 3 schema, 5 mcp, 3 memory DB, 6 RBAC) |
| Migrations | `psql -f migrations/*.sql` on a fresh DB | 9 files apply cleanly |

Tests by crate: kernel 5, authz 22, compliance 6, payroll 11, rag 7, agent 7,
payroll-runner 6 (real PostgreSQL + Chromium: payslips, totals, state machine,
template round-trip, PDF, stuck-run recovery), hr-runner 5 (leave day maths),
wasm-host 6 (real guest module: sandbox, capability refusal, tenant scoping),
graphql 3 (schema shape, money-as-string, RBAC gate), mcp 5 (version negotiation, tool list,
destructive guard, notifications), memory 6 (3 pure + 3 real-Postgres: recall,
RLS isolation, layer filter, expiry purge, stats), gateway 3 (RBAC gate:
hr_admin allowed, employee refused, manager cannot approve payroll).

Manual HTTP smoke test (local, real Postgres + Redis + Chromium): health → login →
`/auth/me` → `/ai/tools` → create payroll run → run reaches `pending_approval` with
2 payslips, total gross ฿99,000, SSO ฿1,450, PND ฿9,416.67 (hand-verified).
Payslip PDF: 77 KB, `%PDF-1.4`, Thai text round-trips through `pdftotext`.
Leave: create (3 days computed server-side) → overlap 409 → bad type 422 → balances → list.

## Compiling workspace members

```
crates/core/kernel          primitives: ids, Money, events, shared workflow types
crates/core/authz           JWT, Argon2id, RBAC matrix
crates/infra/db             TenantPool (SET LOCAL tenant GUC)
crates/domain/compliance    Thai PND tax, SSO, PF, LPA severance, OT
crates/domain/payroll       deterministic payroll DAG + anomaly rules
crates/ai/model-gw          huakati.online client, in-process limiter, circuit breaker
crates/ai/rag               chunker, embedder, hybrid search (RRF)
crates/ai/tools             agent tools (payroll/query/tax/anomaly/compliance + memory)
crates/ai/agent             Observe → Plan → Act → Reflect loop
crates/api/gateway          Axum: /health, /auth/*, /payroll/*, /mcp, /graphql
crates/api/mcp              MCP JSON-RPC over streamable HTTP (stateless, JWT per request)
```

## Web app

`web/` — Next.js 14.2.35, TypeScript strict, no UI dependency beyond React.
`npm run build` passes (10 routes). Browser never holds a token: `/api/session/login`
sets `HttpOnly` cookies and `/api/gw/*` proxies with a bearer header, refreshing
single-flight. Verified against the real gateway: login → `/auth/me` → GraphQL →
payslip template → HTML render → 77 KB PDF, and an expired access token rotates
on the next request (old refresh token then 401, as designed).

## Deferred — not wired

| Crate | Blocker |
|---|---|
| `crates/services/talentos-worker` | The scaffold targeted an API that does not exist in `temporalio-sdk`. SDK 1.1 has a clean `#[workflow]`/`#[activities]` surface (verified in the crate docs), but wiring it means the gateway must depend on the workflow definitions and a Temporal cluster must run in every deployment. Payroll already recovers abandoned runs (`recover_stuck_runs`), so this buys durability/visibility, not a feature. Rewrite the scaffold against 1.1, then decide the deployment default. |


Memory is **done** on pgvector: `crates/ai/talentos-memory` with four layers
(`working` = 1 h TTL by default, `episodic`, `semantic`, `procedural`), hybrid
recall (vector + keyword + importance + recency), tenant RLS, and
`remember_memory`/`recall_memory` tools exposed to the agent and MCP. Graph
edges/ontology concepts live in `metadata`; a real graph store is not built.

RBAC is **enforced per handler** (not just per token): payroll, payslips, employee
lists, leave, plugins, AI and memory each check `crates/core/authz`. Strix's
2026-10-26 "GraphQL order-by SQL injection" finding was inspected and is a false
positive — there is no user-controlled `ORDER BY`; the real gap it pointed at
(any authenticated user could read all payroll data) is now fixed and tested.
An `employee` token receives 403 on team-wide endpoints.

MCP is **done** at `POST /api/v1/mcp` (streamable HTTP, stateless). No session id,
because the JWT on each request is the only identity — one client can never resume
another tenant's session. Capability advertised: `tools` (the same registry the
agent uses). Destructive tools require `X-TalentOS-Allow-Destructive: true`.
`GET` returns 405 (no server-initiated stream) and `DELETE` 204.

GraphQL is **done** at `POST /api/v1/graphql` (queries: `health`, `me`, `employees`,
`payrollRuns`, `payslips`), executed inside a tenant transaction. Mutations stay on
REST until the workflow layer is unified.

Phase 5 (WASM plugins) is **done**: `crates/ext/talentos-wasm-host` + an example guest
(`plugins/bonus-calculator-thai`, built for `wasm32-wasip1`). Host enforces per-tenant
capability grants, memory/time limits, and injects tenant context only when granted.
Build the guest before running its tests:

```bash
cd plugins/bonus-calculator-thai && cargo build --release --target wasm32-wasip1
```

The agent loop currently runs in-process (`AgentRunner`) instead of on Temporal; the
phase boundary is already isolated, so the Temporal swap is a wiring change, not a rewrite.

## Known shortcuts (grep `shortcut:`)

- `to_tsvector('simple', ...)` for Thai keyword search — no Thai tokenizer installed yet.
- Payroll runs execute in-process (`tokio::spawn`) instead of on Temporal, so a process
  restart during a run leaves it in `calculating`.
- `POST /payroll/runs` requires `entity_id` in the body when the JWT is tenant-wide.
- Payslip PDF needs headless Chromium plus a Thai font at runtime (`CHROME_BIN`,
  installed in the Docker image). Without it the endpoint returns 503 rather than
  emitting a PDF with broken Thai glyphs.
- PDFs are cached on local disk (`PAYSLIP_STORE_DIR`), not MinIO yet.
- Only one default template per tenant is editable through the API (no per-entity or multiple named templates in the UI).
- In-process rate limiting in the model gateway; the distributed limiter lives in the API gateway.
- Single static HS256 signing key (no JWKS rotation yet).

# Build status — verified locally

Last verified: 2026-10-28 on Oracle Linux 9.8 (glibc 2.34), Rust 1.99.1, PostgreSQL 16 + pgvector.

## Gate results

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --all -- --check` | pass |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | pass (0 warnings) |
| Tests | `cargo test --workspace` | **292 passed**, 0 failed, 1 ignored (load test; real PostgreSQL, real WASM guest, Chromium PDF, RBAC) |
| Migrations | `sqlx migrate run` on a fresh DB | 32 files apply cleanly (including `20261109000001_expense_claims.sql` for expense claims) |

Unit tests by crate: kernel 5, authz 35 (TOTP RFC 6238 vectors and rejection windows), attendance 10 (shift derivation,
overnight, holiday/weekend OT, timezone), compliance 6, payroll 21, rag 7,
agent 7, hr-runner 10 (leave day maths, attendance break/grace validation, notification rendering), db 5
(PII cipher round-trip, wrong key, tamper, legacy plaintext, masking), graphql 3
(schema shape, money-as-string, RBAC gate), mcp 5 (version negotiation, tool list,
destructive guard, notifications), memory 3 (layers, expiry), gateway 49 (RBAC
gate, tenant provisioning CLI, CSV escaping/totals, holiday classification, shift
management, attendance import/punch/correction/approval, bulk employee import dry-run/duplicate/parse rules).

Integration tests: payroll-runner 14 (`execute_run` 9 + `attendance_scale` 1 + loans 4;
real PostgreSQL + Chromium: payslips, totals, state machine, template round-trip,
PDF, stuck-run recovery, attendance+unpaid-leave feed, tenant policy overrides,
30-employee attendance scale — 3 of the 30 employees are driven through
punch/correct/approve; the rest seed approved records directly), attendance 30
(approve/correct, import, punch, schema isolation, self, shifts), leave
self-service 14 (me/leave, notification outbox, cancel, approvals, balance debit/restore, multi-level chain, legacy
pending isolation, RBAC; service-level DB + handler-helper units — cancel/reject
have no browser E2E, inbox pagination is deferred), memory 3 (recall, RLS isolation, layer filter, expiry
purge, stats), claims 11 (own create/list/cancel, decisions, inbox, isolation), reports 8 (headcount, cost, OT trend, balanced/deterministic GL), leave balances 6 (dry-run, upsert, duplicates, bounds), leave approvals 12 (default one-step, manager→HR, role denial, override, skip, snapshot), notify 1 (payslip recipients and rendered body), OT self-service 13
(own create/list/cancel, duplicates, decisions, inbox, approved history, reason/minute bounds), wasm-host 6 (real guest module: sandbox, capability refusal,
tenant scoping). Total: 292 passed, 1 ignored (`load.rs`).

Manual HTTP smoke test (local, real Postgres + Redis + Chromium): health → login →
`/auth/me` → `/ai/tools` → create payroll run → run reaches `pending_approval` with
2 payslips, total gross ฿99,000, SSO ฿1,450, PND ฿9,416.67 (hand-verified).
Payslip PDF: 77 KB, `%PDF-1.4`, Thai text round-trips through `pdftotext`.
Leave: create (3 days computed server-side) → overlap 409 → bad type 422 → balances → list. Leave self-service flows are covered by the `leave_self` service tests (including
cancel/reject balance restore and decision) and the Playwright E2E suite
(request → HR approve); cancel and reject have no browser E2E.

## Compiling workspace members

```
crates/core/kernel          primitives: ids, Money, events, shared workflow types
crates/core/authz           JWT, Argon2id, RBAC matrix
crates/infra/db             TenantPool + AES-256-GCM PII cipher
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

## Identity, RLS and access

- Runtime uses `talentos_app` (`NOBYPASSRLS`); `talentos_auth` (`BYPASSRLS`) is
  used only for login/refresh and the stuck-run sweeper. Every query goes through
  `TenantPool::begin_tenant`.
- Switching to the app role exposed a latent bug: `current_tenant_id()` did
  `COALESCE(text, uuid)`, which never fired while a superuser skipped RLS. Fixed
  in `20261030000001_fix_current_tenant_id.sql`; tests now run as the app role.
- RBAC is enforced per handler and in GraphQL (`attendance.*`, `employee.*`,
  `payslip.*`, `payroll_run.*`, `settings.*`, `user_management.*`).
- Employee CRUD is soft-delete only. Invitations are hashed, single-use, 7 days.
  Password change revokes all refresh tokens.

## Web app

`web/` — Next.js 15.5.28 + React 19, TypeScript strict, no UI dependency beyond
React. `npm audit` reports 0 vulnerabilities; CI fails on high/critical in
production deps.
`npm run build` passes (21 routes). Browser never holds a token: `/api/session/login`
sets `HttpOnly` cookies and `/api/gw/*` proxies with a bearer header, refreshing
single-flight. Verified against the real gateway: login → `/auth/me` → GraphQL →
payslip template → HTML render → 77 KB PDF, and an expired access token rotates
on the next request (old refresh token then 401, as designed). The standalone
image (`web/Dockerfile`) was built and served `login` → session login → `/me`
against a live gateway.

Full compose stack was previously verified separately (isolated project +
volume): migrate → gateway → web → nginx with the TLS overlay. HTTPS `/health`
200, HTTP→HTTPS 301, web session login and `/api/gw/auth/me` 200, RLS enforced
(`talentos_app`), PII configured, 23 migrations tracked at that time, and
`talentos-gateway provision` created the first tenant through the compose
service. This task did not re-run the compose/TLS verification; the new leave
self-service, notifications, and TOTP were verified via API/DB tests and the
19/19 Playwright E2E suite.

Admin UI: `/settings` (organization, payroll/OT policy, system status with RLS +
PII warnings, TOTP 2FA enrollment) and `/users` (invite, roles, deactivate,
HR-issued password reset, 2FA reset).
Payroll runs export a UTF-8 CSV register (formula-injection safe) with totals.
Employees can be linked to a login and see only their own payslips
(`/me/payslips`) and request leave self-service (`/leave`). The audit trail has a
read API and `/audit` page. Attendance has a tenant holiday calendar; OT is
classified holiday > weekend > weekday automatically. HTTPS is a compose overlay
(`docker-compose.tls.yml`). UI restyled to the approved reference: rounded app
shell, light sidebar, serif display headings, black pill actions, lavender/lime
cards, self-hosted Inter/Instrument Serif/Noto Sans Thai, and a mobile layout.
Browser E2E (`web/e2e`, Playwright) passes 19/19 locally and runs in CI: invite
acceptance → login, settings + audit, employee → attendance → payroll → approval,
holiday OT, user invite, password reset, self-service payslips, employee clock
punch with GPS, HR shift creation and day correction/approval, and employee leave
request → HR approval, bulk CSV employee import, TOTP enroll/login/disable, LINE id save, OT request → HR approval (plus approved history), a two-step manager→HR leave chain, opening leave-balance CSV import, the reports/GL page, a loan/advance with payroll installment, and an expense claim with receipt approval. Payroll load test: 500 employees in ~0.38 s locally
(~1,300/s).

## Known risks

- Payroll executes in the gateway process with a stuck-run sweeper; that is
  recovery, not durable execution. Temporal remains unwired.
- PII encryption is only as safe as `PII_ENCRYPTION_KEY`; losing it loses the
  encrypted fields. Back the key up with the database.
- Payroll formula defaults are Thai practice (unpaid leave `base_salary / 30` per
  day, OT on `base_salary / 30 / 8`), but the divisor and OT multipliers are now
  tenant settings (`/settings`), so HR can adjust them without a deploy.
  Statutory SSO/tax remain in code.
- Holiday dates are tenant-managed (the cabinet announces them yearly); there is
  no bundled national calendar to go stale.
- Attendance GPS geofencing deters but does not cryptographically prove location;
  no face recognition / liveness check. No offline queue; punches require a live
  connection. Shift swaps and self-scheduling are not implemented.
- Leave self-service debits `used_days` when a request becomes pending and
  restores it on cancel/reject; approve keeps the debit. Legacy pending rows
  with `balance_debited=false` are not retroactively debited. Leave and payslip
  notifications are queued for email/LINE; delivery needs provider credentials.
- Expense claims are not reimbursed through payroll; receipts live on the local
  filesystem (back up `CLAIM_UPLOAD_DIR`) and are type-checked by header only.
- Loans have no interest, early payoff, or reversal of past installments; a
  cancelled loan keeps already-deducted amounts. GL credits 2230 for the
  installment but does not split the receivable ledger.
- GL export uses fixed account codes and department as the cost center;
  there is no per-tenant chart of accounts or accounting-system sync.
- Multi-level approvals cover sequential manager→HR chains only; delegation,
  SLA/escalation, per-type conditions, and OT chains are not built. Intermediate
  steps do not send notifications. GraphQL leave mutations are disabled; use REST.
- Payslip acknowledgement records the linked user and time; there is no e-signature
  or dispute workflow, and HR sees it only in the run payslip list.
- OT requests are an authorization trail only; approved minutes are not added to
  payroll. Payroll still uses attendance-derived OT, so actual punches remain the
  single money source.
- Notifications need `EMAIL_API_KEY`+`EMAIL_FROM` and/or `LINE_CHANNEL_TOKEN`;
  without them rows stay pending. Delivery is at-least-once and failed rows are
  terminal after 3 attempts. LINE needs a per-user id set on `/users`.
- TOTP 2FA covers HR admins; there are no backup codes, so recovery is an HR
  admin reset that clears the secret and revokes that user's refresh tokens.
  Codes are not single-use: a code can be replayed inside the ±1 step window.
  Enrollment requires `PII_ENCRYPTION_KEY`; setup returns 503 without it.
- Bulk employee import enforces the 2 MB / 1000-row limits in the browser only;
  direct API callers can send larger batches. PII fields are not part of the CSV
  and are added on the employee page after import.

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

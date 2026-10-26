# TalentOS

AI-native HR & payroll platform for Thai SMEs. Rust core, deterministic payroll,
on-prem first.

**Status:** core payroll works end-to-end; see [`docs/BUILD_STATUS.md`](docs/BUILD_STATUS.md)
for exactly what compiles, what is tested, and what is deferred.

## What works today

- **Auth**: login / refresh with rotating refresh tokens, Argon2id, JWT (HS256), RBAC matrix (4 roles, 20 permissions)
- **Tenancy**: PostgreSQL RLS (`FORCE`) on every tenant table; login uses a separate `BYPASSRLS` role
- **Payroll**: deterministic DAG (gross → OT → SSO → PND tax → PF → net → payslip) with a per-step audit trace
- **Thai compliance**: PND 2567 progressive brackets, SSO 5% capped at ฿15,000, PF, LPA §118 severance, OT 1.5×/2×/3×
- **Agent**: Observe → Plan → Act → Reflect loop over 8 HR tools (runs in-process; Temporal pending)
- **RAG**: chunker, embedder, hybrid search (pgvector + tsvector, RRF fusion)
- **API**: `/health`, `/api/v1/auth/*`, `/api/v1/payroll/*`, `/api/v1/leave/requests/:id/decision`, `/api/v1/ai/*`

## Quick start

```bash
make install      # rust toolchain, sqlx-cli, nextest
make infra-up     # postgres (pgvector), kafka, temporal, redis, minio
make db-reset     # drop + recreate + run migrations
make sqlx-prepare # offline query cache (needed for builds without a DB)
make ci           # fmt + clippy -D warnings + tests
```

Run the gateway:

```bash
export DATABASE_URL=postgres://talentos:talentos_dev_secret@localhost:5432/talentos
export DATABASE_AUTH_URL=postgres://talentos_auth:auth_dev_secret@localhost:5432/talentos
export REDIS_URL=redis://:redis_dev_secret@localhost:6379
export AUTH_JWT_SECRET=dev-secret-change-me-in-prod-minimum-32-bytes-okay!!
cargo run -p talentos-gateway
```

Optional, for the AI surface: `HUAKATI_API_KEY`, `HUAKATI_API_BASE`.

Payslip PDFs need headless Chromium and a Thai font: set `CHROME_BIN`
(default `chromium-browser`) and `PAYSLIP_STORE_DIR`. Without a renderer the
`/pdf` endpoint returns 503 instead of producing a broken file.

## Payroll flow

```
POST /api/v1/payroll/runs          {period_start, period_end, payment_date, entity_id?}
  → 201 {run_id, status: "draft"}   (execution starts in the background)
GET  /api/v1/payroll/runs/:id      → status: pending_approval + totals
GET  /api/v1/payroll/runs/:id/payslips
POST /api/v1/payroll/runs/:id/approve
GET  /api/v1/payroll/payslips/:id/render   → printable HTML (tenant template applied)
GET  /api/v1/payroll/payslips/:id/pdf      → PDF via headless Chromium (Thai shaping)
```

`entity_id` is required when the caller's JWT is tenant-wide rather than
entity-scoped.

## Leave

```
POST /api/v1/leave/requests              {leave_type, start_date, end_date, reason?, employee_id?}
GET  /api/v1/leave/balances?employee_id= &year=
GET  /api/v1/leave/requests?employee_id= &status=
POST /api/v1/leave/requests/:id/decision {approve: bool, reason?}
```

Days are always computed server-side (calendar days inclusive). A request that
overlaps an existing pending/approved leave is rejected with 409; exceeding the
provisioned balance returns 422. `employee_id` defaults to the caller's own
employee record.

## Custom payslip

Tenants brand their own payslip (name, logo URL, accent colour, paper size,
which sections appear, footer). Configuration only — no markup is stored, and
every interpolated value is HTML-escaped, so branding cannot inject script.

```
GET  /api/v1/payslip/template   → current template (defaults when unset)
PUT  /api/v1/payslip/template   → upsert; 422 with a reason on invalid config
GET  /api/v1/payroll/payslips/:id/render → the payslip as text/html
```

The editor UI lives in `demo/index.html` under **แบบฟอร์มสลิป**.

## WASM plugins

Tenant logic runs sandboxed: no filesystem, no network, 16 MiB and a 2 s timeout,
and the host refuses to load a plugin whose declared capabilities were not granted
to that tenant.

```
GET  /api/v1/plugins                        → installed plugins (never returns bytes)
POST /api/v1/plugins/:name/invoke           → {function, input} → {output}
```

Example guest: `plugins/bonus-calculator-thai` (Thai annual bonus, pure JSON in/out).

```bash
cd plugins/bonus-calculator-thai && cargo build --release --target wasm32-wasip1
```

## Layout

```
crates/core/kernel        ids, Money (Decimal), events, shared workflow types
crates/core/authz         JWT, Argon2id, RBAC
crates/infra/db           TenantPool (SET LOCAL app.current_tenant_id)
crates/domain/compliance  Thai statutory rules
crates/domain/payroll     pure payroll DAG
crates/services/payroll-runner  run executor (DB + DAG)
crates/ai/*               model gateway, RAG, agent loop, tools
crates/ext/wasm-host      sandboxed plugin host (Extism, capability grants)
crates/api/gateway        Axum HTTP surface
migrations/               schema + RLS
```

## Rules this codebase follows

1. Money is `rust_decimal::Decimal` — never `f32`/`f64`.
2. Every tenant query runs inside `TenantPool::begin_tenant` (RLS is FORCE-enabled).
3. No LLM inside the payroll calculation DAG.
4. `thiserror` for library errors, no `unwrap()` in production paths.
5. Irreversible agent tools require an explicit human approval.

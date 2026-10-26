# TalentOS

AI-native HR & payroll platform for Thai SMEs. Rust core, deterministic payroll,
on-prem first.

**Status:** core payroll works end-to-end; see [`docs/BUILD_STATUS.md`](docs/BUILD_STATUS.md)
for exactly what compiles, what is tested, and what is deferred.

## What works today

- **Auth**: login / refresh with rotating refresh tokens, Argon2id, JWT (HS256), RBAC matrix (4 roles, 20 permissions)
- **Tenancy**: PostgreSQL RLS (`FORCE`) on every tenant table; runtime uses a `NOBYPASSRLS` role
- **PII**: AES-256-GCM for national id / SSO number / bank account; API returns only last 4 digits
- **Onboarding**: `talentos-gateway provision` creates a tenant, entity and first `hr_admin`
- **Deploy**: one-command compose stack (migrate + gateway + web + nginx), backup/restore scripts
- **Payroll**: deterministic DAG (gross → OT → SSO → PND tax → PF → net → payslip) with a per-step audit trace
- **Thai compliance**: PND 2567 progressive brackets, SSO 5% capped at ฿15,000, PF, LPA §118 severance, OT 1.5×/2×/3×
- **Agent**: Observe → Plan → Act → Reflect loop over the HR/memory tools (runs in-process; Temporal pending)
- **RAG**: chunker, embedder, hybrid search (pgvector + tsvector, RRF fusion)
- **API**: `/health`, `/api/v1/auth/*`, `/api/v1/employees*`, `/api/v1/users*`,
  `/api/v1/attendance*`, `/api/v1/payroll/*`, `/api/v1/leave/*`,
  `/api/v1/settings/*`, `/api/v1/ai/*`, `/api/v1/plugins*`, `/api/v1/graphql`,
  `/api/v1/mcp`

## Quick start

```bash
make install      # rust toolchain, sqlx-cli, nextest
make infra-up     # postgres (pgvector), kafka, temporal, redis, minio
make db-reset     # drop + recreate + run migrations
make sqlx-prepare # offline query cache (needed for builds without a DB)
make ci           # fmt + clippy -D warnings + tests
```

Run the gateway as the **app role** — `talentos_app` is `NOBYPASSRLS`, so RLS
really applies. Migrations use the owner role:

```bash
# migrations (owner)
export DATABASE_URL=postgres://talentos:talentos_dev_secret@localhost:5432/talentos
sqlx migrate run

# runtime (app role)
export DATABASE_URL=postgres://talentos_app:app_dev_secret@localhost:5432/talentos
export DATABASE_AUTH_URL=postgres://talentos_auth:auth_dev_secret@localhost:5432/talentos
export REDIS_URL=redis://:redis_dev_secret@localhost:6379
export AUTH_JWT_SECRET=dev-secret-change-me-in-prod-minimum-32-bytes-okay!!
cargo run -p talentos-gateway
```

`scripts/init-db.sql` creates both roles. Using the owner at runtime silently
disables every tenant policy — never do it.

### Create the first tenant

Provisioning crosses tenants, so it is a CLI (owner connection), not an API:

```bash
export ADMIN_PASSWORD='choose-a-strong-one'   # >= 12 chars
export DATABASE_ADMIN_URL=postgres://talentos:talentos_dev_secret@localhost:5432/talentos
cargo run -p talentos-gateway -- provision \
  --slug acme --name "ACME Co" --email admin@acme.co \
  --entity "ACME Main" --tax-id 0105555555555
```

It creates the tenant, one entity, an `hr_admin` user and an audit entry, then
prints the ids. Run it twice with the same slug and it fails (no silent reuse).

### PII at rest

`national_id`, `sso_number` and `bank_account_number` are encrypted with
AES-256-GCM before insert and are returned only as `****1234`.

```bash
export PII_ENCRYPTION_KEY=$(openssl rand -hex 32)   # keep it in your secret store
```

If the key is missing the gateway warns and stores new values as plaintext
(dev only). For rows written before encryption was enabled:

```bash
DATABASE_ADMIN_URL=... PII_ENCRYPTION_KEY=... \
  cargo run -p talentos-gateway -- pii-encrypt
```

Losing the key means losing the PII (the rest of the employee record still
works); back it up with the database.

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

## Design

The dashboard uses a sage/teal palette on a soft gray canvas: one rounded app
shell, a top pill navigation, mint stat cards, a deep-teal payment summary rail
and green primary actions. Headings use a display serif; numbers stay tabular.

- Tokens live in `web/app/globals.css` (`--page`, `--sage`, `--teal`, `--mint`,
  `--radius`, spacing).
- Fonts are self-hosted in `web/public/fonts` (Inter, Instrument Serif for
  display, Noto Sans Thai) so on-prem installs never call a CDN.
- Responsive: below 1000px the top bar wraps into brand + pills, stat cards go
  2×2, the right rail stacks, and wide tables scroll inside their card.

## Deploy (on-prem)

```bash
cp .env.example .env
# set at minimum: PG_PASSWORD, APP_DB_PASSWORD, AUTH_DB_PASSWORD,
#                 AUTH_JWT_SECRET (>= 32 bytes), PII_ENCRYPTION_KEY
make up                 # migrate + roles, then gateway + web + nginx
open http://localhost   # nginx -> web:3000, /api/* and /health -> gateway:8080
```

- `make migrate` runs the embedded migrations as the owner and (re)creates the
  runtime roles with `APP_DB_PASSWORD` / `AUTH_DB_PASSWORD`. Safe to re-run.
- The gateway and web ports bind to `127.0.0.1` only; nginx is the public entry.
  Terminate TLS in front of nginx (or add certs to `nginx.conf`).
- `make backup` writes a gzip dump to `./backups` (14-day retention);
  `make restore FILE=...` stops the app, restores and restarts.
- `./scripts/deploy.sh` = migrate → rebuild → health check. `--rollback` starts
  the previous gateway/web images.

## Web app

`web/` is a Next.js 14 app (Thai-first, sage/teal dashboard). It never
talks to the gateway from the browser: a route handler proxies `/api/gw/*` and
keeps both tokens in `HttpOnly` cookies, rotating the refresh token single-flight
so concurrent requests cannot trigger family revocation.

Pages: login, dashboard (headcount + latest run), employees, payroll runs
(create / poll / payslips / approve / PDF), payslip template editor with a live
preview of a real payslip.

```bash
cd web
cp .env.example .env.local        # TALENTOS_API_URL=http://127.0.0.1:8080
npm install
npm run dev                       # http://localhost:3000
```

Production image: `docker build -f web/Dockerfile -t talentos/web web` (Next
standalone). Set `TALENTOS_COOKIE_SECURE=false` only when serving plain HTTP on a
trusted network.

## People and users

```
GET/POST   /api/v1/employees            list / create
GET/PUT/DELETE /api/v1/employees/:id    detail / update / soft-terminate
GET        /api/v1/users                accounts + roles
POST       /api/v1/users/invite         one-time invite link (no email service)
POST       /api/v1/auth/accept-invite   public: token + new password
POST       /api/v1/auth/change-password own password, revokes refresh tokens
POST       /api/v1/users/:id/roles      add a role
DELETE     /api/v1/users/:id            deactivate
```

Invitations are hashed, single-use and expire in 7 days. HR copies
`/invite/<token>` to the new user; the page sets a password (12+ chars).

## Attendance → payroll

```
POST /api/v1/attendance      {employee_id, work_date, ot_hours, ot_kind}
GET  /api/v1/attendance?from=&to=&employee_id=
```

Payroll reads the period's OT hours by class (weekday 1.5x, rest day 2x, public
holiday 3x) and approved `unpaid` leave. Unpaid leave deducts `base_salary / 30`
per day; OT is priced on `base_salary / 30 / 8`, unaffected by that deduction.
SSO follows the explicit `sso_enrolled` flag, independent of provident fund.

## Settings (admin UI)

`/settings` in the web app covers:

- **องค์กร** — tenant name and each legal entity's name/tax id.
- **สูตรเงินเดือน/OT** — unpaid-leave divisor (default 30) and OT multipliers
  (1.5 / 2 / 3). Stored in `tenant_payroll_settings` and used by the next payroll
  run; statutory SSO/tax rates are not editable.
- **สถานะระบบ** — the DB role and whether RLS is actually enforced, migration
  tracking, Chromium availability and AI gateway configuration.

```
GET/PUT /api/v1/settings/payroll
GET/PUT /api/v1/settings/organization, /settings/tenant, /settings/entities/:id
GET     /api/v1/settings/system
```

The system card turns red if the process is running as a superuser/BYPASSRLS
role, which is the one misconfiguration that silently disables tenant isolation.

## E2E

Browser tests (Playwright) live in `web/e2e` and run in CI against a real
gateway + Postgres + Redis. They cover invite acceptance/login, settings
persistence, employee creation, attendance → payroll → approval, and user invite.

```bash
docker compose exec -T -e PGPASSWORD=talentos_dev_secret postgres \
  psql -U talentos -d talentos -f - < scripts/seed-e2e.sql
cd web
npx playwright install chromium      # first run
npx playwright test                  # E2E_BASE_URL defaults to :3100
```

## Authorization

Tenant isolation is enforced by Postgres RLS; authorization is enforced per
handler against the RBAC matrix (`crates/core/authz`). A valid token alone is not
enough: payroll/payslip/employee reads require the matching role, and an
`employee` token gets `403` on team-wide endpoints (verified over HTTP). GraphQL
resolvers apply the same checks.

## Memory

Agents remember across runs without an external service. One pgvector table,
four layers (`working`, `episodic`, `semantic`, `procedural`), tenant-scoped by
RLS. Recall is hybrid: embedding similarity when an embedding key is configured,
plus Thai keyword match, importance and 30-day recency decay.

```
POST   /api/v1/ai/memory          {layer, content, importance?, ttl_seconds?, metadata?}
GET    /api/v1/ai/memory?q=…&layers=episodic,semantic&limit=10
GET    /api/v1/ai/memory/stats
DELETE /api/v1/ai/memory/expired
```

The agent and MCP clients get `remember_memory` / `recall_memory` tools.

## MCP

`POST /api/v1/mcp` speaks JSON-RPC 2.0 (Model Context Protocol) over streamable
HTTP. Stateless: the bearer token on every request is the tenant. Advertises the
same tools the agent loop uses.

```bash
curl -s localhost:8080/api/v1/mcp -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

Destructive tools (payroll runs) are refused unless the request also sends
`X-TalentOS-Allow-Destructive: true`. Without `HUAKATI_API_KEY`, the three
LLM-backed tools are not registered; the rest keep working.

## GraphQL

`POST /api/v1/graphql`, `/api/v1/mcp` (JWT required). Read-only queries: `health`, `me`,
`employees(limit, offset)`, `payrollRuns(limit)`, `payslips(runId)`.
Money fields are `String` on purpose — a `Float` would lose satang precision.

```graphql
{ me { roles permissions } employees(limit: 5) { employeeCode baseSalary } }
```

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
crates/ai/memory           agent memory on pgvector (4 layers, tenant-scoped)
crates/ext/wasm-host      sandboxed plugin host (Extism, capability grants)
crates/api/gateway        Axum HTTP surface
web/                      Next.js 14 dashboard (Thai UI)
migrations/               schema + RLS
```

## Rules this codebase follows

1. Money is `rust_decimal::Decimal` — never `f32`/`f64`.
2. Every tenant query runs inside `TenantPool::begin_tenant` (RLS is FORCE-enabled).
3. No LLM inside the payroll calculation DAG.
4. `thiserror` for library errors, no `unwrap()` in production paths.
5. Irreversible agent tools require an explicit human approval.

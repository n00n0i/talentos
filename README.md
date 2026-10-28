# TalentOS

AI-native HR & payroll platform for Thai SMEs. Rust core, deterministic payroll,
on-prem first.

**Status:** core payroll works end-to-end; see [`docs/BUILD_STATUS.md`](docs/BUILD_STATUS.md)
for exactly what compiles, what is tested, and what is deferred.

## What works today

- **Auth**: login / refresh with rotating refresh tokens, Argon2id, JWT (HS256), optional TOTP 2FA for HR admins, RBAC matrix (4 roles)
- **Tenancy**: PostgreSQL RLS (`FORCE`) on every tenant table; runtime uses a `NOBYPASSRLS` role
- **PII**: AES-256-GCM for national id / SSO number / bank account; API returns only last 4 digits
- **Onboarding**: `talentos-gateway provision` creates a tenant, entity and first `hr_admin`
- **Deploy**: one-command compose stack (migrate + gateway + web + nginx), backup/restore scripts
- **Payroll**: deterministic DAG (gross → OT → SSO → PND tax → PF → net → payslip) with a per-step audit trace
- **Thai compliance**: PND 2567 progressive brackets, SSO 5% capped at ฿15,000, PF, LPA §118 severance, OT 1.5×/2×/3×
- **Attendance**: mobile clock page with GPS geofence, shifts + assignments, device JSON import, HR corrections/approval, holiday/weekend/weekday OT classification
- **Agent**: Observe → Plan → Act → Reflect loop over the HR/memory tools (runs in-process; Temporal pending)
- **RAG**: chunker, embedder, hybrid search (pgvector + tsvector, RRF fusion)
- **API**: `/health`, `/api/v1/auth/*`, `/api/v1/employees*`, `/api/v1/users*`,
  `/api/v1/attendance*`, `/api/v1/payroll/*`, `/api/v1/leave/*`,
  `/api/v1/settings/*`, `/api/v1/me/*`, `/api/v1/audit`,
  `/api/v1/ai/*`, `/api/v1/plugins*`, `/api/v1/graphql`, `/api/v1/mcp`

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

If `APP_ENV=production`, a missing key (or the development JWT secret) makes the
gateway refuse to start. In development it warns and stores plaintext. For rows written before encryption was enabled:

```bash
DATABASE_ADMIN_URL=... PII_ENCRYPTION_KEY=... \
  cargo run -p talentos-gateway -- pii-encrypt
```

Losing the key means losing the PII (the rest of the employee record still
works); back it up with the database.

Optional, for the AI surface: `HUAKATI_API_KEY`, `HUAKATI_API_BASE`.

### Notifications

Leave submissions/decisions and approved payroll runs write rows to the
`notifications` outbox in the same transaction as the change. A gateway worker
delivers them:

```bash
export EMAIL_API_KEY=...            # Resend-compatible HTTP API
export EMAIL_FROM=hr@example.com
export LINE_CHANNEL_TOKEN=...       # LINE Messaging API push
# EMAIL_API_BASE / LINE_API_BASE override the provider host (tests use a stub).
```

Email always goes to the user's address; LINE only when the user has a
`line_user_id` (set per user on `/users`). With no credentials the worker is
idle and rows stay `pending`; a send failure is retried up to 3 times before
`failed`. Delivery is at-least-once, so a crash after a send can duplicate one
message. Invitations and password resets remain copy-the-link flows.

Payslip PDFs need headless Chromium and a Thai font: set `CHROME_BIN`
(default `chromium-browser`) and `PAYSLIP_STORE_DIR`. Without a renderer the
`/pdf` endpoint returns 503 instead of producing a broken file.

## Payroll flow

```
POST /api/v1/payroll/runs          {period_start, period_end, payment_date, entity_id?}
  → 201 {run_id, status: "draft"}   (execution starts in the background)
GET  /api/v1/payroll/runs/:id      → status: pending_approval + totals
GET  /api/v1/payroll/runs/:id/payslips
GET  /api/v1/payroll/runs/:id/export.csv  → UTF-8 CSV register + totals
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
GET  /api/v1/me/leave
POST /api/v1/me/leave/:id/cancel
GET  /api/v1/leave/approvals
```

Days are always computed server-side (calendar days inclusive). A request that
overlaps an existing pending/approved leave is rejected with 409; exceeding the
provisioned balance returns 422. `employee_id` defaults to the caller's own
employee record.

Employee self-service lives at `/leave`: `GET /api/v1/me/leave` returns the
linked employee's current-year balances and request history, or an empty result
when no employee is linked. Employees cancel their own pending request with
`POST /api/v1/me/leave/:id/cancel`; only `pending` requests can be cancelled.
HR/managers use `GET /api/v1/leave/approvals` as the pending inbox and
`POST /api/v1/leave/requests/:id/decision` to approve or reject.

GraphQL leave mutations (`requestLeave`, `approveLeaveRequest`, `rejectLeaveRequest`,
`cancelLeaveRequest`) now call the same service layer as REST, so chains and
balances apply on both surfaces.

Leave approvals can be one step (HR) or a sequential chain
`manager → hr_admin`, configured in `/settings` (`leave_approval_chain`). The
chain is snapshotted onto each request; the current step's role decides, and an
HR admin may override. Rejection at any step is final; intermediate steps send
no notification. GraphQL leave mutations are also available for the same request lifecycle.

History is paginated (`limit`/`offset`, default 50, pending first); the page
loads more with a button instead of silently stopping at 200.

Balance accounting is tied to status transitions: a pending request debits
`used_days`, cancel or reject restores it, and approve keeps it debited. Legacy
pending rows created before the `balance_debited` flag was introduced are left
untouched. The `/leave` page and the available leave types are driven by the
employee's `leave_balances` for the current year.

### Opening balances

HR imports opening leave balances from `/leave` or
`POST /api/v1/leave/balances/import` with `{dry_run, rows}`:
`employee_code, leave_type, year, total_days, used_days?`. Dry run reports
per-row errors; a commit upserts every row in one transaction or writes nothing
when any row is invalid. Re-importing the same key overwrites the quota.

## OT self-service

```
GET  /api/v1/me/ot                       own requests
POST /api/v1/me/ot                       {work_date, minutes, reason?}
POST /api/v1/me/ot/:id/cancel            pending only
GET  /api/v1/ot/approvals                HR/manager pending inbox
POST /api/v1/ot/requests/:id/decision    {approve}
```

One live request per employee per date; rejected/cancelled frees the date.
Minutes are 1..720 and the `/ot` page takes hours in 0.5 steps. An approved
request raises that attendance day's effective OT to
`max(punch-derived OT, approved request minutes)` — never adds, so it cannot be
paid twice. A day already approved rejects a request that would change its OT;
reject/cancel restores the derived value.

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

## Final settlement

HR computes a draft from `/final-settlement` or
`POST /api/v1/employees/:id/final-settlement`:
last-month pay prorated to the termination day, unused leave days, notice days,
and statutory LPA §118 severance (completed calendar years). The breakdown is
stored as a draft; tax/SSO withholding and the official 50 ทวิ/SSO forms remain
with payroll/Phase 0. This does not create or change a payroll run.

## Expense claims

Employees submit claims at `/claims` (category, amount, date, description,
receipt). Receipts are JPEG/PNG/PDF up to 5 MB, stored under
`CLAIM_UPLOAD_DIR` with a server-generated name and served only to the owner or
HR. HR approves/rejects from the same page. Approved claims in the next payroll
period are added to net pay as a non-taxable reimbursement and marked “จ่ายแล้ว”;
the payslip and GL show the amount.

### Notification outbox

`/settings` shows pending/failed/sent counts and failed rows (masked recipients)
with a retry button. `GET /api/v1/notifications` and
`POST /api/v1/notifications/:id/retry` are hr_admin-only; retry resets a failed
row to pending for the worker.

## Loans and advances

HR records a loan/advance at `/loans` (one active loan per employee):
`POST /api/v1/loans {employee_id, principal, installment_amount, start_date, note?}`.
Each payroll run deducts `min(installment, balance)` after SSO/tax/PF, records a
`loan_installments` row and reduces the balance in the same transaction; reruns
are idempotent per run and the loan closes at zero. Employees see their loan at
`GET /api/v1/me/loans`; the payslip shows the deduction line. There is no
interest, early payoff, or reversal of past installments.

## Provident fund

Membership is the employee `pf_member` toggle (5% employee/employer in the DAG).
An approved payroll run exports `GET /api/v1/payroll/runs/:id/pf-remittance.csv`
with per-employee and employer amounts plus totals, deterministic and
CSV-injection safe. `/payroll` links it next to the GL export.

## Reports and GL

`/reports` (HR/payroll) shows headcount by department, payroll cost by
department for a period, and the monthly OT trend. An approved payroll run
exports a deterministic journal at
`GET /api/v1/payroll/runs/:id/export.gl.csv`:

```
account_code,account_name,department,debit,credit,description
5100 ค่าแรง                    debit  gross + employer SSO/PF
2100 เงินเดือนค้างจ่าย          credit net
2200 ภาษีหัก ณ ที่จ่าย          credit tax
2210 ประกันสังคม               credit employee + employer SSO
2220 กองทุนสำรองเลี้ยงชีพ       credit employee + employer PF
2230 เงินให้กู้พนักงาน           credit loan deduction
5200 ค่าใช้จ่ายพนักงาน          debit  reimbursement
```

The seven account codes are tenant-configurable under **ตั้งค่า → รหัสบัญชี GL**
(`GET/PUT /api/v1/settings/payroll`, non-empty, ≤20 alphanumeric chars); the
table above shows the defaults, so tenants without a settings row keep the
classic mapping. Each journal row is grouped by `COALESCE(cost_center,
department, 'ไม่ระบุ')` — an employee's **cost center** (`employees.cost_center`,
set on the employee form) wins, falling back to their department. Payroll-cost
reports group by the same value; headcount stays by department. Account names
and text fields remain Thai and CSV-injection safe. The CSV export is the only
supported format; Excel/PDF reports are out of scope.

## Deploy (on-prem)

```bash
cp .env.example .env
# set at minimum: PG_PASSWORD, APP_DB_PASSWORD, AUTH_DB_PASSWORD,
#                 AUTH_JWT_SECRET (>= 32 bytes), PII_ENCRYPTION_KEY
make up                 # migrate + roles, then gateway + web + nginx
open http://localhost   # nginx -> web:3000, /api/* and /health -> gateway:8080
```

- `make migrate` runs the embedded migrations as the owner and (re)creates or
  updates the runtime roles from `APP_DB_PASSWORD` / `AUTH_DB_PASSWORD`. Safe to
  re-run; changing the env password rotates the DB role password.
- The gateway and web ports bind to `127.0.0.1` only; nginx is the public entry.
  Terminate TLS in front of nginx (or add certs to `nginx.conf`).
- `make backup` writes a gzip dump to `./backups` (14-day retention);
  `make restore FILE=...` stops the app, restores and restarts.
- `./scripts/deploy.sh` = migrate → rebuild → health check. `--rollback` starts
  the previous gateway/web images.
- **HTTPS:** `./scripts/selfsigned-cert.sh` (testing) or drop real certs into
  `./certs`, then `docker compose -f docker-compose.yml -f docker-compose.tls.yml
  up -d --build gateway web nginx`. Set `APP_URL=https://…` and
  `TALENTOS_COOKIE_SECURE=true`. Port 80 redirects to 443.
- The embedded migrator keeps a `_sqlx_migrations` ledger. A database created by
  the old hand-run SQL (before this ledger existed) cannot be upgraded in place;
  recreate it with `make db-reset` on dev, or baseline it before the first
  production deploy. Fresh installs need nothing.

## Web app

`web/` is a Next.js 15 app (React 19, Thai-first, sage/teal dashboard). It never
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
POST       /api/v1/employees/import     CSV rows, {dry_run, rows} → per-row report
GET/PUT/DELETE /api/v1/employees/:id    detail / update / soft-terminate
GET        /api/v1/users                accounts + roles
POST       /api/v1/users/invite         one-time invite link (no email service)
POST       /api/v1/auth/accept-invite   public: token + new password
POST       /api/v1/auth/change-password own password, revokes refresh tokens
POST       /api/v1/auth/2fa/setup       issue a TOTP secret (needs PII key)
POST       /api/v1/auth/2fa/enable      {code} confirm and enable
POST       /api/v1/auth/2fa/disable     {code} turn off, revokes refresh tokens
POST       /api/v1/users/:id/2fa/reset  hr_admin: recover a locked-out user
POST       /api/v1/users/:id/roles      add a role
POST       /api/v1/users/:id/reset-password  issue a 1-hour reset link
POST       /api/v1/auth/reset-password  public: token + new password
DELETE     /api/v1/users/:id            deactivate
```

Invitations are hashed, single-use and expire in 7 days. HR copies
`/invite/<token>` to the new user; the page sets a password (12+ chars).
Password resets work the same way: HR issues `/reset/<token>` (1 hour, single
use) and the user sets a new password; every refresh token for that user is
revoked.

`POST /api/v1/employees/import` takes `{dry_run, rows}` where each row carries
the CSV text fields (`employee_code`, Thai first/last name, position, salary,
hire date, and optional department/English names/tax/SSO/PF flags). A dry run
validates only; a commit inserts every row in one transaction or nothing when
any row is invalid. The `/employees` page parses the CSV in the browser, shows
the per-row report, and enables the commit only when the file is clean.
Duplicate codes (in the file or per entity) and foreign entities are reported
on the row. PII (national id, bank account, SSO number) is not imported in v1.
Bulk CSV import does not set `cost_center` — imported employees fall back to
their department in GL grouping until an HR admin sets a cost center on the
employee form.

HR admins can enroll an authenticator app from `/settings` (RFC 6238, 6 digits,
30 s, ±1 step). The secret is shown once and stored AES-256-GCM-encrypted; logins
for an enabled user require `totp_code` and return `totp_required` /
`invalid_totp` otherwise. Enabling returns 10 one-time recovery codes (hashed at
rest, shown once); regenerate them with a current TOTP. `/settings` also lists
active refresh sessions with IP/device and can revoke one or all. Another HR
admin can reset a lost device from `/users`.

An employee can be linked to a login account from the employee form. That user
then sees only their own payslips at `/me/payslips`
(`GET /api/v1/me/payslips`, plus `/render`, `/pdf`, and `/acknowledge`);
ownership is checked
against `employees.user_id`, so `payslip.view_self` never exposes a colleague's
slip. Employees confirm receipt with `POST /api/v1/me/payslips/:id/acknowledge`
(idempotent); HR sees the acknowledgement time in the payroll run payslip list.
The payslip-ready notification links to `/me/payslips`.

`GET /api/v1/audit` (permission `audit_log.read`) lists the tenant audit trail;
the admin UI is at `/audit`.

## Attendance

Clock in/out from the mobile-friendly `/clock` page. The browser records GPS at
punch time and rejects the punch when the device is outside the entity geofence.
Because the browser can only read location over a secure origin, geofencing
requires HTTPS in production (use the TLS compose overlay or terminate TLS in
front of nginx).

```
POST /api/v1/me/attendance/punch   {kind: "in"|"out", lat, lng, accuracy_m?}   lat/lng required
GET  /api/v1/me/attendance?month=YYYY-MM   own derived days
GET  /api/v1/attendance/days?from=&to=&employee_id=&status=   HR derived days
```

Every punch is stored with the Bangkok `work_date` it belongs to, and the day is
recomputed from the events of that date. An out punch closes the employee's open
in from the previous 16 hours, so early arrivals, late leaves and overnight shifts
stay on the day they started. Worked time deducts the shift break only when the
stint is longer than 5 hours (LPA §27). Weekday OT is the time worked **after the
shift ends** (early arrival is not OT); holiday/weekend time is all OT.

Shifts define the expected window and break. Assign a shift to an employee from
`/attendance/shifts` or via `POST /api/v1/shifts/:id/assign`. The entity geofence
is set with `PUT /api/v1/settings/entities/:id/geofence {lat, lng, radius_m}`
(radius 50–5000 m).

```
GET/POST   /api/v1/shifts
POST       /api/v1/shifts/:id/assign          {employee_id, effective_from}
PUT        /api/v1/settings/entities/:id/geofence
GET/POST   /api/v1/attendance/holidays
DELETE     /api/v1/attendance/holidays/:id
```

HR works on `/attendance` (Bangkok month picker; filter by pending / incomplete /
late / early leave):

```
POST /api/v1/attendance/days/:id/correct   {in_at, out_at, note?}   RFC 3339 instants
POST /api/v1/attendance/days/:id/approve
```

A correction replaces the day's events with one manual in/out pair and resets the
day to `pending`. **Only approved days feed payroll.** Approval writes the matching
`attendance_records` row; any change that re-opens an approved day (a new punch, a
correction, or a device import) resets it to `pending`, clears the approver and
**deletes that payroll row** until HR approves again. Incomplete days (an `in`
without an `out`) cannot be approved (422); correct them first.

Device rows are imported as JSON (no CSV/multipart upload):

```
POST /api/v1/attendance/import   {"rows": [{"employee_code": "E001", "date": "2026-10-27", "in": "09:00", "out": "18:00"}]}
```

Times are Bangkok `HH:MM`. For an overnight shift an `out` at or before `in` lands
on the next calendar day while the row's `date` stays the work date. Each row is
validated on its own; the response reports `imported`, `skipped` and per-row
`errors`. Days that already have mobile punches are skipped (mobile is
authoritative); re-importing replaces earlier manual/import events.

Legacy raw records (`GET/POST /api/v1/attendance`, `DELETE /api/v1/attendance/:id`)
remain only for migration/backfill of historical hours; the UI no longer exposes
them and derived approval is the supported path into payroll.

### Location data retention

Each mobile punch stores GPS-derived `distance_m` and `accuracy_m` (not the raw
coordinates). Attendance events are kept 24 months by default and then purged by
an operator script or cron job (for example
`DELETE FROM attendance_events WHERE occurred_at < now() - interval '24 months'`
run with the owner role). There is no automatic purge job yet.

### Attendance → payroll

Payroll reads `attendance_records` OT hours by class inside the period (rows exist only for approved days),
plus approved `unpaid` leave. Unpaid leave deducts `base_salary / 30` per day; OT
is priced on `base_salary / 30 / 8`, unaffected by that deduction. SSO follows the
explicit `sso_enrolled` flag, independent of provident fund.

The OT class is automatic: a date in the tenant holiday calendar is `holiday`
(3x), weekends are `weekend` (2x), everything else `weekday` (1.5x). HR can still
override the class on a legacy record. The 30-employee acceptance fixture in
`talentos-payroll-runner` drives three employees (weekday, weekend, holiday)
through `punch_at` / `correct_day` / `approve_day` and seeds the other 27 approved
records directly to exercise the payroll boundary at scale.

### Attendance v1 limits

- GPS geofencing deters buddy-punching from outside the site, but it does not
  cryptographically prove location; a motivated user can spoof coordinates.
- No face recognition / liveness check.
- No offline queue: punches require a live connection. If the request fails, the
  employee must retry.
- No shift swaps or self-scheduling; HR assigns shifts.
- Holiday/weekend OT policy: all hours on a holiday or weekend are classified as
  OT (holiday 3x, weekend 2x). There is no separate "holiday pay" multiplier
  beyond the OT class.
- Only approved days feed payroll; pending and incomplete days are ignored.

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
gateway + Postgres + Redis. They cover invite acceptance/login, settings and
audit, employee creation, attendance → payroll → approval, holiday OT
classification, user invite, password reset, and self-service payslips.

Payroll throughput check (not in the normal suite):

```bash
LOAD_EMPLOYEES=500 cargo test -p talentos-payroll-runner --test load \
  -- --ignored --nocapture
# local: 500 employees in ~0.38s (~1,300 employees/s)
```

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

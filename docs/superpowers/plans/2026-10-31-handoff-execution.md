# Plan: execute remaining TalentOS handoff items

Spec: `handoff.md` (repo root) §8, §9, §10 — user opted out of brainstorming
("ไล่ทำไปให้เสร็จเลย"), continuous execution approved. Baseline confirmed
2026-10-31: `cargo test --workspace` = 451 passed / 0 failed / 1 ignored
(load). No OAuth credentials exist → SSO task is scope-reduced (see Ruling R1).

Worktree: `.worktrees/handoff-execution` (branch `handoff-execution`, cut from `1f32578`).

## Global Constraints (from handoff §6 — binding for every task)

- RLS: services use `pool.begin_tenant(tenant_id)`; every query carries `tenant_id = $1`.
- Money: `rust_decimal::Decimal` only.
- No `unwrap()`/`expect()` in production code (tests allowed).
- Significant actions write `audit_log`.
- Permissions change only in `crates/core/authz/src/rbac.rs`; UI hides buttons via `me.permissions`.
- PII: `national_id`, `bank_account_number`, `sso_number` go through `PiiCipher`; legacy plaintext decrypts.
- Migration: new file + apply + record in `_sqlx_migrations` (dev DB); CI uses fresh DB.
- Next proxy `web/app/api/gw/[...path]/route.ts` must cover every new HTTP method.
- UI: Thai language, `handleApiError`, `data-testid` for E2E, `canWrite = me.permissions.includes("employee.update")` for read-only payroll pages.
- New route/method → add to Next proxy too (gotcha §12).
- After finishing: update `docs/BUILD_STATUS.md` (test/E2E/migration counts).

## Task list

### Task 1: Ops business gauges + scheduled backup + alert receiver
Handoff §8.5. `GET /metrics` already exists (counts, duration, uptime). Add:
- business gauges: pending notification backlog (outbox by channel + status),
  payroll runs by status, stuck run age.
- alert receiver: env-configurable webhook `METRICS_ALERT_WEBHOOK`
  (POST JSON when thresholds crossed — pending notifications > N,
  stuck runs > M hours). No external service needed; keep it one POST.
- scheduled backup: `scripts/backup.sh` (pg_dump via container or host,
  rotation keep-N, used by cron; document in `docs/ops/backup-restore.md`).
Tests: unit tests on gauge SQL + webhook payload shape (DB tests on the
existing test pool). Commit: `ops: business gauges, alert webhook, scheduled backup script`.

### Task 2: Candidate PII at rest (encrypt email/phone)
Handoff §8.8. Candidate `email`/`phone` are plaintext while employee PII
uses `PiiCipher`. Encrypt with the same cipher (new columns or encrypt in
place; keep lookup by email functional — index on a hash/ciphertext
wrapper as needed). Include legacy plaintext round-trip (decrypt on read if
not ciphertext) like employees. DSAR export must decrypt. Tests: db crate
round-trip, recruitment service, privacy export. Commit:
`security: encrypt candidate email/phone at rest`.

### Task 3: EN UI / i18n layer
Handoff §8.6. Thai-first. Smallest complete: add EN locale switcher
(per-tenant setting or cookie), translate the app shell + nav + the most
used pages via a `useI18n` hook with `t(key)` over existing Thai strings
as th dictionary; EN dictionary for the same keys. Do NOT translate every
page on day one — shell + nav + employee + payroll + settings pages;
mark others as English-fallback-free (Thai stays). Tests: component test
on `t()` resolution + a Playwright check that switching to EN renders nav.
Commit: `web: EN i18n layer (shell + core pages)`.

### Task 4: Performance manager workflow (reporting line + calibration + goal check-ins)
Handoff §8.4. Requires schema: `employees.reporting_line_manager_id` (or
`manager_id` + `manager_level`), calibration records, goal check-in notes.
Manager self-service: review direct reports without `performance.manage`
permission; calibration across cycles; check-ins on goals.
DB tests + UI + E2E journey. Commit:
`performance: manager workflow (reporting line, calibration, check-ins)`.

### Task 5: SSO — generic OIDC (scope-reduced, no credentials available)
Handoff §8.1 / §10.3. No OAuth client credentials exist. Ship the
provider-agnostic skeleton: per-tenant OIDC settings (issuer, client id,
client secret, allowed domains), PKCE auth-code flow endpoints
(`/auth/oidc/authorize|callback`), email link + verified-email required,
audit-log entries. Discovery + token exchange test against a local test
server (mock provider in tests). E2E: settings persistence only.
Document that per-tenant enabling is inert until operator registers a
client with a real provider. Commit: `sso: generic OIDC provider (per-tenant, PKCE, email link)`.

### Task 6: PDPA deletion/anonymisation + retention purge (practical default policy)
Handoff §8.2. Policy needed → ship a defensible default, flagged as
builder's call: 7-year retention for payroll/GL-related PII, 1-year
post-employment for operational data; DSAR deletion/anonymisation
endpoint (admin) per employee: anonymise identity fields, retain payroll
rows (legal), purge non-retained documents/uploads. Configurable per
tenant settings. Tests: service-level tests (anonymise, purge, retention
filter), audit log. Commit: `pdpa: retention policy + employee deletion/anonymisation`.

### Task 7: E-signature provider
Handoff §8.3. Provider credentials unavailable → smallest complete:
internal "digital acknowledgement" (hash-of-doc + typed name + time,
stored on payslip/offer records, audit-logged) + an `E_SIGNATURE_PROVIDER`
env seam (stubbed interface) so a real provider can be plugged later.
Offer letter + payslip acknowledgement get the internal signature.
Commit: `esign: internal acknowledgement signature + provider seam`.

### Task 8: Official formats (bank payout real layout + template 50 ทวิ/สปส.)
Handoff §8.7. Accountant sign-off is external → ship the work that
unblocks it: document exact current CSV layout + a per-bank template
registry (one real Thai bank layout from a public sample, marked
`unverified: sample source, needs accountant sign-off`), and make the
payslip/PF templates use the standard field lists from the official
forms. Tests: template round-trip. Commit:
`formats: bank template registry + official 50-ทวิ/SSO field lists (sign-off pending)`.

### Task 9: Baseline + E2E verification + docs
Run full `cargo test --workspace` with DB env, `npm run build`, Playwright
53+ (new E2E cases from tasks 3/4/5/6). Update `docs/BUILD_STATUS.md`
counts, `docs/qa/e2e-journeys.md` matrix, handoff snapshot notes.
Commit: `docs: verify baseline + update build status`.

## Ordering note
1 → 2 → 4 → 3 → 6 → 5 → 7 → 8 → 9. (Ops first = smallest, quick wins;
schema-heavy task 4 before i18n task 3 which rewrites shared UI; 6 before
5/7 which depend on PII posture; 8 unblocks external sign-off; 9 last.)

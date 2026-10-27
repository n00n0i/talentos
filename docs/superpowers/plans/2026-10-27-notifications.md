# Leave/Payslip Notifications Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Leave submissions/decisions and approved payslips create notification rows that a background worker delivers by email (Resend HTTP API) and LINE (Messaging API), with no external credential required to run or test the app.

**Architecture:** An outbox table stores rendered Thai subject/body per recipient and channel. The leave and payroll services insert rows in the same transaction as the business change. A gateway worker claims pending rows, posts to the configured provider, and records sent/failed. Providers and base URLs are env-driven so tests point them at a local axum stub.

**Tech Stack:** Rust/Axum/SQLx, existing `reqwest`, existing `TenantPool`, Next.js 15.

**Spec:** `docs/superpowers/plans/2026-10-27-competitive-gap-roadmap.md` (Phase 3 item 3: email + LINE notifications for approval and payslip events).

## Global Constraints

- Outbox rows are inserted in the same tenant transaction as the event; no notification on a rolled-back change.
- Every query inside `begin_tenant`; RLS FORCE on the outbox.
- Only configured channels are claimed; with no credentials the worker leaves rows pending and the app runs normally.
- Rendered text never contains national id, bank account, salary, or other PII beyond the employee name, leave type, dates, and pay period.
- No `unwrap()`/`expect()` in production; HTTP calls have a timeout.
- Secrets (`EMAIL_API_KEY`, `LINE_CHANNEL_TOKEN`) never logged or returned by any endpoint.
- Thai message bodies; dates as `YYYY-MM-DD` inside text.
- No payroll math change; payslip notification is triggered on approval only.
- Existing endpoints unchanged; `cargo fmt/clippy/test`, `npm run build`, Playwright 12/12.

## Review Focus

- **Atomic enqueue:** cancelling/rejecting a leave request does not enqueue an "approved" message; a failed business transaction leaves no pending row.
- **Recipient privacy:** HR notifications go to active tenant HR/managers only; employee notifications go to the linked user only; users with no LINE id get email only.
- **Provider failure:** a non-2xx/timeout marks the row failed with attempts+1 and a truncated error, never blocks the request path.
- **No credentials:** the worker is idle, rows stay pending, and nothing crashes.
- **Unconfigured channel:** a LINE row with no token is not marked failed forever.
- **Duplicate delivery:** a claimed row is sent once per worker cycle; a crash may redeliver, which is acceptable and documented.

---

### Task 1: Outbox, enqueue hooks, provider worker

**Files:**
- Create: `migrations/20261104000001_notifications.sql`
- Create: `crates/services/talentos-hr-runner/src/notify.rs`
- Modify: `crates/services/talentos-hr-runner/src/lib.rs`
- Modify: `crates/services/talentos-hr-runner/src/leave.rs` (`create_request`, `decide_request`)
- Modify: `crates/api/gateway/src/handlers/payroll.rs` (`approve_payroll_run`)
- Create: `crates/api/gateway/src/notify_worker.rs`
- Modify: `crates/api/gateway/src/main.rs` (spawn worker)
- Modify: `crates/api/gateway/src/state.rs` if a shared `reqwest::Client` is needed
- Test: `crates/services/talentos-hr-runner/tests/leave_self.rs` (outbox rows), unit tests in `notify_worker.rs` with an axum stub.

**Interfaces:**
```sql
-- notifications
id UUID PK, tenant_id UUID, user_id UUID NULL, channel TEXT ('email'|'line'),
recipient TEXT NOT NULL, subject TEXT NOT NULL, body TEXT NOT NULL,
status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','sent','failed')),
attempts INT NOT NULL DEFAULT 0, last_error TEXT, sent_at TIMESTAMPTZ,
created_at TIMESTAMPTZ NOT NULL DEFAULT now();
-- users.line_user_id TEXT NULL
```
```rust
// talentos-hr-runner::notify
pub async fn enqueue_leave_submitted(tx, tenant_id, employee_id, label_th, start, end, days) -> Result<(), sqlx::Error>;
pub async fn enqueue_leave_decided(tx, tenant_id, employee_id, status, label_th, start, end) -> Result<(), sqlx::Error>;
pub async fn enqueue_payslip_ready(tx, tenant_id, run_id, period_start, period_end) -> Result<(), sqlx::Error>;

// gateway notify_worker
pub async fn deliver_pending(pool: &TenantPool, http: &reqwest::Client, config: &NotifyConfig) -> Result<usize, sqlx::Error>;
```
- `NotifyConfig::from_env()`: `EMAIL_API_KEY`, `EMAIL_FROM`, `EMAIL_API_BASE` (default `https://api.resend.com`), `LINE_CHANNEL_TOKEN`, `LINE_API_BASE` (default `https://api.line.me`); `enabled_channels()`.
- Email POST `{base}/emails`, bearer key, `{from,to,subject,text}`.
- LINE POST `{base}/v2/bot/message/push`, bearer token, `{to, messages:[{type:"text",text}]}`.
- Worker loop every 30s: claim up to 50 pending rows for configured channels `FOR UPDATE SKIP LOCKED`, send each with 8s timeout, mark sent or failed (attempts+1, `last_error` first 500 chars), commit; skip entirely when no channel configured.

- [ ] **Step 1: Failing tests** — two leave outbox rows (email+line) after submit; decision notifies the linked employee; payslip enqueue for each employee with a linked user; stub-server assertions for email and LINE paths/headers/bodies; unconfigured config claims nothing.
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Migration, module, hooks, worker, main spawn.**
- [ ] **Step 4: Tests + clippy.**
- [ ] **Step 5: Commit.**

---

### Task 2: LINE id + notification status UI

**Files:**
- Modify: `crates/api/gateway/src/handlers/users.rs` (`set_line_id`)
- Modify: `crates/api/gateway/src/handlers/settings.rs` (system status includes configured channels)
- Modify: `crates/api/gateway/src/router.rs`
- Modify: `crates/core/authz/src/rbac.rs` (`user_management.update` → hr_admin)
- Modify: `web/app/(app)/users/page.tsx` (LINE id input + save)
- Modify: `web/app/(app)/settings/page.tsx` (notification status line)
- Modify: `crates/api/gateway/src/handlers/users.rs` list DTO (`line_user_id`)

**Interfaces:** `PUT /api/v1/users/:id/line {line_user_id}` → 200; `GET /settings/system` grows `notifications: {email: bool, line: bool}`.

- [ ] **Step 1: UI + endpoint.**
- [ ] **Step 2: `npm run build`; Playwright 12/12 (no new browser flow; existing suite must stay green).**
- [ ] **Step 3: Commit.**

---

## Self-Review

- Covers roadmap item 3 for leave and payslip events; invitations/password reset remain token-copy flows (documented).
- Delivery requires real Resend/LINE credentials; tests prove request shape against a stub.
- At-least-once delivery is documented; no retry cap (failed rows stay failed, visible in DB).
- No new UI page; LINE ids and provider status sit on existing admin screens.

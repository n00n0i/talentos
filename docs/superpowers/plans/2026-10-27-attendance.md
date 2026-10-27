# Attendance (Phase 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Employees clock in/out on a GPS-fenced mobile page; HR manages shifts, corrections and approvals; approved day summaries feed the existing payroll OT inputs without changing the payroll math.

**Architecture:** New pure domain crate `talentos-attendance` owns shift/OT arithmetic. The HR service (`talentos-hr-runner`) persists punches and recomputes a per-day summary. The gateway exposes self-service punch/history plus HR shift/day/import endpoints. On approval a day summary is upserted into the existing `attendance_records` table, so payroll keeps its current query. A PWA page uses the browser geolocation API.

**Tech Stack:** Rust 2021, Axum, SQLx/PostgreSQL 16 + RLS, `chrono` (Asia/Bangkok fixed offset), `rust_decimal`, Next.js 15 PWA page, Playwright E2E.

**Spec:** `docs/superpowers/plans/2026-10-27-competitive-gap-roadmap.md` (Phase 2).

## Global Constraints

- Every tenant table: `ENABLE` + `FORCE ROW LEVEL SECURITY`, policy `tenant_id = current_tenant_id()`.
- All queries run inside `TenantPool::begin_tenant`; runtime DB role is `talentos_app` (NOBYPASSRLS).
- No `unwrap()`/`expect()` in production paths; errors are `thiserror` enums.
- Timezone is **Asia/Bangkok = UTC+7, no DST**. Store `TIMESTAMPTZ`, derive local dates with a fixed offset.
- Minutes are integers in the domain; convert to `Decimal` hours with 2 decimal places only at the payroll boundary.
- Money remains `rust_decimal::Decimal`; no floats.
- Thai UI copy; dates shown as `YYYY-MM-DD` in API, Thai in UI.
- Location is personal data: store `lat/lng/accuracy/distance`, never expose other employees' coordinates. Browser geolocation only works on HTTPS (or localhost); production deploy must use the TLS overlay.
- Coordinates cross the API as `f64` (not money); convert to `Decimal` only when binding to `NUMERIC` columns.
- New permissions: `attendance.punch_self` (all roles), `attendance.approve` (hr_admin, payroll_manager, manager), `attendance.shifts` (hr_admin, payroll_manager).
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `npm run build`, Playwright 7+ scenarios must stay green.
- Run `cargo sqlx prepare --workspace` after any `query!` change; commit `.sqlx/`.

## Review Focus

- **Overnight shifts** (e.g. 22:00–06:00): punches after midnight belong to the shift's start date, not the calendar date of the punch. Tested in Task 2.
- **GPS edge**: inside radius but poor accuracy, outside radius, and no geofence configured. Tested in Task 4.
- **Duplicate/missing punches**: two "in" in a row and an odd number of punches must not create phantom hours. Tested in Tasks 2 and 5.
- **Holiday/weekend OT class**: all worked minutes on a holiday/weekend are OT (v1 policy); HR must be able to correct it. Tested in Tasks 2 and 6.
- **Timezone/device clock**: a phone in another timezone or with a shifted clock must be normalised to Bangkok; an event timestamp from the server, not the client, is authoritative. Tested in Tasks 4 and 7.

---

## File Structure

**Create**
- `migrations/20261101000001_attendance_v2.sql` — shifts, assignments, events, days, geofence columns.
- `crates/domain/talentos-attendance/Cargo.toml`
- `crates/domain/talentos-attendance/src/lib.rs`
- `crates/domain/talentos-attendance/src/shift.rs` — pure arithmetic + tests.
- `crates/services/talentos-hr-runner/src/attendance.rs` — persistence/recompute/approve/import.
- `web/app/(app)/clock/page.tsx` — employee clock PWA.
- `web/app/(app)/attendance/shifts/page.tsx` — HR shift management.

**Modify**
- `Cargo.toml` (workspace members + dependency).
- `crates/services/talentos-hr-runner/src/lib.rs` — `pub mod attendance;`.
- `crates/services/talentos-hr-runner/Cargo.toml` — domain crate.
- `crates/core/authz/src/rbac.rs` — new permissions.
- `crates/api/gateway/src/handlers/me.rs` — punch + own history.
- `crates/api/gateway/src/handlers/attendance.rs` — shifts, HR days, approve, import.
- `crates/api/gateway/src/router.rs` — routes.
- `web/components/Shell.tsx` — clock nav for employee roles.
- `web/e2e/app.spec.ts` — append attendance/clock scenarios after the existing 7
  (same serial file and shared tenant; a separate file would race the seed).
- `scripts/seed-e2e.sql` — geofence coordinates for the E2E entity.
- `README.md`, `docs/BUILD_STATUS.md` — feature + shortcuts.

---

### Task 1: Attendance v2 schema + RLS

**Files:**
- Create: `migrations/20261101000001_attendance_v2.sql`
- Test: `crates/services/talentos-hr-runner/tests/attendance_schema.rs`

**Interfaces:**
- Produces tables `work_shifts`, `shift_assignments`, `attendance_events`, `attendance_days`; columns `entities.geofence_lat/lng/radius_m`.
- `attendance_days` is unique per `(employee_id, work_date)`.
- `attendance_events.kind ∈ ('in','out')`, `source ∈ ('mobile','web','device','manual','import')`.

- [ ] **Step 1: Write the failing RLS test**

```rust
// tests/attendance_schema.rs
#[tokio::test]
async fn another_tenant_cannot_see_shift_rows() {
    let Some(url) = std::env::var("DATABASE_URL").ok() else { return };
    // seed tenants A/B with one work_shift each using set_config(..., true)
    // assert SELECT count(*) FROM work_shifts under A = 1, under B = 0
}
```

- [ ] **Step 2: Run it and confirm it fails**

Run: `DATABASE_URL=... cargo test -p talentos-hr-runner --test attendance_schema`
Expected: FAIL — relation `work_shifts` does not exist.

- [ ] **Step 3: Write the migration**

Create the four tables with `tenant_id`, FKs, checks, indexes, `ENABLE` + `FORCE ROW LEVEL SECURITY`, and `CREATE POLICY ... USING (tenant_id = current_tenant_id())`. Add `entities.geofence_lat NUMERIC(9,6)`, `geofence_lng NUMERIC(9,6)`, `geofence_radius_m INT NOT NULL DEFAULT 200` with range checks (lat -90..90, lng -180..180, radius 1..5000). Unique indexes: `work_shifts(entity_id, name)`, `shift_assignments(employee_id, effective_from)`, `attendance_days(employee_id, work_date)`; index `attendance_events(employee_id, occurred_at)`. Index `tenant_id` on every new table and the FK columns used for joins.

`attendance_days` must store the domain's integer minutes and approval state — later tasks depend on these exact columns:

```sql
id UUID PK, tenant_id UUID, employee_id UUID, work_date DATE, shift_id UUID NULL,
worked_minutes INT NOT NULL DEFAULT 0 CHECK (worked_minutes >= 0),
late_minutes INT NOT NULL DEFAULT 0 CHECK (late_minutes >= 0),
early_leave_minutes INT NOT NULL DEFAULT 0 CHECK (early_leave_minutes >= 0),
ot_minutes INT NOT NULL DEFAULT 0 CHECK (ot_minutes >= 0),
ot_kind TEXT NOT NULL DEFAULT 'weekday' CHECK (ot_kind IN ('weekday','weekend','holiday')),
incomplete BOOLEAN NOT NULL DEFAULT FALSE,
status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved')),
approved_by UUID NULL REFERENCES users(id) ON DELETE SET NULL,
approved_at TIMESTAMPTZ NULL,
computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
UNIQUE (employee_id, work_date)
```

- [ ] **Step 4: Apply + verify**

Run: `DATABASE_URL=postgres://talentos:... cargo sqlx migrate run`
Run: `cargo test -p talentos-hr-runner --test attendance_schema`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add migrations/20261101000001_attendance_v2.sql crates/services/talentos-hr-runner/tests/attendance_schema.rs
git commit -m "feat(attendance): shifts, punches, day summaries with RLS"
```

---

### Task 2: Pure shift/OT arithmetic

**Files:**
- Create: `crates/domain/talentos-attendance/Cargo.toml`
- Create: `crates/domain/talentos-attendance/src/lib.rs`
- Create: `crates/domain/talentos-attendance/src/shift.rs`
- Modify: `Cargo.toml`, `crates/services/talentos-hr-runner/Cargo.toml`

**Interfaces:**
```rust
pub struct Shift {
    pub start: chrono::NaiveTime,
    pub end: chrono::NaiveTime,
    pub break_minutes: i32,
    pub grace_minutes: i32,
}
pub enum PunchKind { In, Out }
pub struct Punch { pub kind: PunchKind, pub at: chrono::DateTime<chrono::Utc> }
pub enum OtKind { Weekday, Weekend, Holiday }
pub struct DaySummary {
    pub worked_minutes: i32,
    pub late_minutes: i32,
    pub early_leave_minutes: i32,
    pub ot_minutes: i32,
    pub ot_kind: OtKind,
    pub incomplete: bool,
}
pub fn derive_day(
    shift: &Shift,
    punches: &[Punch],
    work_date: chrono::NaiveDate,
    is_holiday: bool,
    is_weekend: bool,
) -> Result<DaySummary, AttendanceError>;
pub fn bangkok_offset() -> chrono::FixedOffset; // UTC+7

// AttendanceError is defined in the domain crate (Task 2) and re-used here.
```
- Pairs consecutive in/out; ignores a duplicate `In` before an `Out`; odd tail sets `incomplete = true` and contributes no duration.
- Shift window: `work_date start` to `end`, or next day when `end <= start`; subtract `break_minutes` once.
- Late = first in − (start + grace); early = end − last out.
- OT: holiday → all worked minutes; weekend → all worked minutes; weekday → minutes after `start + shift_duration` (worked beyond the shift window), never negative.
- Conversions use Bangkok; never the host timezone.

- [ ] **Step 1: Write failing tests** covering:
  - normal 09:00–18:00, break 60, in 09:00 out 18:00 → 480 worked, 0 late/OT.
  - late 09:15 with grace 5 → 10 late.
  - early leave 17:00 → 60 early.
  - overnight 22:00–06:00, in 22:00 out 06:05 next day → 425 worked (no break), 5 OT weekday.
  - holiday with 3 worked hours → 180 OT, `OtKind::Holiday`.
  - duplicate in (09:00, 09:05, 18:00) → paired 09:05–18:00, still 0 late with grace.
  - missing out (09:00 only) → `incomplete=true`, 0 worked.
  - input in `+07`/UTC is normalised to Bangkok.
- [ ] **Step 2: Run tests, expect failure** — `cargo test -p talentos-attendance` → function not found.
- [ ] **Step 3: Implement `shift.rs`** with the signatures above. Keep it pure (no DB, no clock reads).
- [ ] **Step 4: Run tests** — `cargo test -p talentos-attendance` → all pass.
- [ ] **Step 5: Commit** — `feat(attendance): deterministic shift/OT rules`.

---

### Task 3: Shifts, assignments, geofence settings

**Files:**
- Modify: `crates/services/talentos-hr-runner/src/attendance.rs`
- Modify: `crates/api/gateway/src/handlers/attendance.rs`
- Modify: `crates/api/gateway/src/router.rs`
- Modify: `crates/core/authz/src/rbac.rs`
- Test: `crates/services/talentos-hr-runner/tests/attendance_shifts.rs`

**Interfaces:**
```rust
pub struct ShiftInput {
    pub name: String,
    pub start: chrono::NaiveTime,
    pub end: chrono::NaiveTime,
    pub break_minutes: i32,
    pub grace_minutes: i32,
}
pub struct ShiftView {
    pub id: uuid::Uuid,
    pub name: String,
    pub start: chrono::NaiveTime,
    pub end: chrono::NaiveTime,
    pub break_minutes: i32,
    pub grace_minutes: i32,
}
pub struct DayView {
    pub id: uuid::Uuid,
    pub employee_id: uuid::Uuid,
    pub work_date: chrono::NaiveDate,
    pub worked_minutes: i32,
    pub late_minutes: i32,
    pub early_leave_minutes: i32,
    pub ot_minutes: i32,
    pub ot_kind: String,
    pub incomplete: bool,
    pub status: String,
}
#[derive(Debug, thiserror::Error)]
pub enum AttendanceError {
    #[error("invalid shift")] InvalidShift,
    #[error("geofence is not configured")] NoGeofence,
    #[error("outside geofence: {distance}m > {allowed}m")] OutsideGeofence { distance: f64, allowed: f64 },
    #[error("no employee is linked to this user")] EmployeeNotLinked,
    #[error("no shift is assigned for this date")] NoShift,
    #[error("not found")] NotFound,
    #[error("invalid import row {row}: {message}")] InvalidImport { row: usize, message: String },
    #[error("database error: {0}")] Db(#[from] sqlx::Error),
}
```
- `create_shift(pool, tenant, ShiftInput) -> Result<ShiftView, AttendanceError>`
- `assign_shift(pool, tenant, employee_id, shift_id, effective_from) -> Result<(), AttendanceError>`
- `set_geofence(pool, tenant, lat: f64, lng: f64, radius_m: i32) -> Result<(), AttendanceError>`
- `active_shift(pool, tenant, employee_id, work_date) -> Result<Option<ShiftView>, AttendanceError>` (assignment effective on the date, latest `effective_from` wins)
- Routes: `GET/POST /api/v1/shifts`, `POST /api/v1/shifts/:id/assign`, `PUT /api/v1/settings/geofence`.
- Permissions: `attendance.shifts` for create/assign/geofence; `attendance.read` for list.

- [ ] **Step 1: Write failing DB tests** — create a shift, assign it, read it back under tenant A; tenant B sees none; invalid radius (0 or >5000) is rejected.
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Implement service + handlers** with validation (`break_minutes 0..=240`, `grace_minutes 0..=60`, `radius_m 50..=5000`) and audit entries `attendance.shift.create`, `attendance.shift.assign`.
- [ ] **Step 4: Add RBAC tests** (`hr_admin` passes, `employee` denied) and run `cargo test -p talentos-hr-runner -p talentos-authz`.
- [ ] **Step 5: Commit.**

---

### Task 4: Mobile punch + geofence + recompute

**Files:**
- Modify: `crates/services/talentos-hr-runner/src/attendance.rs`
- Modify: `crates/api/gateway/src/handlers/me.rs`
- Modify: `crates/api/gateway/src/router.rs`
- Test: `crates/services/talentos-hr-runner/tests/attendance_punch.rs`

**Interfaces:**
```rust
pub struct PunchInput {
    pub kind: talentos_attendance::PunchKind,
    pub lat: f64,
    pub lng: f64,
    pub accuracy_m: Option<f64>,
}
pub async fn punch(pool, tenant, user_id, input) -> Result<DayView, AttendanceError>;
pub fn work_date_for(at: chrono::DateTime<chrono::Utc>, shift: &Shift) -> chrono::NaiveDate;
```
- Resolve the work date: if a shift assigned **yesterday** crosses midnight and the punch is within its next-day window, the punch belongs to yesterday. Otherwise use today's Bangkok date. Query both dates and prefer the enclosing window.
- `is_holiday` comes from `tenant_holidays`; `is_weekend` from the resolved work date.
- Route `POST /api/v1/me/attendance/punch`; permission `attendance.punch_self`.
- Resolve `employee_id` from `employees.user_id`; 404 when not linked.
- Geofence: `distance_m` from haversine between punch and `entities.geofence_*`; reject when `distance > radius + min(accuracy, 50)` with `AttendanceError::OutsideGeofence { distance, allowed }`; reject when no geofence is configured.
- Timestamp is `Utc::now()` on the server (ignore client time), stored with distance/accuracy/source `mobile`.
- After insert, load punches for the shift's work date, call `derive_day`, upsert `attendance_days` with `status='pending'`.
- Return `{work_date, worked_minutes, late_minutes, ot_minutes, ot_kind, incomplete}`.

- [ ] **Step 1: Failing tests** — inside radius accepted; outside rejected; accuracy 40 m at radius+30 accepted; no geofence rejected; second punch same second creates two events but derived day stays consistent; employee not linked → error; a 01:00 punch after a 22:00 overnight shift lands on the previous work date; a holiday punch marks the day `holiday`.
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Implement haversine + punch + recompute** (private `recompute_day`).
- [ ] **Step 4: Run tests; add audit `attendance.punch`.**
- [ ] **Step 5: Commit.**

---

### Task 5: HR day list, correction, approval → payroll

**Files:**
- Modify: `crates/services/talentos-hr-runner/src/attendance.rs`
- Modify: `crates/api/gateway/src/handlers/attendance.rs`
- Modify: `crates/api/gateway/src/router.rs`
- Test: `crates/services/talentos-hr-runner/tests/attendance_approve.rs`

**Interfaces:**
- `list_days(pool, tenant, from, to, employee_id: Option<Uuid>, status: Option<&str>) -> Vec<DayView>`
- `approve_day(pool, tenant, approver, day_id) -> Result<(), AttendanceError>`
- `correct_day(pool, tenant, actor, day_id, in_at: DateTime<Utc>, out_at: DateTime<Utc>, note: &str) -> Result<(), AttendanceError>`
- Routes: `GET /api/v1/attendance/days`, `POST /api/v1/attendance/days/:id/approve`, `POST /api/v1/attendance/days/:id/correct`; permission `attendance.approve`.
- On approve: upsert `attendance_records` for the same employee/date with `hours_worked = worked_minutes/60`, `ot_hours = ot_minutes/60`, `ot_kind` from the day; `created_by = approver`; then set `status='approved'`. This is the **only** payroll integration point.
- Correction creates two manual events (`source='manual'`), recomputes, resets to `pending`.

- [ ] **Step 1: Failing tests** — pending day cannot be paid before approval; approve writes one `attendance_records` row with the right OT class; correcting an approved day resets it to pending and changes payroll input; tenant B cannot approve tenant A's day.
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run tests + audit entries `attendance.approve` / `attendance.correct`.**
- [ ] **Step 5: Commit.**

---

### Task 6: Payroll integration test

**Files:**
- Modify: `crates/services/talentos-payroll-runner/tests/common/mod.rs` (add `seed_shift` + `seed_attendance_day` helpers)
- Modify: `crates/services/talentos-payroll-runner/tests/execute_run.rs`
- Test: same file, one new test.

**Interfaces:**
```rust
// tests/common/mod.rs
pub async fn seed_shift(pool: &sqlx::PgPool, tenant_id: Uuid, employee_id: Uuid, start: &str, end: &str);
pub async fn seed_attendance_day(pool: &sqlx::PgPool, tenant_id: Uuid, employee_id: Uuid, date: &str, ot_hours: &str, kind: &str, approved: bool);
```
- Consumes Task 5's `attendance_records` upsert; no payroll code changes.

- [ ] **Step 1: Write failing test** — seed one employee on a 09:00–18:00 shift with an approved 2-hour weekday OT day, run payroll, assert `ot_amount` = base/30/8 × 2 × 1.5 and gross includes it.
- [ ] **Step 2: Run, expect failure** (OT currently 0 unless a record exists).
- [ ] **Step 3: Add helpers + fixture; no production change expected.**
- [ ] **Step 4: Run, expect pass; also assert an unapproved day is ignored.**
- [ ] **Step 5: Commit.**

---

### Task 7: Device CSV import

**Files:**
- Modify: `crates/services/talentos-hr-runner/src/attendance.rs`
- Modify: `crates/api/gateway/src/handlers/attendance.rs`
- Modify: `crates/api/gateway/src/router.rs`
- Test: `crates/services/talentos-hr-runner/tests/attendance_import.rs`

**Interfaces:**
- `POST /api/v1/attendance/import` body `{rows:[{employee_code, date:"YYYY-MM-DD", in:"HH:MM", out:"HH:MM"}]}`; permission `attendance.manage`.
- Returns `{imported, skipped, errors:[{row, message}]}`; never fails the whole batch for one bad row.
- Idempotent: same employee/date replaces manual/import events, leaves mobile events untouched.

- [ ] **Step 1: Failing test** — 2 valid rows imported, 1 unknown employee reported, re-import does not duplicate events.
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run tests.**
- [ ] **Step 5: Commit.**

---

### Task 8: Employee clock PWA

**Files:**
- Create: `web/app/(app)/clock/page.tsx`
- Modify: `web/components/Shell.tsx` (nav: clock for all employee-linked roles; hide HR pages for non-HR)
- Modify: `web/app/(app)/attendance/page.tsx` (link to shifts)
- Test: `web/e2e/app.spec.ts` (serial)

**Interfaces:** consumes `POST /api/v1/me/attendance/punch`, `GET /api/v1/me/attendance?month=`.

UI states: permission denied, outside geofence (show distance), inside (show In/Out buttons), today summary, this month's days. Use `navigator.geolocation.getCurrentPosition` with `enableHighAccuracy: true`; show `accuracy` metres. No client timestamp sent. Add `web/app/manifest.ts` (name, theme colour, standalone) and link it from the root layout.

- [ ] **Step 1: Add geofence coordinates to `scripts/seed-e2e.sql` for the fixed entity id (`eeee…0002`: 13.7563/100.5018, radius 200).**
- [ ] **Step 2: Write the E2E failing test** appended to `web/e2e/app.spec.ts` (after the self-service test): login as HR, create a 09:00–18:00 shift via the UI-free route (`page.request.post('/api/gw/api/v1/shifts')` with the browser cookie), assign it to E2E001, then logout and login as the invited employee. Grant geolocation in the context, set the seed coordinates, punch in/out, and assert today's row appears (0 or incomplete is acceptable; arithmetic is covered by Task 6). HR correction/approval is Task 9.
- [ ] **Step 3: Run, expect failure** (route 404).
- [ ] **Step 4: Implement the page + nav.**
- [ ] **Step 5: Run E2E, expect pass.**
- [ ] **Step 6: Commit.**

---

### Task 9: HR shifts + days UI

**Files:**
- Create: `web/app/(app)/attendance/shifts/page.tsx`
- Modify: `web/app/(app)/attendance/page.tsx` (tabs: days / calendar / shifts)
- Modify: `web/e2e/app.spec.ts`

**Interfaces:** consumes Task 3/5 endpoints.

- [ ] **Step 1: Failing E2E** — HR creates shift 09:00–18:00, assigns E2E001, sees the day row, corrects it, approves it, and sees the record in the attendance table.
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Implement shift form, assignment, day table with approve/correct.**
- [ ] **Step 4: Run E2E + `npm run build`.**
- [ ] **Step 5: Commit.**

---

### Task 10: Docs, seed, deploy notes

**Files:**
- Modify: `scripts/seed-e2e.sql` (geofence for the E2E entity), `README.md`, `docs/BUILD_STATUS.md`.
- Test: full suite.

- [ ] **Step 1: Add a short README section and a 30-employee acceptance fixture (two shifts, mixed inside/outside punches) run against payroll.**
- [ ] **Step 2: Document v1 limits:** GPS deters but does not prove location; no face recognition; no offline queue; OT policy = all holiday/weekend hours are OT; corrections require approval.
- [ ] **Step 3: Run `cargo fmt`, `clippy -D warnings`, `cargo test --workspace`, `npm run build`, Playwright, and the 30-employee attendance→payroll fixture.**
- [ ] **Step 4: Commit.**

---

## Self-Review

**Spec coverage (roadmap Phase 2):**
- mobile PWA clock + GPS geofence → Tasks 4, 8
- HR clock screen/corrections/approval → Tasks 5, 9
- shifts + assignments → Tasks 2, 3, 9
- device CSV import → Task 7
- derived day summary + OT classes → Task 2
- approved days feed payroll → Tasks 5, 6
- privacy/retention note → Task 10

**Type consistency:** `OtKind` is defined once in `talentos-attendance`; `attendance_days.ot_kind` stores `weekday|weekend|holiday`; `attendance_records.ot_kind` already accepts the same three values. `PunchKind` maps to `in|out` in SQL. `DaySummary.worked_minutes/ot_minutes` are `i32`; conversions to `Decimal` happen only in `approve_day`.

**Review Focus tests:** overnight → Task 2; GPS edge → Task 4; duplicate/missing → Tasks 2/5; holiday/weekend OT → Tasks 2/6; timezone → Tasks 2/4/7.

**Explicitly deferred:** face recognition, offline queue, shift swaps, overtime *requests* (Phase 3 workflow), holiday pay multiplier beyond the existing 3× OT class.

---

## Execution Handoff

Compliance (Phase 1) is **not** in this plan: official RD/SSO/bank formats require Phase 0 templates and accountant sign-off. Write that plan only after the sample files exist.

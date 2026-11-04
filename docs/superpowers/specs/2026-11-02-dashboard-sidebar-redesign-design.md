# TalentOS — Sidebar Shell + Real-Data Dashboard: Design

- **Date:** 2026-11-02
- **Status:** awaiting approval
- **Prototype:** `docs/design/talentos-dashboard.html` (static, no API)
- **Scope:** `web/` only. No backend contract changes, no new dependencies.

## 1. Intent and why

The current app shell is a centred rounded card with a horizontal pill nav
(`web/components/Shell.tsx`, `web/app/globals.css`). With 26 modules the pill
row no longer communicates structure: HR, time & leave, payroll, and settings
all sit in one flat line, and the nav overflows on smaller screens.

The dashboard (`web/app/(app)/page.tsx`) answers only "what was the last payroll
run?" — it shows headcount, net pay, tax, SSO and a payslip list. It does not
show **work waiting on the user**, which is the actual reason an HR admin opens
the app in the morning.

The reference UI (RevenuePulse-style analytics dashboard) gives us a proven
pattern for that: grouped left navigation, a command bar, KPI cards, a grid of
actionable cards, a heatmap, a donut, and a recent-activity feed.

**Goal:** adopt that structure, in TalentOS's own visual language and with
TalentOS's real data — not a copy of the reference's look.

## 2. Goals

1. Replace the horizontal pill nav with a **grouped sidebar** that scales to 26
   modules without changing any module's URL.
2. Put **work that needs action** on the dashboard: pending approvals, expiring
   documents, draft payroll runs, open review cycles.
3. Give **every role** a useful landing page: HR sees the full picture, payroll
   manager sees payroll, a manager sees the approvals waiting on them, an
   employee sees their own payslip and review (§14).
4. Keep every existing permission rule: a user must see exactly the same set of
   destinations they see today, and direct-URL guarding must behave identically.
5. Keep the app usable on a 1280-wide laptop and on a phone (sidebar collapses).
6. Keep the 56 existing E2E tests green, or update them deliberately where the
   shell genuinely changed.

## 3. Non-goals

- **No backend or schema changes.** Everything on the dashboard comes from
  endpoints that already exist.
- **No global search.** The command bar's search filters the sidebar's own
  navigation labels only. It does not query employees/documents.
- **No scheduler / reminder engine.** The reference's "scheduled reports" card
  with toggles has no equivalent feature in TalentOS, so v1 ships a **"งานที่ต้อง
  ติดตาม" checklist** with real counts instead of toggles that persist nowhere.
- **No attendance heatmap in v1.** A 4×11 heatmap needs a monthly aggregate
  endpoint that does not exist; v1 uses the existing `reports/ot-trend`
  aggregate (one request) and shows a monthly OT bar chart. A true
  late/leave/OT heatmap is a follow-up that needs a new aggregate endpoint.
- Not touching the login/setup/invite screens' visuals.
- Not changing the brand mark, fonts, or colour palette.

## 4. Anti-goals (what would make this a failure even if it renders)

- A user with a restricted role (payroll manager, manager, employee) reaches a
  module they cannot use today.
- The dashboard shows numbers that are wrong, stale-looking, or contradictory
  (e.g. "12 pending" while the approvals inbox is empty).
- Cards that only look alive: fake toggles, hardcoded schedules, invented
  percentages.
- Dashboard load makes 8+ sequential requests and feels slower than today.
- The sidebar pushes content off-screen at 1280×720.

## 5. Constraints

| Constraint | Detail |
|---|---|
| E2E suite | 56 tests. 34 references to `getByLabel("ออกจากระบบ")`, 2 to `data-testid="lang-toggle"`, several `getByRole("link", { name: "Employees" / "Payroll" / "Settings" })`, plus role-restriction tests in `e2e/roles.spec.ts`. |
| i18n | All user-visible strings go through `t()` (`lib/i18n-dicts.ts`, 341 keys, th + en). New keys must be added to both dictionaries. |
| Permissions | `Me.roles` / `Me.permissions` from `api/v1/auth/me`. Existing gating logic in `Shell.tsx` must be preserved verbatim (including the `/clock` fallback for denied direct URLs). |
| No new deps | Charts and heatmap are hand-built SVG/CSS. No chart library. |
| Visual tokens | `--sage/--teal/--mint/--warning/--danger` + `Inter`/`Noto Sans Thai`/`Instrument Serif` from `globals.css`. |
| Thai-first | Thai labels, พ.ศ. dates, `฿` amounts; EN via the existing toggle. |

## 6. Approaches considered

**A. Sidebar shell + real-data dashboard (chosen).**
Rewrite `Shell.tsx` as sidebar + command bar; rebuild the dashboard page around
existing endpoints. Moderate work, no new deps, keeps every URL.

**B. Keep the top nav, only restyle the dashboard.**
Cheapest, but leaves the 26-item overflow problem unsolved, and the reference's
most valuable element — grouped navigation — is exactly what we need. Rejected.

**C. Full Floware treatment (3D isometric scene, floating cards).**
Visually striking, but the 3D warehouse has no analogue in HR data, the
implementation is heavy, and it would dominate the design budget without making
any task faster. Rejected.

**Why A:** it fixes a real navigation problem and puts actionable work on the
landing page, using only data we already serve.

## 7. What changes

| File | Change |
|---|---|
| `web/components/Shell.tsx` | Rewritten: sidebar + command bar. Nav arrays grouped; permission filtering unchanged; adds mobile drawer state and nav search filter. |
| `web/app/globals.css` | Add `.app-shell`, `.side`, `.side-group`, `.topbar`, `.search`, `.kpi-grid`, `.appr-grid`, `.heat`, `.recent` etc. Keep existing `.card/.btn/.badge/.table` primitives. Remove `.pills` only if nothing else uses it. |
| `web/app/(app)/page.tsx` | Rebuilt dashboard (sections below). |
| `web/lib/i18n-dicts.ts` | New keys for groups, KPIs and card titles (th + en). |
| `web/e2e/app.spec.ts` / `roles.spec.ts` | Update only where the shell genuinely changed (see §11). |

No routing changes: `/employees`, `/payroll`, … keep their URLs, so deep links,
bookmarks, and `?run=` query links keep working.

## 8. Navigation model

Sidebar groups (order matters; groups with no visible item are hidden). Every
destination in today's `NAV` and `ADMIN_NAV` must appear in exactly one group —
nothing is dropped:

| Group | Items (`nav.*` keys already exist) |
|---|---|
| — | ภาพรวม (`/`) — only for users with `employee.read_all`, as today |
| งานบุคคล | พนักงาน, เอกสาร, เริ่มงาน, สรรหาบุคลากร, อบรม, ประเมินผล, เป้าหมาย† |
| เวลาและสิทธิ | ลงเวลา, ปฏิทินทีม, การลา, OT, ค่าใช้จ่าย |
| ค่าตอบแทน | รอบเงินเดือน, เงินกู้\*, พนักงานลาออก\*, รายงาน\*, แบบฟอร์มสลิป\* |
| ของฉัน | สลิปของฉัน, ผลการประเมินของฉัน, ความเป็นส่วนตัว |
| ตั้งค่า | ผู้ใช้\*, ตั้งค่า\*, แบบฟอร์ม\*, ประวัติ\* |

`*` = gated exactly as today (`isAdmin` / `canReports`).
`†` = visible only with `performance.manage_direct` or HR, as today.
`ลงเวลา` stays visible to employees (a test asserts this).

The existing filter expression is moved, not rewritten: it becomes a
`visible(item, me)` predicate with the same outcomes. The redirect guard
(`adminOnly` / `payrollOnly` / `hrOnly` / `attendanceOnly` / `directOnly`) keeps
its current behaviour, including `router.replace("/clock")`.

Command bar (top, sticky): search (filters sidebar labels by substring, case-
and diacritic-insensitive), current period chip, `lang-toggle` (same testid),
avatar with name + role tooltip, logout button with `aria-label="ออกจากระบบ"`.

Mobile (`< 900px`): sidebar becomes an off-canvas panel opened by a menu button
in the command bar; closing on navigation; a backdrop closes it.

## 9. Dashboard sections → who sees what, and from where

The page is **role-shaped**: each role sees the cards its permissions allow, and
nothing else. No card is rendered in a disabled state for a role that cannot
read its endpoint. All amounts and counts come from live endpoints; each card
has its own loading, empty, and error state.

| Section | hr_admin | payroll_manager | manager | employee |
|---|---|---|---|---|
| KPI: headcount (`employee.read_all`) | ✓ | ✓ | – | – |
| KPI: latest net pay + PND.1 tax | ✓ | ✓ | – | – |
| KPI: pending approvals (`*_request.view_team`) | ✓ | – | ✓ | – |
| KPI: own latest payslip (`payslip.view_self`) | – | – | ✓ | ✓ |
| งานรออนุมัติ card grid | ✓ | – | ✓ | – |
| งานที่ต้องติดตาม (docs ≤30d, draft runs) | ✓ | ✓ | – | – |
| OT รายเดือน | ✓ | ✓ | – | – |
| สรุปรอบล่าสุด (dark panel) | ✓ | ✓ | – | – |
| พนักงานตามแผนก (donut) | ✓ | ✓ | – | – |
| กิจกรรมล่าสุด (`audit_log.read`) | ✓ | – | – | – |
| ของฉัน: สลิป / ผลประเมิน / วันลา / OT | – | – | ✓ | ✓ |

Manager's "ของฉัน" plus team cards is what makes the first-approver role
useful; the employee sees only self-service.

| Section | Source | Notes |
|---|---|---|
| KPI: พนักงานที่ใช้งาน | `GET /api/v1/reports/headcount` → `active + probation + leave` | Must equal the donut total. No invented delta. |
| KPI: ยอดจ่ายสุทธิรอบล่าสุด | GraphQL `payrollRuns(limit:1)` → `totalNet` | existing query. |
| KPI: ภาษีหัก ณ ที่จ่าย | same run → `totalTax` | label states the period, e.g. "รอบ ต.ค. 2569". |
| KPI: งานรออนุมัติ | `leave/approvals` + `ot/approvals` + `claims/approvals` lengths | one card, breakdown in the sub-line. |
| งานรออนุมัติ (card grid) | the three approval lists, flattened, max 6 | each card shows type, employee, **business date** (leave range / OT work date / claim date), and links to the owning module. The APIs expose no submission timestamp, so cards sort by business date, newest first — they must not claim "ยื่นเมื่อ …". |
| งานที่ต้องติดตาม | `documents/expiring` (≤30 days), draft payroll runs, open cycles | replaces the reference's scheduled+toggles card. |
| OT รายเดือน | `GET /api/v1/reports/ot-trend?from&to` (12 months) | bar chart, hand-built SVG. |
| สรุปรอบล่าสุด (dark panel) | latest run totals (`totalGross/totalTax/totalSso/totalPf/totalNet`) | CTA opens `/payroll` (see §15.9). |
| พนักงานตามแผนก | `reports/headcount` → `by_department` | donut; `by_department` already aggregates non-terminated staff, so no extra request and the total matches the KPI. |
| กิจกรรมล่าสุด | `GET /api/v1/audit` (limit 4) | action + relative time only. `/audit` exposes `actor_type` + a UUID, not a name, so the card does not show an actor. Action keys are Thai-mapped for the handful we display, raw key otherwise. |
| ของฉัน | `/api/v1/me/payslips`, `/api/v1/me/reviews`, `/api/v1/me/leave`, `/api/v1/me/ot` | one compact card each with the latest item and a link to the full page. |

Request budget by role — hr_admin: 8 parallel requests (headcount, run, 3
approvals, expiring, ot-trend, audit); payroll_manager: 6 (no approvals, no
audit); manager: 4 (3 approvals + own payslips); employee: 2–4 (self-service
only). Each card owns its own loading/error state, so one slow or forbidden
endpoint degrades a single card instead of blocking the page.

## 10. Visual language

- Sidebar: `--surface`, 236px, 1px `--line` right border; active item
  `--sage-soft` background + `--teal` text; group labels 10px uppercase
  `--text-3`.
- Cards: `--surface`, `1px --line`, radius 20px, `--shadow` (flat, not floaty).
- Accents per card type: teal (payroll), sage (people), warning (deadlines),
  plain (documents). No new hues.
- Type: display serif only for the page title; everything else Inter/Noto Sans
  Thai at 11–26px. Tabular numerals for money.
- One dark surface only (the "สรุปรอบล่าสุด" panel, `--teal`), as in the
  prototype.
- Icons: reuse `components/Icon.tsx` names where they exist; otherwise a small
  text glyph like today's nav.

## 11. Test impact

Must still pass unchanged (no edit):
- logout flows (34 uses of `getByLabel("ออกจากระบบ")`) — the button keeps that
  accessible name.
- `lang-toggle` testid and the EN nav-label assertions — sidebar links keep
  their `t()` text and `href`.
- role restriction tests — same predicate, same redirects.

Expected edits (deliberate, small):
- **One redirect-dependent test.** `e2e/app.spec.ts` logs in as the invited
  employee and expects the heading "สลิปของฉัน" immediately; that relied on `/`
  redirecting non-HR users to `/me/payslips`. With role-shaped dashboards the
  spec must navigate to `/me/payslips` explicitly (see §14).
- Any assertion that relies on the dashboard's old copy (`สวัสดี…`, "รอบเงินเดือน
  ล่าสุด", "ยังไม่มีรอบเงินเดือน") either keeps that copy on the new page or the
  spec is updated to the new section titles.
- If the old pill nav is removed, no test currently targets `.pills`, so no test
  should need to change for that — to be confirmed by running the suite.

Verification: full 56-test clean-DB suite plus `npx tsc --noEmit`. Do not claim
done on a subset.

## 12. Rollout

1. Shell (sidebar + topbar + responsive + i18n keys), dashboard left as-is.
   Run suite → expect green with zero spec edits.
2. Dashboard sections, one PR-sized step each: KPIs → approvals → follow-ups →
   OT chart → summary/donut/activity. Run suite after each.
3. Visual pass + mobile check at 390px and 1280px.

## 13. Left to the builder

- Exact spacing scale, border radii, and card order inside each grid row.
- Whether to split `Shell.tsx` into `Sidebar`/`Topbar` components (only if the
  file becomes unwieldy; the default is one file).
- Whether the nav search filters by label only or also by group name.
- Relative-time formatting ("2 ชม. ที่แล้ว") — use `Intl.RelativeTimeFormat`.
- Which icon set to use where `Icon.tsx` has no match.

## 14. Decision: every role gets the dashboard

Chosen: **each role sees the cards it can actually use** (matrix in §9). This
replaces the current behaviour where `/` redirects anyone without
`employee.read_all` to `/me/payslips`.

Consequences:

- **Deliberate test change.** `e2e/app.spec.ts` logs in as the invited employee
  and immediately expects the heading "สลิปของฉัน" — that only worked because of
  the redirect. Under role-shaped dashboards the employee lands on `/` and sees
  their dashboard. The spec must navigate to `/me/payslips` explicitly (or
  assert the new dashboard first). This is the only test that depends on the
  redirect.
- Sidebar `ภาพรวม` becomes visible to every authenticated user; the per-card
  gating above replaces the page-level gate.
- The dashboard's `useEffect` redirect is removed. The Shell's direct-URL guard
  is unchanged, so a user who types `/payroll` without permission still lands on
  `/clock` exactly as today.
- Manager role gains real value (their approval queue) without any new
  permission or endpoint.

## 15. Decisions I made that you should check

1. **Heatmap → OT-by-month bars.** The 4×11 attendance heatmap from the
   reference needs an aggregate endpoint; v1 uses real OT data instead.
2. **Scheduled card → follow-up checklist.** Toggles that persist nowhere would
   be decorative; the card shows real counts instead.
3. **Search = nav filter only.** No global search in v1.
4. **KPI deltas are omitted** where there is no historical comparison, rather
   than inventing a percentage.
5. **Sidebar groups mirror how HR staff work**, not the code's module order;
   every existing destination is kept (see §8).
6. **Approval cards use business dates**, because the approval APIs expose no
   submission timestamp and adding one would break the no-backend-change rule.
7. **`/audit` shows action + time, no actor**, because the endpoint returns a
   UUID rather than a name.
8. **Donut uses `headcount.by_department`**, so the donut total and the
   headcount KPI can never disagree.
9. **`/payroll?run=<id>` is not honoured today** — the payroll page ignores query
   params. v1's CTA links to `/payroll`; teaching payroll to open a specific run
   is a separate change.

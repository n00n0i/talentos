# Sidebar Shell + Role-Shaped Dashboard — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the horizontal pill nav with a grouped sidebar and turn the landing page into a role-shaped dashboard built from existing endpoints.

**Architecture:** A pure nav model (`lib/nav.ts`) decides which destinations a user may see, using the exact same rules as today's `Shell.tsx`. `Shell.tsx` renders that model as a sidebar plus a sticky command bar. The dashboard (`app/(app)/page.tsx`) renders sections chosen by a pure role→sections map (`lib/dashboard.ts`), each section owning its own request and empty state.

**Tech Stack:** Next.js 15 App Router (client components), TypeScript, hand-written CSS in `app/globals.css`, vitest for pure logic, Playwright for E2E.

**Spec:** `docs/superpowers/specs/2026-11-02-dashboard-sidebar-redesign-design.md`

## Global Constraints

- No new dependencies. Charts/donut are inline SVG.
- No backend or schema changes; use only endpoints that exist today.
- Every user-visible string goes through `t()`; add each key to **both** `th` and `en` in `lib/i18n-dicts.ts`.
- Keep `data-testid="lang-toggle"` and the logout control's accessible name `ออกจากระบบ` (34 E2E references).
- Keep every existing route URL unchanged.
- `npx tsc --noEmit` must exit 0; `npx vitest run` must pass; the 56-test E2E suite must pass on a clean DB.
- E2E base URL in this environment: `http://127.0.0.1:3000`; setup token `e2e-setup-token`.
- Clean DB before a full suite run: `docker exec loans-postgres-1 psql -U talentos -d talentos -c "DELETE FROM tenants;"`
- Approved six deliberate behaviour changes (spec §15): business dates on approval cards, no actor name in activity, no fake toggles, OT bars instead of a heatmap, nav-only search, `/payroll` without `?run=`.

## Review Focus

- **Payroll manager on `/`** — has `employee.read_all` and `payroll_run.export_report`, but gets 403 from the three approval endpoints and `/audit`; those cards must disappear entirely, not render an error.
- **Employee with no payslip yet** — fresh tenant, `employee_linked: false` or empty list; the self-service cards must show a "ยังไม่มี…" state, never a blank panel or `NaN`.
- **Employees with `department IS NULL`** — must land in the `ไม่ระบุ` bucket and keep the donut total equal to the headcount KPI.
- **Long Thai names in approval cards** — `employee_name` can be long; the card must truncate without breaking the grid at 1280px.
- **Sidebar drawer at 390px** — opening must trap no focus, closing (backdrop, Escape) must work, and nav must not overflow horizontally.

---

## File Structure

| File | Responsibility |
|---|---|
| `web/lib/nav.ts` (create) | Nav groups + `visibleHrefs` / `visibleNav` pure rules. No JSX, no React. |
| `web/test/nav.test.ts` (create) | Unit tests for the permission rules (all four roles). |
| `web/lib/dashboard.ts` (create) | Role detection, section matrix, money/relative-time formatting, OT trend range. |
| `web/test/dashboard.test.ts` (create) | Unit tests for those pure helpers. |
| `web/components/Shell.tsx` (modify) | Sidebar + command bar + mobile drawer. Renders `visibleNav`. |
| `web/app/globals.css` (modify) | `.app-shell`, `.side`, `.topbar`, `.kpi-grid`, `.appr-grid`, `.chart`, `.donut`, `.recent`; remove `.pills` only if unreferenced. |
| `web/app/(app)/page.tsx` (modify) | Role-shaped dashboard. |
| `web/lib/i18n-dicts.ts` (modify) | `nav.group.*`, `dash.*` keys (th + en). |
| `web/e2e/app.spec.ts` (modify) | One redirect-dependent assertion (Task 4). |

---

### Task 1: Nav model with today's permission rules

**Files:**
- Create: `web/lib/nav.ts`
- Create: `web/test/nav.test.ts`

**Interfaces:**
- Consumes: `Me` from `@/lib/api` (`roles: string[]`, `permissions: string[]`).
- Produces:
  - `export type NavItem = { href: string; labelKey: string; icon: IconName }`
  - `export type NavGroup = { id: string; labelKey: string | null; items: NavItem[] }`
  - `export const NAV_GROUPS: NavGroup[]`
  - `export function visibleHrefs(me: Me | null): Set<string>`
  - `export function visibleNav(me: Me | null): NavGroup[]`

- [ ] **Step 1: Write the failing test**

```ts
// web/test/nav.test.ts
import { describe, expect, it } from "vitest";
import { NAV_GROUPS, visibleHrefs, visibleNav } from "@/lib/nav";
import type { Me } from "@/lib/api";

const me = (roles: string[], permissions: string[]): Me => ({
  user_id: "u", email: "e@x", full_name_th: "ผู้ใช้", full_name_en: null,
  tenant_id: "t", tenant_name: "T", entity_id: null, roles, permissions,
  last_login_at: null,
});

const HR = me(["hr_admin"], ["employee.read_all", "attendance.read", "payroll_run.export_report", "audit_log.read"]);
const PAYROLL = me(["payroll_manager"], ["employee.read_all", "attendance.read", "payroll_run.export_report"]);
const MANAGER = me(["manager"], ["performance.manage_direct", "attendance.read"]);
const EMPLOYEE = me(["employee"], []);

describe("visibleHrefs", () => {
  it("hr_admin sees admin destinations and the overview", () => {
    const hrefs = visibleHrefs(HR);
    for (const h of ["/", "/employees", "/payroll", "/users", "/settings", "/audit", "/loans", "/final-settlement"]) {
      expect(hrefs.has(h)).toBe(true);
    }
  });

  it("payroll_manager sees payroll and people but no admin destinations", () => {
    const hrefs = visibleHrefs(PAYROLL);
    expect(hrefs.has("/payroll")).toBe(true);
    expect(hrefs.has("/reports")).toBe(true);
    expect(hrefs.has("/users")).toBe(false);
    expect(hrefs.has("/settings")).toBe(false);
    expect(hrefs.has("/audit")).toBe(false);
  });

  it("employee sees the overview plus self-service only", () => {
    const hrefs = visibleHrefs(EMPLOYEE);
    expect([...hrefs].sort()).toEqual(
      ["/", "/claims", "/clock", "/leave", "/me/payslips", "/me/reviews", "/ot", "/privacy"].sort(),
    );
  });

  it("manager additionally sees performance and goals", () => {
    const hrefs = visibleHrefs(MANAGER);
    expect(hrefs.has("/performance")).toBe(true);
    expect(hrefs.has("/goals")).toBe(true);
    expect(hrefs.has("/employees")).toBe(false);
  });

  it("returns every href in NAV_GROUPS at most once", () => {
    const all = NAV_GROUPS.flatMap((g) => g.items.map((i) => i.href));
    expect(new Set(all).size).toBe(all.length);
  });
});

describe("visibleNav", () => {
  it("drops empty groups and keeps group order", () => {
    const groups = visibleNav(EMPLOYEE);
    expect(groups.every((g) => g.items.length > 0)).toBe(true);
    const ids = groups.map((g) => g.id);
    expect(ids.indexOf("people")).toBe(-1);
    expect(ids.indexOf("mine")).toBeGreaterThan(-1);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && npx vitest run test/nav.test.ts`
Expected: FAIL — `Failed to resolve import "@/lib/nav"`.

- [ ] **Step 3: Implement `web/lib/nav.ts`**

`NAV_GROUPS` uses existing `nav.*` keys and `IconName`s. `visibleHrefs` must reproduce today's expression exactly:
`canReports` = has any of `hr_admin|payroll_manager`; `canHr` = `employee.read_all`; `isAdmin` = `hr_admin`.
Non-`canHr` users get only: `/clock`, `/leave`, `/ot`, `/claims`, `/me/payslips`, `/privacy`, `/me/reviews`, plus `/performance` + `/goals` when `performance.manage_direct`, plus `/attendance` + `/calendar` when `attendance.read`.

```ts
import type { IconName } from "@/components/Icon";
import type { Me } from "@/lib/api";

export type NavItem = { href: string; labelKey: string; icon: IconName };
export type NavGroup = { id: string; labelKey: string | null; items: NavItem[] };

export const NAV_GROUPS: NavGroup[] = [
  { id: "overview", labelKey: null, items: [{ href: "/", labelKey: "nav.overview", icon: "overview" }] },
  { id: "people", labelKey: "nav.group.people", items: [
    { href: "/employees", labelKey: "nav.employees", icon: "users" },
    { href: "/documents", labelKey: "nav.documents", icon: "file" },
    { href: "/onboarding", labelKey: "nav.onboarding", icon: "users" },
    { href: "/recruitment", labelKey: "nav.recruitment", icon: "users" },
    { href: "/training", labelKey: "nav.training", icon: "file" },
    { href: "/performance", labelKey: "nav.performance", icon: "file" },
    { href: "/goals", labelKey: "nav.goals", icon: "file" },
  ]},
  { id: "time", labelKey: "nav.group.time", items: [
    { href: "/attendance", labelKey: "nav.attendance", icon: "clock" },
    { href: "/calendar", labelKey: "nav.calendar", icon: "clock" },
    { href: "/clock", labelKey: "nav.clock", icon: "clock" },
    { href: "/leave", labelKey: "nav.leave", icon: "clock" },
    { href: "/ot", labelKey: "nav.ot", icon: "clock" },
    { href: "/claims", labelKey: "nav.claims", icon: "wallet" },
  ]},
  { id: "pay", labelKey: "nav.group.pay", items: [
    { href: "/payroll", labelKey: "nav.payroll", icon: "wallet" },
    { href: "/loans", labelKey: "nav.loans", icon: "wallet" },
    { href: "/final-settlement", labelKey: "nav.finalSettlement", icon: "users" },
    { href: "/reports", labelKey: "nav.reports", icon: "file" },
    { href: "/payslip", labelKey: "nav.payslip", icon: "file" },
  ]},
  { id: "mine", labelKey: "nav.group.mine", items: [
    { href: "/me/payslips", labelKey: "nav.myPayslips", icon: "file" },
    { href: "/me/reviews", labelKey: "nav.myReviews", icon: "file" },
    { href: "/privacy", labelKey: "nav.privacy", icon: "file" },
  ]},
  { id: "admin", labelKey: "nav.group.admin", items: [
    { href: "/users", labelKey: "nav.users", icon: "user" },
    { href: "/settings", labelKey: "nav.settings", icon: "settings" },
    { href: "/forms", labelKey: "nav.forms", icon: "file" },
    { href: "/audit", labelKey: "nav.audit", icon: "file" },
  ]},
];

const EMPLOYEE_ONLY = ["/clock", "/leave", "/ot", "/claims", "/me/payslips", "/privacy", "/me/reviews"];
const DIRECT_ONLY = ["/performance", "/goals"];
const ATTENDANCE_ONLY = ["/attendance", "/calendar"];

export function visibleHrefs(me: Me | null): Set<string> {
  const all = NAV_GROUPS.flatMap((g) => g.items.map((i) => i.href));
  if (!me) return new Set(all);
  const isAdmin = me.roles.includes("hr_admin");
  const canHr = me.permissions.includes("employee.read_all");
  const canReports = me.roles.some((r) => ["hr_admin", "payroll_manager"].includes(r));
  const canDirect = me.permissions.includes("performance.manage_direct");
  const canAttend = me.permissions.includes("attendance.read");

  if (canHr) {
    const allowed = new Set(all);
    if (!canReports) for (const h of ["/loans", "/final-settlement", "/reports"]) allowed.delete(h);
    if (!isAdmin) for (const h of ["/loans", "/final-settlement", "/users", "/settings", "/forms", "/audit", "/payslip"]) allowed.delete(h);
    return allowed;
  }
  // Role-shaped dashboard (spec §14): everyone gets ภาพรวม.
  const allowed = new Set(["/", ...EMPLOYEE_ONLY]);
  if (canDirect) DIRECT_ONLY.forEach((h) => allowed.add(h));
  if (canAttend) ATTENDANCE_ONLY.forEach((h) => allowed.add(h));
  return allowed;
}

export function visibleNav(me: Me | null): NavGroup[] {
  const allowed = visibleHrefs(me);
  return NAV_GROUPS
    .map((g) => ({ ...g, items: g.items.filter((i) => allowed.has(i.href)) }))
    .filter((g) => g.items.length > 0);
}
```

- [ ] **Step 4: Add the four group keys to both dictionaries**

In `web/lib/i18n-dicts.ts` add to `th`: `"nav.group.people": "งานบุคคล"`, `"nav.group.time": "เวลาและสิทธิ"`, `"nav.group.pay": "ค่าตอบแทน"`, `"nav.group.mine": "ของฉัน"`, `"nav.group.admin": "ตั้งค่า"`. Add the same keys to `en`: `"People"`, `"Time & leave"`, `"Payroll"`, `"Mine"`, `"Settings"`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd web && npx vitest run test/nav.test.ts && npx tsc --noEmit`
Expected: PASS, tsc exit 0.

- [ ] **Step 6: Commit**

```bash
git add web/lib/nav.ts web/test/nav.test.ts web/lib/i18n-dicts.ts
git commit -m "feat(web): extract nav model with existing permission rules"
```

---

### Task 2: Sidebar shell (dashboard unchanged)

**Files:**
- Modify: `web/components/Shell.tsx` (replace nav rendering, keep every other effect)
- Modify: `web/app/globals.css` (add shell classes near the existing `.shell` block at line 111; leave `.pills` in place for now)
- Modify: `web/lib/i18n-dicts.ts` (add `shell.menu`, `shell.closeMenu`, `shell.searchPlaceholder` if absent)

**Interfaces:**
- Consumes: `visibleNav(me)` from Task 1; `t()` from `@/lib/i18n`; `Icon`.
- Produces: sidebar markup with `data-testid="side-nav"` and the menu button `data-testid="menu-toggle"`; on mobile an element `data-testid="side-backdrop"`.

- [ ] **Step 1: Add the failing assertion to the existing i18n-free smoke path**

There is no unit seam for JSX here, so the check is a Playwright assertion added to `web/e2e/app.spec.ts` inside the existing `"switches to English via the lang-toggle…"` test, after the current assertions:

```ts
await expect(page.getByTestId("side-nav")).toBeVisible();
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd web && E2E_BASE_URL=http://127.0.0.1:3000 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test -g "switches to English"`
Expected: FAIL — `data-testid="side-nav"` not found.

- [ ] **Step 3: Rewrite `Shell.tsx`'s render**

Keep unchanged: the `gw("api/v1/auth/me")` effect, the service-worker registration, the redirect guard effect (`/clock` fallback), `initials()`, `logout()`, the date chip, the language toggle (`data-testid="lang-toggle"`), the avatar, and the logout button whose `aria-label` is `t("shell.logout")`.

Replace the header/nav block with:

```tsx
<div className="app-shell">
  <aside className={open ? "side open" : "side"} data-testid="side-nav">
    <Link href="/" className="side-brand">…brand mark + TalentOS…</Link>
    {groups.map((g) => (
      <div className="side-group" key={g.id}>
        {g.labelKey && <span>{t(g.labelKey)}</span>}
        {g.items.map((i) => (
          <Link key={i.href} href={i.href} className={pathname === i.href ? "active" : undefined}>
            <span className="ico"><Icon name={i.icon} size={15} /></span>
            {t(i.labelKey)}
          </Link>
        ))}
      </div>
    ))}
    <div className="side-user">…initials, displayName, roleLabel…</div>
  </aside>
  {open && <button className="side-backdrop" data-testid="side-backdrop" aria-label={t("shell.closeMenu")} onClick={() => setOpen(false)} />}
  <div className="main">
    <div className="topbar">
      <button className="menu-btn" data-testid="menu-toggle" aria-label={t("shell.menu")} onClick={() => setOpen((v) => !v)}>☰</button>
      <label className="search"><input placeholder={t("shell.searchPlaceholder")} value={q} onChange={(e) => setQ(e.target.value)} /></label>
      <span className="date-chip">{today}</span>
      …lang-toggle, avatar, logout…
    </div>
    <main className="content">{children}</main>
  </div>
</div>
```

`const groups = visibleNav(me)`; close the drawer on navigation with `useEffect(() => setOpen(false), [pathname])`; `q` filters items by `t(i.labelKey).toLowerCase().includes(q.toLowerCase())` before rendering.

- [ ] **Step 4: Add the shell CSS**

In `web/app/globals.css` add `.app-shell` (grid `236px 1fr`, full viewport, no card wrapper), `.side` (sticky, `--surface`, right border, scroll), `.side-group`, `.side a`, `.side a.active` (`--sage-soft`/`--teal`), `.side-user`, `.topbar` (sticky), `.search`, `.menu-btn` (hidden ≥900px), `.side-backdrop` (fixed, hidden ≥900px), and a `@media (max-width: 900px)` block that turns `.side` into `position: fixed; transform: translateX(-100%)` and `.side.open` into `translateX(0)`. Keep all existing `.card/.btn/.badge/.table` rules untouched.

- [ ] **Step 5: Build and run the full suite**

```bash
cd web && npm run build
rm -rf .next/standalone/.next/static .next/standalone/public
cp -r .next/static .next/standalone/.next/ && cp -r public .next/standalone/
for pid in $(ss -tlnp 2>/dev/null | grep ':3000' | grep -oP 'pid=\K[0-9]+' | sort -u); do kill "$pid"; done
cd .next/standalone && setsid env TALENTOS_API_URL=http://127.0.0.1:18479 TALENTOS_COOKIE_SECURE=false PORT=3000 HOSTNAME=0.0.0.0 node server.js > /tmp/opencode/web-server.log 2>&1 < /dev/null &
sleep 4 && curl -s -o /dev/null -w "web=%{http_code}\n" http://127.0.0.1:3000/login
docker exec loans-postgres-1 psql -U talentos -d talentos -c "DELETE FROM tenants;"
E2E_BASE_URL=http://127.0.0.1:3000 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test
```
Expected: 56 passed. If the EN-nav test now finds links only inside `side-nav`, still passes because `getByRole("link", { name: "Employees" })` matches sidebar links.

- [ ] **Step 6: Commit**

```bash
git add web/components/Shell.tsx web/app/globals.css web/lib/i18n-dicts.ts web/e2e/app.spec.ts
git commit -m "feat(web): grouped sidebar shell with command bar and mobile drawer"
```

---

### Task 3: Dashboard role matrix and formatting helpers

**Files:**
- Create: `web/lib/dashboard.ts`
- Create: `web/test/dashboard.test.ts`

**Interfaces:**
- Produces:
  - `export type Role = "hr_admin" | "payroll_manager" | "manager" | "employee"`
  - `export function primaryRole(me: Me): Role`
  - `export type SectionId = "headcount" | "net" | "tax" | "approvals" | "ownPayslip" | "selfService" | "approvalsGrid" | "followUps" | "otTrend" | "runSummary" | "departments" | "activity"`
  - `export function sectionsFor(role: Role): SectionId[]`
  - `export function compactThb(n: number | string): string`
  - `export function relativeTh(iso: string, now?: Date): string`
  - `export function otTrendQuery(months = 12, now?: Date): string` → `"from=YYYY-MM-01&to=YYYY-MM-DD"`

- [ ] **Step 1: Write the failing test**

```ts
import { describe, expect, it } from "vitest";
import { compactThb, otTrendQuery, primaryRole, relativeTh, sectionsFor } from "@/lib/dashboard";
import type { Me } from "@/lib/api";

const me = (roles: string[]): Me => ({
  user_id: "u", email: "e@x", full_name_th: "x", full_name_en: null, tenant_id: "t",
  tenant_name: "T", entity_id: null, roles, permissions: [], last_login_at: null,
});

describe("primaryRole", () => {
  it("prefers hr_admin over everything", () => {
    expect(primaryRole(me(["manager", "hr_admin"]))).toBe("hr_admin");
  });
  it("maps the other three roles", () => {
    expect(primaryRole(me(["payroll_manager"]))).toBe("payroll_manager");
    expect(primaryRole(me(["manager"]))).toBe("manager");
    expect(primaryRole(me(["employee"]))).toBe("employee");
  });
});

describe("sectionsFor", () => {
  it("hr_admin gets payroll, approvals, follow-ups and activity", () => {
    const s = sectionsFor("hr_admin");
    for (const id of ["headcount", "net", "tax", "approvals", "approvalsGrid", "followUps", "otTrend", "runSummary", "departments", "activity"]) {
      expect(s).toContain(id);
    }
    expect(s).not.toContain("selfService");
  });
  it("payroll_manager gets payroll cards but no approvals or activity", () => {
    const s = sectionsFor("payroll_manager");
    expect(s).toContain("headcount");
    expect(s).toContain("runSummary");
    expect(s).not.toContain("approvalsGrid");
    expect(s).not.toContain("activity");
  });
  it("manager gets approvals plus self-service", () => {
    const s = sectionsFor("manager");
    expect(s).toContain("approvalsGrid");
    expect(s).toContain("selfService");
    expect(s).not.toContain("headcount");
    expect(s).not.toContain("runSummary");
  });
  it("employee gets self-service only", () => {
    const s = sectionsFor("employee");
    expect(s).toContain("selfService");
    expect(s).not.toContain("approvalsGrid");
    expect(s).not.toContain("tax");
  });
});

describe("compactThb", () => {
  it("uses ล้าน with two decimals from one million", () => {
    expect(compactThb(3_421_000)).toBe("฿3.42 ล้าน");
  });
  it("groups smaller amounts with thousands separators and no decimals", () => {
    expect(compactThb(184_300)).toBe("฿184,300");
    expect(compactThb(0)).toBe("฿0");
  });
  it("accepts numeric strings from the API", () => {
    expect(compactThb("3421000.00")).toBe("฿3.42 ล้าน");
  });
});

describe("relativeTh", () => {
  const now = new Date("2026-11-02T12:00:00Z");
  it("formats hours and days in Thai", () => {
    expect(relativeTh("2026-11-02T10:00:00Z", now)).toContain("ชม.");
    expect(relativeTh("2026-10-31T12:00:00Z", now)).toContain("วัน");
  });
  it("falls back to a date beyond 30 days", () => {
    expect(relativeTh("2026-01-01T00:00:00Z", now)).toMatch(/\d/);
  });
});

describe("otTrendQuery", () => {
  it("spans 12 months back from the given date", () => {
    expect(otTrendQuery(12, new Date("2026-11-15T00:00:00Z"))).toBe("from=2025-12-01&to=2026-11-15");
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && npx vitest run test/dashboard.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `web/lib/dashboard.ts`**

Use `Intl.RelativeTimeFormat("th", { numeric: "auto" })` for `relativeTh` (minutes < 60, hours < 24, days < 30, else `toLocaleDateString("th-TH")`). `compactThb` must handle both `number` and decimal strings (`Number(value)`). `primaryRole` precedence: `hr_admin` → `payroll_manager` → `manager` → `employee`.

- [ ] **Step 4: Add dashboard i18n keys**

Add to both dicts: `dash.title` ("ภาพรวม" / "Overview"), `dash.subtitle`, `dash.kpi.headcount`, `dash.kpi.net`, `dash.kpi.tax`, `dash.kpi.approvals`, `dash.kpi.ownPayslip`, `dash.approvals.title`, `dash.followUps.title`, `dash.ot.title`, `dash.summary.title`, `dash.dept.title`, `dash.activity.title`, `dash.mine.title`, `dash.empty.*` for each section, `dash.unknownDept` ("ไม่ระบุ" / "Unassigned").

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd web && npx vitest run && npx tsc --noEmit`
Expected: PASS, tsc exit 0.

- [ ] **Step 6: Commit**

```bash
git add web/lib/dashboard.ts web/test/dashboard.test.ts web/lib/i18n-dicts.ts
git commit -m "feat(web): dashboard role matrix and formatting helpers"
```

---

### Task 4: Dashboard page — route by role, KPI row, self-service cards

**Files:**
- Modify: `web/app/(app)/page.tsx` (full rewrite)
- Modify: `web/e2e/app.spec.ts` (one assertion, see Step 1)
- Modify: `web/app/globals.css` (`.kpi-grid`, `.kpi`, `.mine-grid`, `.dash-grid`, `.stack`)

**Interfaces:**
- Consumes: `primaryRole`, `sectionsFor`, `compactThb`, `relativeTh`, `otTrendQuery` (Task 3); `gw`, `gql`, `handleApiError`, `Me`, `thDate`, `thbShort`; `Icon`.
- Produces: `data-testid="dash-title"`, `data-testid="kpi-row"`, `data-testid="kpi-<id>"`, `data-testid="mine-grid"` — later tasks append sections to the same page.

- [ ] **Step 1: Update the redirect-dependent assertion and add dashboard assertions (deliberate, spec §14)**

In `web/e2e/app.spec.ts`, find the payslip test that logs in as `invitedEmail` and immediately asserts the heading `สลิปของฉัน`. Replace that with:

```ts
// employee lands on the dashboard, not on /me/payslips
await expect(page.getByTestId("dash-title")).toBeVisible();
await expect(page.getByTestId("mine-grid")).toBeVisible();
await page.goto("/me/payslips");
await expect(page.getByRole("heading", { name: "สลิปของฉัน" })).toBeVisible();
```

- [ ] **Step 2: Run that test to verify it fails**

Run: `cd web && E2E_BASE_URL=http://127.0.0.1:3000 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test -g "acknowledge"`
Expected: FAIL — `dash-title` not found (the old page still redirects to `/me/payslips`).

- [ ] **Step 4: Rewrite `page.tsx`**

Remove the `if (!profile.permissions.includes("employee.read_all")) router.replace("/me/payslips")` redirect. Structure:

```tsx
const role = me ? primaryRole(me) : null;
const sections = role ? sectionsFor(role) : [];
```

Render `dash-title` header, then `kpi-row` containing only the KPIs in `sections`, then a `.dash-grid` whose left `.stack` and right `.stack` receive the remaining sections in order. Each section is a small local component with its own `useEffect` fetch, `loading`, `error` (a muted line, never a red page), and empty state.

KPI requests for this task:
- `headcount`: `gw<{ active: number; probation: number; leave: number; by_department: { department: string; count: number }[] }>("api/v1/reports/headcount")`, value `active + probation + leave`.
- `net` / `tax`: GraphQL `{ payrollRuns(limit: 1) { periodStart periodEnd status totalNet totalTax } }` (reuse the existing query shape).
- `approvals` and `ownPayslip`: `gw<{ requests: unknown[] }>("api/v1/leave/approvals")` etc. and `gw<{ payslips: { net_pay: string; period_start: string }[] }>("api/v1/me/payslips")`.

Self-service card (`selfService`) for manager/employee: 4 tiles linking to `/me/payslips`, `/me/reviews`, `/leave` (own balance), `/ot` (own requests), each showing "—" until its fetch resolves and a `dash.empty.*` line when empty.

- [ ] **Step 5: Add the CSS**

`.kpi-grid` (4 → 2 → 1 columns at 1240/760px), `.kpi` (label/value/hint), `.dash-grid` (`1fr 340px`, collapsing to one column at 1240px), `.stack`, `.mine-grid` (2×2 tiles, truncating `text-overflow: ellipsis`).

- [ ] **Step 6: Verify**

```bash
cd web && npx tsc --noEmit && npm run build
rm -rf .next/standalone/.next/static .next/standalone/public
cp -r .next/static .next/standalone/.next/ && cp -r public .next/standalone/
for pid in $(ss -tlnp 2>/dev/null | grep ':3000' | grep -oP 'pid=\K[0-9]+' | sort -u); do kill "$pid"; done
cd .next/standalone && setsid env TALENTOS_API_URL=http://127.0.0.1:18479 TALENTOS_COOKIE_SECURE=false PORT=3000 HOSTNAME=0.0.0.0 node server.js > /tmp/opencode/web-server.log 2>&1 < /dev/null &
sleep 4
docker exec loans-postgres-1 psql -U talentos -d talentos -c "DELETE FROM tenants;"
E2E_BASE_URL=http://127.0.0.1:3000 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test
```
Expected: 56 passed.

- [ ] **Step 7: Commit**

```bash
git add web/app/\(app\)/page.tsx web/app/globals.css web/e2e/app.spec.ts
git commit -m "feat(web): role-shaped dashboard shell with KPI row and self-service tiles"
```

---

### Task 5: Approvals card grid

**Files:**
- Modify: `web/app/(app)/page.tsx`
- Modify: `web/app/globals.css` (`.appr-grid`, `.appr`, `.appr-*`, `.chip`)

**Interfaces:**
- Consumes: the three approval endpoints; `thDate`; `relativeTh` is **not** used here (no submission timestamp — spec §15.6).
- Produces: `data-testid="approvals-grid"` and `data-testid="approval-card"` per item.

- [ ] **Step 1: Add a new standalone test (do not navigate mid-flow inside an existing test)**

Append a new test after the existing leave-request test (the suite is `serial`, so a pending leave exists by then):

```ts
test("dashboard shows the pending approvals", async ({ page }) => {
  await login(page);
  await expect(page.getByTestId("approvals-grid")).toBeVisible();
  await expect(page.getByTestId("approval-card").first()).toBeVisible();
  await expect(page.getByTestId("approval-card").first()).toContainText("E2E001");
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && E2E_BASE_URL=http://127.0.0.1:3000 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test -g "dashboard shows the pending approvals"`
Expected: FAIL — `approvals-grid` not found.

- [ ] **Step 3: Implement the section**

Fetch in parallel:

```ts
const [leave, ot, claims] = await Promise.all([
  gw<{ requests: { id: string; employee_name: string; employee_code: string; leave_type: string; label_th: string; start_date: string; end_date: string; days: string }[] }>("api/v1/leave/approvals"),
  gw<{ requests: { id: string; employee_name: string; employee_code: string; work_date: string; minutes: number }[] }>("api/v1/ot/approvals"),
  gw<{ claims: { id: string; employee_name: string; employee_code: string; category: string; amount: string; expense_date: string }[] }>("api/v1/claims/approvals"),
]);
```

Normalise to `{ id, kind, employee, code, when, meta, href }`, sort by `when` descending, take 6. Card links: `/leave`, `/ot`, `/claims`. Render empty state when all three are empty. On any 403 (payroll_manager path never renders this section, but a delegated manager may hit one), render the section's empty state instead of an error.

- [ ] **Step 4: Verify**

Run: `cd web && npx tsc --noEmit && npx playwright test -g "leave"` with the server running; then the full suite as in Task 4 Step 6.
Expected: 56 passed.

- [ ] **Step 5: Commit**

```bash
git add web/app/\(app\)/page.tsx web/app/globals.css web/e2e/app.spec.ts
git commit -m "feat(web): approval cards on the dashboard"
```

---

### Task 6: Follow-ups and OT trend chart

**Files:**
- Modify: `web/app/(app)/page.tsx`
- Modify: `web/app/globals.css` (`.follow`, `.chart`, `.bar`)

**Interfaces:**
- Consumes: `otTrendQuery()`; `gw<{ documents: unknown[] }>("api/v1/documents/expiring")`; draft runs from the payroll-runs GraphQL query.
- Produces: `data-testid="follow-ups"`, `data-testid="ot-chart"`.

- [ ] **Step 1: Add failing assertions**

In the HR test after login:

```ts
await expect(page.getByTestId("follow-ups")).toBeVisible();
await expect(page.getByTestId("ot-chart")).toBeVisible();
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd web && E2E_BASE_URL=http://127.0.0.1:3000 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test -g "creates an employee"`
Expected: FAIL — testids missing.

- [ ] **Step 3: Implement**

`follow-ups` lists three counters: เอกสารใกล้หมดอายุ (`documents/expiring` length), รอบเงินเดือนฉบับร่าง (draft runs), รอบประเมินที่เปิด (from `api/v1/performance/cycles` where `status === "active"`). Each row links to its module. `ot-chart` fetches `api/v1/reports/ot-trend?${otTrendQuery()}` → `{ rows: { month: string; ot_minutes: number }[] }` and renders one `<rect>` per month, height proportional to the max, with the month label. Empty state when all `ot_minutes` are 0.

- [ ] **Step 4: Verify and commit**

Run the full suite as in Task 4 Step 6; expect 56 passed.
```bash
git add web/app/\(app\)/page.tsx web/app/globals.css web/e2e/app.spec.ts
git commit -m "feat(web): follow-up counters and monthly OT chart"
```

---

### Task 7: Run summary, department donut, recent activity

**Files:**
- Modify: `web/app/(app)/page.tsx`
- Modify: `web/app/globals.css` (`.summary`, `.donut`, `.recent`)

**Interfaces:**
- Consumes: `headcount.by_department` (reuse the Task 4 response — do not fetch twice); `gw<{ entries: { action: string; created_at: string }[] }>("api/v1/audit?limit=4")`.
- Produces: `data-testid="run-summary"`, `data-testid="dept-donut"`, `data-testid="activity"`.

- [ ] **Step 1: Add failing assertions**

```ts
await expect(page.getByTestId("run-summary")).toBeVisible();
await expect(page.getByTestId("dept-donut")).toBeVisible();
await expect(page.getByTestId("activity")).toBeVisible();
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd web && E2E_BASE_URL=http://127.0.0.1:3000 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test -g "creates an employee"`
Expected: FAIL.

- [ ] **Step 3: Implement**

- `run-summary`: dark `--teal` panel with `totalGross/totalTax/totalSso/totalPf/totalNet` from the latest run (reuse the Task 4 state), CTA `Link href="/payroll"`.
- `dept-donut`: inline SVG using `stroke-dasharray` segments over `by_department`, sorted desc, top 4 + "อื่น ๆ"; the centre shows the total which must equal the headcount KPI value (same response object).
- `activity`: action + `relativeTh(created_at)`; map the four most common action keys to Thai labels (`employee.create`, `payroll.run.approve`, `leave.decide`, `attendance.import`) and fall back to the raw key.

- [ ] **Step 4: Verify and commit**

Run the full suite as in Task 4 Step 6; expect 56 passed.
```bash
git add web/app/\(app\)/page.tsx web/app/globals.css web/e2e/app.spec.ts
git commit -m "feat(web): run summary, department donut and recent activity"
```

---

### Task 8: Role coverage, responsive check, full verification

**Files:**
- Modify: `web/e2e/roles.spec.ts` (role-specific dashboard assertions)

**Interfaces:**
- Consumes: everything above.
- Produces: no new API.

- [ ] **Step 1: Add role assertions**

In `roles.spec.ts`, after the existing payroll-manager loop:

```ts
await page.goto("/");
await expect(page.getByTestId("dash-title")).toBeVisible();
await expect(page.getByTestId("run-summary")).toBeVisible();
await expect(page.getByTestId("approvals-grid")).toHaveCount(0);
```

After the manager loop:

```ts
await page.goto("/");
await expect(page.getByTestId("approvals-grid")).toBeVisible();
await expect(page.getByTestId("run-summary")).toHaveCount(0);
```

In the employee test:

```ts
await page.goto("/");
await expect(page.getByTestId("mine-grid")).toBeVisible();
await expect(page.getByTestId("approvals-grid")).toHaveCount(0);
```

- [ ] **Step 2: Run the role spec**

Run: `cd web && E2E_BASE_URL=http://127.0.0.1:3000 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test e2e/roles.spec.ts`
Expected: all role tests pass.

- [ ] **Step 3: Full verification**

```bash
cd web && npx tsc --noEmit && npx vitest run && npm run build
docker exec loans-postgres-1 psql -U talentos -d talentos -c "DELETE FROM tenants;"
E2E_BASE_URL=http://127.0.0.1:3000 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test
```
Expected: tsc exit 0, vitest all pass, build exit 0, **56 passed**.

- [ ] **Step 4: Responsive screenshots**

Capture `/` at 1440, 1280, and 390 wide. Check: sidebar hidden behind the drawer at 390; no horizontal scroll; the 4th KPI wraps rather than clipping; approval cards truncate long names.

- [ ] **Step 5: Commit**

```bash
git add web/e2e/roles.spec.ts
git commit -m "test(web): role-specific dashboard coverage"
```

---

## Self-Review Notes

- Spec §8 (nav model) → Task 1 + Task 2. §9 (role matrix, sections, data sources) → Tasks 3–7. §10 (visual language) → CSS in Tasks 2, 4–7. §11 (test impact) → Task 4 Step 1 and Task 8. §14 (role-shaped decision) → Task 4 Step 4 removes the redirect.
- Every section in the spec's §9 matrix has exactly one owning task.
- Names used across tasks: `visibleNav`/`visibleHrefs` (Task 1) are consumed unchanged in Task 2; `sectionsFor`/`primaryRole`/`compactThb`/`relativeTh`/`otTrendQuery` (Task 3) are consumed unchanged in Tasks 4–7.
- No task adds a dependency, a backend field, or a new route.

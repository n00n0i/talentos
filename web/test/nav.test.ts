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
const MANAGER = me(["manager"], ["performance.manage_direct", "attendance.read", "employee.read_own_team"]);
const EMPLOYEE = me(["employee"], []);

const ALL_HREFS = NAV_GROUPS.flatMap((g) => g.items.map((i) => i.href));
// The base set today's shell shows while /api/v1/auth/me is still loading.
const BASE_HREFS = [
  "/", "/employees", "/documents", "/onboarding", "/recruitment", "/training", "/performance",
  "/goals", "/team", "/me/reviews", "/attendance", "/calendar", "/clock", "/leave", "/ot", "/claims",
  "/payroll", "/payslip", "/me/payslips", "/privacy",
];

describe("visibleHrefs", () => {
  it("hr_admin sees admin destinations and the overview", () => {
    const hrefs = visibleHrefs(HR);
    for (const h of ["/", "/employees", "/payroll", "/users", "/settings", "/audit", "/loans", "/final-settlement"]) {
      expect(hrefs.has(h)).toBe(true);
    }
    expect([...hrefs].sort()).toEqual([...ALL_HREFS].sort());
  });

  it("payroll_manager sees payroll and people but no admin destinations", () => {
    const hrefs = visibleHrefs(PAYROLL);
    expect(hrefs.has("/payroll")).toBe(true);
    expect(hrefs.has("/reports")).toBe(true);
    expect(hrefs.has("/users")).toBe(false);
    expect(hrefs.has("/settings")).toBe(false);
    expect(hrefs.has("/audit")).toBe(false);
    expect([...hrefs].sort()).toEqual(
      [
        "/", "/attendance", "/calendar", "/claims", "/clock", "/documents", "/employees", "/goals",
        "/leave", "/me/payslips", "/me/reviews", "/onboarding", "/ot", "/payroll", "/performance",
        "/privacy", "/recruitment", "/reports", "/team", "/training",
      ].sort(),
    );
  });

  it("hides /payslip for non-admin HR users and shows it for hr_admin", () => {
    // /payslip is an admin-only route: Shell.tsx redirects it to /clock for
    // anyone who is not hr_admin, so the nav must not offer it.
    expect(visibleHrefs(PAYROLL).has("/payslip")).toBe(false);
    expect(visibleHrefs(HR).has("/payslip")).toBe(true);
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
    expect(hrefs.has("/attendance")).toBe(true);
    expect(hrefs.has("/calendar")).toBe(true);
    expect([...hrefs].sort()).toEqual(
      [
        "/", "/attendance", "/calendar", "/claims", "/clock", "/goals", "/leave", "/me/payslips",
        "/me/reviews", "/ot", "/performance", "/privacy", "/team",
      ].sort(),
    );
  });

  it("shows today's base nav while the user is still loading", () => {
    // No admin destination may flash before /api/v1/auth/me resolves.
    expect([...visibleHrefs(null)].sort()).toEqual([...BASE_HREFS].sort());
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
    expect(ids).toEqual(["overview", "time", "mine"]);
  });

  it("keeps every group in order for hr_admin", () => {
    expect(visibleNav(HR).map((g) => g.id)).toEqual([
      "overview", "people", "time", "pay", "mine", "admin",
    ]);
  });
});

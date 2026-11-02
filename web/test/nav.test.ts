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

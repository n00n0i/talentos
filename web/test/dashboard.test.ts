import { describe, expect, it } from "vitest";
import { compactThb, otTrendQuery, primaryRole, relativeTh, sectionsFor } from "@/lib/dashboard";
import { translate } from "@/lib/i18n-dicts";
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

describe("role labels", () => {
  const roles = ["hr_admin", "payroll_manager", "manager", "employee"];
  it("has a label in both dictionaries", () => {
    for (const r of roles) {
      expect(translate("th", `role.${r}`)).not.toBe(`role.${r}`);
      expect(translate("en", `role.${r}`)).not.toBe(`role.${r}`);
    }
  });
  it("translates the role names", () => {
    expect(translate("th", "role.hr_admin")).toBe("ผู้ดูแล HR");
    expect(translate("en", "role.hr_admin")).toBe("HR admin");
    expect(translate("en", "role.employee")).toBe("Employee");
  });
});

import { describe, expect, it } from "vitest";
import { en, missingKeys, plural, th, translate } from "@/lib/i18n-dicts";
import { formatCurrency, formatDate, formatNumber } from "@/lib/i18n-format";

const THAI = /[\u0E00-\u0E7F]/; // ฿ (U+0E3F) is allowed as a currency symbol

describe("translate", () => {
  it("returns Thai for a known key when lang=th", () => {
    expect(translate("th", "nav.employees")).toBe("พนักงาน");
  });

  it("returns English for a known key when lang=en", () => {
    expect(translate("en", "nav.employees")).toBe("Employees");
  });

  it("returns the key itself when the key is missing from both dicts", () => {
    expect(translate("th", "non.existent.key")).toBe("non.existent.key");
    expect(translate("en", "non.existent.key")).toBe("non.existent.key");
  });

  it("records missing keys for diagnostics", () => {
    translate("en", "missing.detection.probe");
    expect(missingKeys()).toContain("missing.detection.probe");
  });

  it("interpolates {n} and {code} variables", () => {
    expect(translate("en", "employees.importedNotice", { n: 3 })).toBe("Imported 3 employees");
    expect(translate("en", "employees.confirmTerminate", { code: "E001" })).toBe(
      "Close E001? Payroll history is kept.",
    );
  });

  it("leaves interpolation braces when variable is missing", () => {
    expect(translate("en", "employees.importedNotice")).toBe("Imported {n} employees");
  });

  it("has the same keys in TH and EN (missing EN fails the build)", () => {
    expect(Object.keys(en).sort()).toEqual(Object.keys(th).sort());
  });

  it("has no Thai copy left in the EN catalog", () => {
    const stray = Object.entries(en).filter(([, value]) => THAI.test(value.replace(/฿/g, "")));
    expect(stray, `untranslated EN values: ${JSON.stringify(stray)}`).toEqual([]);
  });

  it("covers the critical namespaces with typed keys", () => {
    for (const ns of ["auth.login", "payroll", "leave", "attendance", "employees", "claims", "ot", "settings", "team", "users", "common"]) {
      expect(Object.keys(th).some((key) => key.startsWith(`${ns}.`)), ns).toBe(true);
    }
  });

  it("resolves shell nav keys to English when lang=en", () => {
    expect(translate("en", "nav.payroll")).toBe("Payroll");
    expect(translate("en", "nav.settings")).toBe("Settings");
    expect(translate("en", "nav.clock")).toBe("Clock");
  });
});

describe("plural", () => {
  it("uses the English one/other forms", () => {
    expect(plural("en", "common.employeeCount", 1)).toBe("1 employee");
    expect(plural("en", "common.employeeCount", 7)).toBe("7 employees");
  });

  it("uses the Thai single form", () => {
    expect(plural("th", "common.employeeCount", 1)).toBe("พนักงาน 1 คน");
    expect(plural("th", "common.employeeCount", 7)).toBe("พนักงาน 7 คน");
  });
});

describe("locale formatting", () => {
  it("formats date-only values without a timezone shift", () => {
    expect(formatDate("2026-01-01", "en")).toBe("1 Jan 2026");
    expect(formatDate("2026-01-01", "th")).toContain("2569");
  });

  it("formats numbers and currency per locale", () => {
    expect(formatNumber(1234.5, "en")).toBe("1,234.50");
    expect(formatCurrency(1234.5, "th")).toContain("1,234.50");
  });
});

import { describe, expect, it } from "vitest";
import { en, th, translate } from "@/lib/i18n-dicts";

describe("translate", () => {
  it("returns Thai for a known key when lang=th", () => {
    expect(translate("th", "nav.employees")).toBe("พนักงาน");
  });

  it("returns English for a known key when lang=en", () => {
    expect(translate("en", "nav.employees")).toBe("Employees");
  });

  it("falls back to Thai when lang=en and the key is missing from the EN dict", () => {
    // "common.testOnlyTh" is intentionally in th but not in en
    expect(translate("en", "common.testOnlyTh")).toBe(th["common.testOnlyTh"]);
  });

  it("returns the key itself when the key is missing from both dicts", () => {
    expect(translate("th", "non.existent.key")).toBe("non.existent.key");
    expect(translate("en", "non.existent.key")).toBe("non.existent.key");
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

  it("exposes the th dict for every en key (th always present)", () => {
    for (const key of Object.keys(en)) {
      expect(th[key], `key ${key} missing from th dict`).toBeDefined();
    }
  });

  it("resolves shell nav keys to English when lang=en", () => {
    expect(translate("en", "nav.payroll")).toBe("Payroll");
    expect(translate("en", "nav.settings")).toBe("Settings");
    expect(translate("en", "nav.clock")).toBe("Clock");
  });

  it("resolves common action keys", () => {
    expect(translate("en", "common.save")).toBe("Save");
    expect(translate("en", "common.cancel")).toBe("Cancel");
  });

  it("TH dict has nav.employees", () => {
    expect(th["nav.employees"]).toBeTruthy();
  });

  it("EN dict has nav.employees", () => {
    expect(en["nav.employees"]).toBe("Employees");
  });
});

import type { Me } from "@/lib/api";

export type Role = "hr_admin" | "payroll_manager" | "manager" | "employee";

export type SectionId =
  | "headcount"
  | "net"
  | "tax"
  | "approvals"
  | "ownPayslip"
  | "selfService"
  | "approvalsGrid"
  | "followUps"
  | "otTrend"
  | "runSummary"
  | "departments"
  | "activity";

/** Widest role wins: an HR admin who is also a manager sees the HR view. */
export function primaryRole(me: Me): Role {
  const roles = me.roles ?? [];
  if (roles.includes("hr_admin")) return "hr_admin";
  if (roles.includes("payroll_manager")) return "payroll_manager";
  if (roles.includes("manager")) return "manager";
  return "employee";
}

/** Dashboard sections per role (design §9). Order is the render order. */
export function sectionsFor(role: Role): SectionId[] {
  switch (role) {
    case "hr_admin":
      return [
        "headcount",
        "net",
        "tax",
        "approvals",
        "approvalsGrid",
        "followUps",
        "otTrend",
        "runSummary",
        "departments",
        "activity",
      ];
    case "payroll_manager":
      return ["headcount", "net", "tax", "followUps", "otTrend", "runSummary", "departments"];
    case "manager":
      return ["approvals", "ownPayslip", "approvalsGrid", "selfService"];
    case "employee":
      return ["ownPayslip", "selfService"];
  }
}

const MILLION = 1_000_000;

/** ฿1,234,567 → "฿1.23 ล้าน"; anything smaller keeps exact baht. */
export function compactThb(n: number | string): string {
  const v = Number(n);
  if (!Number.isFinite(v)) return "฿0";
  if (Math.abs(v) >= MILLION) return `฿${(v / MILLION).toFixed(2)} ล้าน`;
  return `฿${new Intl.NumberFormat("th-TH", { maximumFractionDigits: 0 }).format(v)}`;
}

// "always" + "short" gives "2 ชม. ที่แล้ว" / "2 วันที่แล้ว"; "auto" would
// render -2 days as "เมื่อวานซืน", which hides the day count.
const RTF = new Intl.RelativeTimeFormat("th", { numeric: "always", style: "short" });

/** Thai relative time: minutes → hours → days → plain date after 30 days. */
export function relativeTh(iso: string, now: Date = new Date()): string {
  const then = new Date(iso);
  if (Number.isNaN(then.getTime())) return iso;
  const diff = now.getTime() - then.getTime();
  const abs = Math.abs(diff);
  const sign = diff >= 0 ? -1 : 1;
  if (abs < 60 * 60_000) return RTF.format(sign * Math.round(abs / 60_000), "minute");
  if (abs < 24 * 60 * 60_000) return RTF.format(sign * Math.round(abs / 3_600_000), "hour");
  if (abs < 30 * 24 * 60 * 60_000) return RTF.format(sign * Math.round(abs / 86_400_000), "day");
  return then.toLocaleDateString("th-TH");
}

const pad = (n: number) => String(n).padStart(2, "0");

/**
 * "from=2025-12-01&to=2026-11-15" — local date parts, so today's rows are not
 * dropped between 00:00 and 07:00 Bangkok time (UTC is still yesterday).
 */
export function otTrendQuery(months = 12, now: Date = new Date()): string {
  const y = now.getFullYear();
  const m = now.getMonth();
  const from = new Date(y, m - (months - 1), 1);
  const to = new Date(y, m, now.getDate());
  return `from=${from.getFullYear()}-${pad(from.getMonth() + 1)}-01&to=${to.getFullYear()}-${pad(to.getMonth() + 1)}-${pad(to.getDate())}`;
}

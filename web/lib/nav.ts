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

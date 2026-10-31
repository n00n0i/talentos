"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import Icon, { IconName } from "@/components/Icon";
import { gw, Me } from "@/lib/api";
import { useI18N } from "@/lib/i18n";

type NavKey = "nav.overview" | "nav.employees" | "nav.documents" | "nav.onboarding" |
  "nav.recruitment" | "nav.training" | "nav.performance" | "nav.goals" | "nav.myReviews" |
  "nav.attendance" | "nav.calendar" | "nav.clock" | "nav.leave" | "nav.ot" |
  "nav.claims" | "nav.payroll" | "nav.payslip" | "nav.myPayslips" | "nav.privacy" |
  "nav.loans" | "nav.finalSettlement" | "nav.reports" | "nav.users" | "nav.settings" |
  "nav.forms" | "nav.audit";

const NAV: { href: string; label: NavKey; icon: IconName }[] = [
  { href: "/", label: "nav.overview", icon: "overview" },
  { href: "/employees", label: "nav.employees", icon: "users" },
  { href: "/documents", label: "nav.documents", icon: "file" },
  { href: "/onboarding", label: "nav.onboarding", icon: "users" },
  { href: "/recruitment", label: "nav.recruitment", icon: "users" },
  { href: "/training", label: "nav.training", icon: "file" },
  { href: "/performance", label: "nav.performance", icon: "file" },
  { href: "/goals", label: "nav.goals", icon: "file" },
  { href: "/me/reviews", label: "nav.myReviews", icon: "file" },
  { href: "/attendance", label: "nav.attendance", icon: "clock" },
  { href: "/calendar", label: "nav.calendar", icon: "clock" },
  { href: "/clock", label: "nav.clock", icon: "clock" },
  { href: "/leave", label: "nav.leave", icon: "clock" },
  { href: "/ot", label: "nav.ot", icon: "clock" },
  { href: "/claims", label: "nav.claims", icon: "wallet" },
  { href: "/payroll", label: "nav.payroll", icon: "wallet" },
  { href: "/payslip", label: "nav.payslip", icon: "file" },
  { href: "/me/payslips", label: "nav.myPayslips", icon: "file" },
  { href: "/privacy", label: "nav.privacy", icon: "file" },
];

const ADMIN_NAV: { href: string; label: NavKey; icon: IconName }[] = [
  { href: "/users", label: "nav.users", icon: "user" },
  { href: "/settings", label: "nav.settings", icon: "settings" },
  { href: "/forms", label: "nav.forms", icon: "file" },
  { href: "/audit", label: "nav.audit", icon: "file" },
];

const ROLE_LABEL: Record<string, string> = {
  hr_admin: "ผู้ดูแล HR",
  payroll_manager: "ผู้จัดการเงินเดือน",
  manager: "หัวหน้างาน",
  employee: "พนักงาน",
};

function initials(name: string) {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "T";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return `${parts[0][0]}${parts[1][0]}`.toUpperCase();
}

export default function Shell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();
  const [me, setMe] = useState<Me | null>(null);
  const [today, setToday] = useState("");
  const { lang, setLang, t } = useI18N();

  useEffect(() => {
    gw<Me>("api/v1/auth/me")
      .then(setMe)
      .catch((e) => {
        if (e.status === 401) router.replace("/login");
      });
  }, [router]);

  useEffect(() => {
    setToday(
      new Intl.DateTimeFormat("th-TH", {
        day: "numeric",
        month: "short",
        year: "numeric",
      }).format(new Date()),
    );
  }, []);

  // Register the offline service worker (network-first, cache fallback for
  // navigations). Best-effort: failures must never affect the app.
  useEffect(() => {
    if ("serviceWorker" in navigator) {
      navigator.serviceWorker.register("/sw.js").catch(() => {});
    }
  }, []);

  async function logout() {
    await fetch("/api/session/logout", { method: "POST" });
    router.replace("/login");
  }

  const isAdmin = me?.roles.includes("hr_admin") ?? false;
  const canHr = me?.permissions.includes("employee.read_all") ?? true;
  const canDirect = me?.permissions.includes("performance.manage_direct") ?? false;
  const canReports = me?.roles.some((r) => ["hr_admin", "payroll_manager"].includes(r)) ?? false;

  // Direct URLs must not render admin screens for roles that only see the
  // error state. The API still enforces permissions; this keeps the UI honest.
  useEffect(() => {
    if (!me) return;
    const adminOnly = ["/users", "/settings", "/forms", "/audit", "/loans", "/final-settlement", "/payslip"];
    const payrollOnly = ["/payroll", "/reports"];
    const hrOnly = [
      "/employees",
      "/documents",
      "/onboarding",
      "/recruitment",
      "/training",
      "/performance",
      "/goals",
    ];
    const attendanceOnly = ["/attendance", "/calendar"];
    // /performance and /goals are reachable by managers with manage_direct.
    const directOnly = ["/performance", "/goals"];
    const denied =
      (adminOnly.some((p) => pathname === p || pathname.startsWith(`${p}/`)) && !isAdmin) ||
      (payrollOnly.some((p) => pathname === p || pathname.startsWith(`${p}/`)) && !canReports) ||
      (hrOnly.some((p) => pathname === p || pathname.startsWith(`${p}/`)) &&
        !canHr &&
        !directOnly.some((p) => pathname === p || pathname.startsWith(`${p}/`))) ||
      (attendanceOnly.some((p) => pathname === p || pathname.startsWith(`${p}/`)) &&
        !me.permissions.includes("attendance.read"));
    if (denied) router.replace("/clock");
  }, [me, pathname, router, isAdmin, canHr, canReports]);
  const displayName = me?.full_name_th || me?.email || "TalentOS";
  const roleLabel = me?.roles.map((r) => ROLE_LABEL[r] ?? r).join(" · ") || "";
  const baseNav = canReports
    ? [
        ...NAV,
        ...(isAdmin
          ? [
              { href: "/loans", label: "nav.loans" as NavKey, icon: "wallet" as IconName },
              { href: "/final-settlement", label: "nav.finalSettlement" as NavKey, icon: "users" as IconName },
            ]
          : []),
        { href: "/reports", label: "nav.reports" as NavKey, icon: "file" as IconName },
      ]
    : NAV;
  const employeeNav = [
    "/clock",
    "/leave",
    "/ot",
    "/claims",
    "/me/payslips",
    "/privacy",
    "/me/reviews",
  ];
  const directPages = ["/performance", "/goals"];
  const items = !me
    ? baseNav
    : canHr
      ? [...baseNav, ...(isAdmin ? ADMIN_NAV : [])]
      : baseNav.filter(
          (i) =>
            employeeNav.includes(i.href) ||
            (directPages.includes(i.href) && canDirect) ||
            ((i.href === "/attendance" || i.href === "/calendar") &&
              me.permissions.includes("attendance.read")),
        );

  return (
    <div className="shell">
      <header className="appbar">
        <Link href="/" className="brand" aria-label="TalentOS">
          <span className="brand-mark">
            <Icon name="clover" size={22} />
          </span>
        </Link>

        <nav className="pills" aria-label={t("shell.menu")}>
          {items.map((item) => (
            <Link
              key={item.href}
              href={item.href}
              className={pathname === item.href ? "active" : undefined}
            >
              {t(item.label)}
            </Link>
          ))}
        </nav>

        <div className="appbar-right">
          <button
            data-testid="lang-toggle"
            className="lang-toggle"
            onClick={() => setLang(lang === "th" ? "en" : "th")}
            aria-label={t("shell.toggleLang")}
            title={t("shell.toggleLang")}
          >
            {lang === "th" ? "EN" : "TH"}
          </button>
          <span className="date-chip">{today}</span>
          <div className="avatar" title={`${displayName} · ${roleLabel}`}>
            {initials(displayName)}
          </div>
          <button className="icon-btn" onClick={logout} title={t("shell.logout")} aria-label={t("shell.logout")}>
            <Icon name="logout" size={15} />
          </button>
        </div>
      </header>

      <main className="content">{children}</main>
    </div>
  );
}

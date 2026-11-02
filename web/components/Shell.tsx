"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import Icon from "@/components/Icon";
import { gw, Me } from "@/lib/api";
import { useI18N } from "@/lib/i18n";
import { visibleNav } from "@/lib/nav";

/** Case- and diacritic-insensitive fold: "Käse" → "kase", Thai marks stripped too. */
function fold(value: string) {
  return value.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

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
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
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

  useEffect(() => setOpen(false), [pathname]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

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
    // /performance and /goals are reachable only by managers with manage_direct.
    const directOnly = ["/performance", "/goals"];
    const denied =
      (adminOnly.some((p) => pathname === p || pathname.startsWith(`${p}/`)) && !isAdmin) ||
      (payrollOnly.some((p) => pathname === p || pathname.startsWith(`${p}/`)) && !canReports) ||
      (hrOnly.some((p) => pathname === p || pathname.startsWith(`${p}/`)) &&
        !canHr &&
        !(
          canDirect &&
          directOnly.some((p) => pathname === p || pathname.startsWith(`${p}/`))
        )) ||
      (attendanceOnly.some((p) => pathname === p || pathname.startsWith(`${p}/`)) &&
        !me.permissions.includes("attendance.read"));
    if (denied) router.replace("/clock");
  }, [me, pathname, router, isAdmin, canHr, canReports]);

  const displayName = me?.full_name_th || me?.email || "TalentOS";
  const roleLabel =
    me?.roles
      .map((r) => {
        const key = `role.${r}`;
        const label = t(key);
        return label === key ? r : label; // unknown role → show the raw role
      })
      .join(" · ") || "";
  const needle = fold(q.trim());
  const groups = visibleNav(me)
    .map((g) => ({
      ...g,
      items: g.items.filter((i) => fold(t(i.labelKey)).includes(needle)),
    }))
    .filter((g) => g.items.length > 0);

  return (
    <div className="app-shell">
      <aside className={open ? "side open" : "side"} data-testid="side-nav">
        <Link href="/" className="side-brand">
          <span className="side-brand-mark">
            <Icon name="clover" size={18} />
          </span>
          <span className="side-brand-name">TalentOS</span>
        </Link>
        <nav className="side-nav" id="side-nav-menu" aria-label={t("shell.menu")}>
          {groups.map((g) => (
            <div className="side-group" key={g.id}>
              {g.labelKey && <span className="side-group-label">{t(g.labelKey)}</span>}
              {g.items.map((i) => (
                <Link key={i.href} href={i.href} className={pathname === i.href ? "active" : undefined}>
                  <span className="ico">
                    <Icon name={i.icon} size={15} />
                  </span>
                  {t(i.labelKey)}
                </Link>
              ))}
            </div>
          ))}
          {groups.length === 0 && <p className="side-empty">{t("shell.noResults")}</p>}
        </nav>
        <div className="side-user">
          <div className="avatar">{initials(displayName)}</div>
          <div className="side-user-text">
            <span className="side-user-name">{displayName}</span>
            {roleLabel && <span className="side-user-role">{roleLabel}</span>}
          </div>
        </div>
      </aside>
      {open && (
        <button
          className="side-backdrop"
          data-testid="side-backdrop"
          aria-label={t("shell.closeMenu")}
          onClick={() => setOpen(false)}
        />
      )}
      <div className="main">
        <div className="topbar">
          <button
            className="menu-btn"
            data-testid="menu-toggle"
            aria-label={t("shell.menu")}
            aria-expanded={open}
            aria-controls="side-nav-menu"
            onClick={() => setOpen((v) => !v)}
          >
            ☰
          </button>
          <label className="search">
            <input
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder={t("shell.searchPlaceholder")}
              aria-label={t("shell.searchPlaceholder")}
            />
          </label>
          <span className="date-chip">{today}</span>
          <div className="topbar-right">
            <button
              data-testid="lang-toggle"
              className="lang-toggle"
              onClick={() => setLang(lang === "th" ? "en" : "th")}
              aria-label={t("shell.toggleLang")}
              title={t("shell.toggleLang")}
            >
              {lang === "th" ? "EN" : "TH"}
            </button>
            <div className="avatar" title={`${displayName} · ${roleLabel}`}>
              {initials(displayName)}
            </div>
            <button className="icon-btn" onClick={logout} title={t("shell.logout")} aria-label={t("shell.logout")}>
              <Icon name="logout" size={15} />
            </button>
          </div>
        </div>
        <main className="content">{children}</main>
      </div>
    </div>
  );
}

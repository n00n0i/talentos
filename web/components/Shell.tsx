"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import Icon, { IconName } from "@/components/Icon";
import { gw, Me } from "@/lib/api";

const NAV: { href: string; label: string; icon: IconName }[] = [
  { href: "/", label: "ภาพรวม", icon: "overview" },
  { href: "/employees", label: "พนักงาน", icon: "users" },
  { href: "/attendance", label: "เวลาและ OT", icon: "clock" },
  { href: "/payroll", label: "รอบเงินเดือน", icon: "wallet" },
  { href: "/payslip", label: "แบบฟอร์มสลิป", icon: "file" },
];

const ADMIN_NAV: { href: string; label: string; icon: IconName }[] = [
  { href: "/users", label: "ผู้ใช้", icon: "user" },
  { href: "/settings", label: "ตั้งค่า", icon: "settings" },
];

const TITLES: Record<string, string> = {
  "/": "ภาพรวม",
  "/employees": "พนักงาน",
  "/attendance": "เวลาและ OT",
  "/payroll": "รอบเงินเดือน",
  "/payslip": "แบบฟอร์มสลิป",
  "/users": "ผู้ใช้",
  "/settings": "ตั้งค่า",
};

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
        month: "long",
        year: "numeric",
      }).format(new Date()),
    );
  }, []);

  async function logout() {
    await fetch("/api/session/logout", { method: "POST" });
    router.replace("/login");
  }

  const isAdmin = me?.roles.includes("hr_admin") ?? false;
  const displayName = me?.full_name_th || me?.email || "TalentOS";
  const roleLabel = me?.roles.map((r) => ROLE_LABEL[r] ?? r).join(" · ") || "";
  const title = TITLES[pathname] ?? "TalentOS";
  const items = isAdmin ? [...NAV, ...ADMIN_NAV] : NAV;

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">t</span> talentos
        </div>

        <div className="workspace">
          <div className="workspace-avatar">{initials(me?.tenant_name ?? "T")}</div>
          <div className="workspace-copy">
            <div className="workspace-name">{me?.tenant_name ?? "พื้นที่ทำงาน"}</div>
            <div className="workspace-role">{roleLabel || "TalentOS"}</div>
          </div>
        </div>

        <nav className="nav">
          <div className="nav-label">งานบุคคล</div>
          {items.map((item) => (
            <Link
              key={item.href}
              href={item.href}
              className={pathname === item.href ? "active" : undefined}
            >
              <Icon name={item.icon} size={17} />
              <span>{item.label}</span>
            </Link>
          ))}
        </nav>

        <div className="side-card">
          <div className="side-card-icon">
            <Icon name="sparkle" size={15} />
          </div>
          <div className="side-card-title">สูตรเงินเดือน</div>
          <p>ปรับตัวหารค่าแรงและอัตรา OT ได้เองโดยไม่ต้อง deploy</p>
          <Link className="side-card-link" href={isAdmin ? "/settings" : "/payslip"}>
            {isAdmin ? "ไปที่ตั้งค่า" : "ดูแบบฟอร์มสลิป"}
            <Icon name="chevron" size={12} />
          </Link>
        </div>

        <div className="sidebar-foot">
          <div className="user-row">
            <div className="avatar">{initials(displayName)}</div>
            <div className="user-meta">
              <div className="user-name">{displayName}</div>
              <div className="user-role">{roleLabel}</div>
            </div>
            <button className="icon-btn" onClick={logout} title="ออกจากระบบ" aria-label="ออกจากระบบ">
              <Icon name="logout" size={15} />
            </button>
          </div>
        </div>
      </aside>

      <div className="main">
        <header className="topbar">
          <div className="crumbs">
            <span>TalentOS</span>
            <span className="crumb-sep">/</span>
            <span className="crumb-current">{title}</span>
          </div>
          <div className="topbar-right">
            <span className="date-chip">{today}</span>
            <div className="avatar avatar-sm" title={displayName}>
              {initials(displayName)}
            </div>
          </div>
        </header>
        <main className="content">{children}</main>
      </div>
    </div>
  );
}

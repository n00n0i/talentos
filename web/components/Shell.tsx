"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { gw, Me } from "@/lib/api";

const NAV = [
  { href: "/", label: "ภาพรวม" },
  { href: "/employees", label: "พนักงาน" },
  { href: "/attendance", label: "เวลาและ OT" },
  { href: "/payroll", label: "รอบเงินเดือน" },
  { href: "/payslip", label: "แบบฟอร์มสลิป" },
];

const ADMIN_NAV = [{ href: "/users", label: "ผู้ใช้" }];

const TITLES: Record<string, string> = {
  "/": "ภาพรวม",
  "/employees": "พนักงาน",
  "/attendance": "เวลาและ OT",
  "/payroll": "รอบเงินเดือน",
  "/payslip": "แบบฟอร์มสลิป",
  "/users": "ผู้ใช้",
};

export default function Shell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();
  const [me, setMe] = useState<Me | null>(null);

  useEffect(() => {
    gw<Me>("api/v1/auth/me")
      .then(setMe)
      .catch((e) => {
        if (e.status === 401) router.replace("/login");
      });
  }, [router]);

  async function logout() {
    await fetch("/api/session/logout", { method: "POST" });
    router.replace("/login");
  }

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">T</span> TalentOS
        </div>
        <nav className="nav">
          <div className="nav-label">งานบุคคล</div>
          {[...NAV, ...(me?.roles.includes("hr_admin") ? ADMIN_NAV : [])].map((item) => (
            <Link
              key={item.href}
              href={item.href}
              className={pathname === item.href ? "active" : undefined}
            >
              {item.label}
            </Link>
          ))}
        </nav>
        <div className="sidebar-foot">
          <div>
            <div className="user-name">{me?.full_name_th || me?.email || "…"}</div>
            <div className="user-role">{me?.roles?.join(", ") || ""}</div>
          </div>
          <button className="btn btn-sm" onClick={logout}>
            ออกจากระบบ
          </button>
        </div>
      </aside>
      <div className="main">
        <header className="topbar">
          <span className="topbar-title">{TITLES[pathname] ?? "TalentOS"}</span>
          <span className="hint">{me?.email}</span>
        </header>
        <main className="content">{children}</main>
      </div>
    </div>
  );
}

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
  { href: "/me/payslips", label: "สลิปของฉัน", icon: "file" },
];

const ADMIN_NAV: { href: string; label: string; icon: IconName }[] = [
  { href: "/users", label: "ผู้ใช้", icon: "user" },
  { href: "/settings", label: "ตั้งค่า", icon: "settings" },
  { href: "/audit", label: "ประวัติ", icon: "file" },
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

  async function logout() {
    await fetch("/api/session/logout", { method: "POST" });
    router.replace("/login");
  }

  const isAdmin = me?.roles.includes("hr_admin") ?? false;
  const canHr = me?.permissions.includes("employee.read_all") ?? true;
  const displayName = me?.full_name_th || me?.email || "TalentOS";
  const roleLabel = me?.roles.map((r) => ROLE_LABEL[r] ?? r).join(" · ") || "";
  const items = !me
    ? NAV
    : canHr
      ? [...NAV, ...(isAdmin ? ADMIN_NAV : [])]
      : [{ href: "/me/payslips", label: "สลิปของฉัน", icon: "file" as IconName }];

  return (
    <div className="shell">
      <header className="appbar">
        <Link href="/" className="brand" aria-label="TalentOS">
          <span className="brand-mark">
            <Icon name="clover" size={22} />
          </span>
        </Link>

        <nav className="pills" aria-label="เมนูหลัก">
          {items.map((item) => (
            <Link
              key={item.href}
              href={item.href}
              className={pathname === item.href ? "active" : undefined}
            >
              {item.label}
            </Link>
          ))}
        </nav>

        <div className="appbar-right">
          <span className="date-chip">{today}</span>
          <div className="avatar" title={`${displayName} · ${roleLabel}`}>
            {initials(displayName)}
          </div>
          <button className="icon-btn" onClick={logout} title="ออกจากระบบ" aria-label="ออกจากระบบ">
            <Icon name="logout" size={15} />
          </button>
        </div>
      </header>

      <main className="content">{children}</main>
    </div>
  );
}

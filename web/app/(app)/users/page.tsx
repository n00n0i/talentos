"use client";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError } from "@/lib/api";

type User = {
  id: string;
  email: string;
  full_name_th: string | null;
  is_active: boolean;
  roles: string[];
  last_login_at: string | null;
};

type InviteResult = { invite_path: string; email: string; role: string; expires_at: string };

const ROLES = [
  { value: "hr_admin", label: "ผู้ดูแล HR" },
  { value: "payroll_manager", label: "ผู้จัดการเงินเดือน" },
  { value: "manager", label: "หัวหน้างาน" },
  { value: "employee", label: "พนักงาน" },
];

export default function UsersPage() {
  const [users, setUsers] = useState<User[] | null>(null);
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [role, setRole] = useState("employee");
  const [invite, setInvite] = useState<InviteResult | null>(null);
  const [reset, setReset] = useState<{ email: string; reset_path: string; expires_at: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    const r = await gw<{ users: User[] }>("api/v1/users");
    setUsers(r.users);
  }, []);

  useEffect(() => {
    load().catch((e) => handleApiError(e, setError));
  }, [load]);

  async function submitInvite(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    setInvite(null);
    try {
      const r = await gw<InviteResult>("api/v1/users/invite", {
        method: "POST",
        body: JSON.stringify({ email, full_name_th: name || null, role }),
      });
      setInvite(r);
      setEmail("");
      setName("");
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function assign(user: User, newRole: string) {
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/users/${user.id}/roles`, {
        method: "POST",
        body: JSON.stringify({ role: newRole }),
      });
      setNotice(`เพิ่มบทบาท ${newRole} ให้ ${user.email} แล้ว`);
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function issueReset(user: User) {
    setBusy(true);
    setError("");
    setNotice("");
    setReset(null);
    try {
      const r = await gw<{ email: string; reset_path: string; expires_at: string }>(
        `api/v1/users/${user.id}/reset-password`,
        { method: "POST" },
      );
      setReset(r);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function deactivate(user: User) {
    if (!confirm(`ปิดการใช้งานบัญชี ${user.email}?`)) return;
    setBusy(true);
    try {
      await gw(`api/v1/users/${user.id}`, { method: "DELETE" });
      setNotice("ปิดการใช้งานบัญชีแล้ว");
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  const inviteUrl = invite
    ? `${typeof window === "undefined" ? "" : window.location.origin}${invite.invite_path}`
    : "";
  const resetUrl = reset
    ? `${typeof window === "undefined" ? "" : window.location.origin}${reset.reset_path}`
    : "";

  return (
    <>
      <div className="page-head">
        <div>
          <h1>ผู้ใช้</h1>
          <p className="page-sub">เชิญผู้ใช้และกำหนดบทบาท</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      <form className="card card-pad" onSubmit={submitInvite} style={{ display: "grid", gap: 14 }}>
        <h2>เชิญผู้ใช้</h2>
        <div className="form-row">
          <div className="field">
            <label htmlFor="invite-email">อีเมล</label>
            <input
              id="invite-email"
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="invite-name">ชื่อ (ไทย)</label>
            <input id="invite-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="invite-role">บทบาท</label>
            <select id="invite-role" value={role} onChange={(e) => setRole(e.target.value)}>
              {ROLES.map((r) => (
                <option key={r.value} value={r.value}>
                  {r.label}
                </option>
              ))}
            </select>
          </div>
        </div>
        <div>
          <button className="btn btn-primary" type="submit" disabled={busy}>
            {busy ? "กำลังสร้าง…" : "สร้างลิงก์เชิญ"}
          </button>
        </div>
        {invite && (
          <div style={{ background: "#eef0ff", borderRadius: 6, padding: 12, display: "grid", gap: 6 }}>
            <strong>ลิงก์เชิญ (ใช้ได้ครั้งเดียว, หมดอายุ 7 วัน)</strong>
            <a href={inviteUrl} style={{ wordBreak: "break-all", fontSize: 12, color: "var(--teal)" }}>
              {inviteUrl}
            </a>
            <div>
              <button
                type="button"
                className="btn btn-sm"
                onClick={() => navigator.clipboard?.writeText(inviteUrl)}
              >
                คัดลอกลิงก์
              </button>
            </div>
          </div>
        )}
      </form>

      {reset && (
        <div className="card card-pad" style={{ display: "grid", gap: 6 }}>
          <strong>ลิงก์รีเซ็ตรหัสผ่านสำหรับ {reset.email}</strong>
          <span className="hint">ใช้ได้ครั้งเดียว · หมดอายุใน 1 ชั่วโมง</span>
          <a href={resetUrl} style={{ wordBreak: "break-all", fontSize: 12, color: "var(--teal)" }}>
            {resetUrl}
          </a>
          <div>
            <button
              type="button"
              className="btn btn-sm"
              onClick={() => navigator.clipboard?.writeText(resetUrl)}
            >
              คัดลอกลิงก์
            </button>
          </div>
        </div>
      )}

      <div className="card">
        {users === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>อีเมล</th>
                <th>ชื่อ</th>
                <th>บทบาท</th>
                <th>สถานะ</th>
                <th>เข้าสู่ระบบล่าสุด</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {users.map((u) => (
                <tr key={u.id}>
                  <td>{u.email}</td>
                  <td>{u.full_name_th ?? "—"}</td>
                  <td>{u.roles.length ? u.roles.join(", ") : "—"}</td>
                  <td>
                    <span className={`badge ${u.is_active ? "success" : "danger"}`}>
                      {u.is_active ? "ใช้งาน" : "ปิด"}
                    </span>
                  </td>
                  <td>{u.last_login_at ? new Date(u.last_login_at).toLocaleString("th-TH") : "—"}</td>
                  <td className="num" style={{ whiteSpace: "nowrap" }}>
                    <select
                      aria-label={`เพิ่มบทบาทให้ ${u.email}`}
                      defaultValue=""
                      onChange={(e) => {
                        if (e.target.value) assign(u, e.target.value);
                      }}
                      className="compact-select"
                    >
                      <option value="">เพิ่มบทบาท…</option>
                      {ROLES.map((r) => (
                        <option key={r.value} value={r.value}>
                          {r.label}
                        </option>
                      ))}
                    </select>{" "}
                    {u.is_active && (
                      <>
                        <button className="btn btn-sm" onClick={() => issueReset(u)} disabled={busy}>
                          รีเซ็ต
                        </button>{" "}
                        <button className="btn btn-sm btn-danger" onClick={() => deactivate(u)} disabled={busy}>
                          ปิด
                        </button>
                      </>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}

"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useState } from "react";

export default function AcceptInvitePage() {
  const params = useParams<{ token: string }>();
  const router = useRouter();
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [error, setError] = useState("");
  const [done, setDone] = useState(false);
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    if (password !== confirmPassword) {
      setError("รหัสผ่านทั้งสองช่องไม่ตรงกัน");
      return;
    }
    setBusy(true);
    try {
      const res = await fetch("/api/session/accept-invite", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ token: params.token, password }),
      });
      const body = await res.json().catch(() => ({}));
      if (!res.ok) throw new Error(body.message || "ไม่สามารถตั้งรหัสผ่านได้");
      setDone(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : "ไม่สามารถตั้งรหัสผ่านได้");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="login-wrap">
      <div className="card login-card">
        <div className="login-brand">
          <span className="brand-mark">T</span> TalentOS
        </div>
        {done ? (
          <>
            <h1 style={{ fontSize: 20, marginBottom: 8 }}>ตั้งรหัสผ่านแล้ว</h1>
            <p className="hint" style={{ marginBottom: 18 }}>
              เข้าสู่ระบบด้วยอีเมลและรหัสผ่านใหม่ได้เลย
            </p>
            <button className="btn btn-primary" onClick={() => router.replace("/login")}>
              ไปหน้าเข้าสู่ระบบ
            </button>
          </>
        ) : (
          <form onSubmit={submit} style={{ display: "grid", gap: 14 }}>
            <h1 style={{ fontSize: 20 }}>ตั้งรหัสผ่าน</h1>
            <p className="hint">รหัสผ่านอย่างน้อย 12 ตัวอักษร</p>
            <div className="field">
              <label>รหัสผ่านใหม่</label>
              <input
                type="password"
                autoComplete="new-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
            </div>
            <div className="field">
              <label>ยืนยันรหัสผ่าน</label>
              <input
                type="password"
                autoComplete="new-password"
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
                required
              />
            </div>
            {error && <p className="error">{error}</p>}
            <button className="btn btn-primary" type="submit" disabled={busy}>
              {busy ? "กำลังบันทึก…" : "ตั้งรหัสผ่าน"}
            </button>
            <Link href="/login" className="hint">
              กลับหน้าเข้าสู่ระบบ
            </Link>
          </form>
        )}
      </div>
    </div>
  );
}

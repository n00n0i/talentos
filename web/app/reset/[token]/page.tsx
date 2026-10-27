"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useState } from "react";
import Icon from "@/components/Icon";

export default function ResetPasswordPage() {
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
      const res = await fetch("/api/session/reset-password", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ token: params.token, password }),
      });
      const body = await res.json().catch(() => ({}));
      if (!res.ok) throw new Error(body.message || "ไม่สามารถตั้งรหัสผ่านใหม่ได้");
      setDone(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : "ไม่สามารถตั้งรหัสผ่านใหม่ได้");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="login-wrap">
      <div className="card login-card">
        <div className="login-brand">
          <span className="brand-mark">
            <Icon name="clover" size={20} />
          </span>{" "}
          talentos
        </div>
        {done ? (
          <>
            <h1 style={{ fontSize: 20, marginBottom: 8 }}>ตั้งรหัสผ่านใหม่แล้ว</h1>
            <p className="hint" style={{ marginBottom: 18 }}>
              ลิงก์นี้ใช้ได้ครั้งเดียว — เข้าสู่ระบบด้วยรหัสใหม่ได้เลย
            </p>
            <button className="btn btn-primary" onClick={() => router.replace("/login")}>
              ไปหน้าเข้าสู่ระบบ
            </button>
          </>
        ) : (
          <form onSubmit={submit} style={{ display: "grid", gap: 14 }}>
            <h1 style={{ fontSize: 20 }}>ตั้งรหัสผ่านใหม่</h1>
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

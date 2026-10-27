"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";
import Icon from "@/components/Icon";

export default function LoginPage() {
  const router = useRouter();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [tenant, setTenant] = useState("smoke");
  const [totp, setTotp] = useState("");
  const [errorCode, setErrorCode] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const res = await fetch("/api/session/login", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ email, password, tenant_slug: tenant, ...(totp ? { totp_code: totp } : {}) }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        if (res.status === 401 && (body.error === "totp_required" || body.error === "invalid_totp")) {
          setErrorCode(body.error);
          setTotp("");
          setError(body.error === "invalid_totp" ? "รหัสยืนยันไม่ถูกต้อง ลองอีกครั้ง" : "");
          return;
        }
        throw new Error(body.message || "เข้าสู่ระบบไม่สำเร็จ");
      }
      router.replace("/");
      router.refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "เข้าสู่ระบบไม่สำเร็จ");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="login-wrap">
      <div className="card login-card">
        <div className="login-brand">
          <span className="brand-mark"><Icon name="clover" size={20} /></span> talentos
        </div>
        <h1 style={{ fontSize: 20, marginBottom: 4 }}>เข้าสู่ระบบ</h1>
        <p className="hint" style={{ marginBottom: 20 }}>
          ระบบ HR และเงินเดือนสำหรับองค์กรไทย
        </p>
        <form onSubmit={submit}>
          <div className="field">
            <label htmlFor="tenant">องค์กร</label>
            <input id="tenant" value={tenant} onChange={(e) => setTenant(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="email">อีเมล</label>
            <input
              id="email"
              type="email"
              autoComplete="username"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="password">รหัสผ่าน</label>
            <input
              id="password"
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
            />
          </div>
          {errorCode && (
            <div className="field">
              <label htmlFor="totp">กรอกรหัสจากแอป Authenticator</label>
              <input
                id="totp"
                data-testid="login-totp"
                inputMode="numeric"
                autoComplete="one-time-code"
                pattern="[0-9]{6}"
                maxLength={6}
                autoFocus
                value={totp}
                onChange={(e) => setTotp(e.target.value)}
                required
              />
            </div>
          )}
          {error && <p className="error">{error}</p>}
          <button className="btn btn-primary" type="submit" disabled={busy}>
            {busy ? "กำลังเข้าสู่ระบบ…" : "เข้าสู่ระบบ"}
          </button>
        </form>
      </div>
    </div>
  );
}

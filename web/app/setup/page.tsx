"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";
import Icon from "@/components/Icon";

export default function SetupPage() {
  const router = useRouter();
  const [org, setOrg] = useState("");
  const [slug, setSlug] = useState("");
  const [entity, setEntity] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    if (password !== confirm) {
      setError("รหัสผ่านทั้งสองช่องไม่ตรงกัน");
      return;
    }
    setBusy(true);
    try {
      const res = await fetch("/api/gw/api/v1/setup", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          org_name: org,
          slug,
          entity_name: entity,
          admin_email: email,
          password,
        }),
      });
      const body = await res.json().catch(() => ({}));
      if (!res.ok) throw new Error(body.message || "ตั้งค่าระบบไม่สำเร็จ");
      router.replace("/login");
    } catch (err) {
      setError(err instanceof Error ? err.message : "ตั้งค่าระบบไม่สำเร็จ");
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
        <h1 style={{ fontSize: 20, marginBottom: 4 }}>ติดตั้งระบบครั้งแรก</h1>
        <p className="hint" style={{ marginBottom: 16 }}>
          สร้างองค์กร สาขา และผู้ดูแล HR คนแรก
        </p>
        <form onSubmit={submit} style={{ display: "grid", gap: 12 }}>
          <div className="field">
            <label htmlFor="setup-org">ชื่อองค์กร</label>
            <input id="setup-org" data-testid="setup-org" value={org} onChange={(e) => setOrg(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="setup-slug">รหัสองค์กร (a-z, 0-9, -)</label>
            <input id="setup-slug" data-testid="setup-slug" value={slug} onChange={(e) => setSlug(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="setup-entity">ชื่อสาขา/นิติบุคคล</label>
            <input id="setup-entity" data-testid="setup-entity" value={entity} onChange={(e) => setEntity(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="setup-email">อีเมลผู้ดูแล</label>
            <input id="setup-email" data-testid="setup-email" type="email" value={email} onChange={(e) => setEmail(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="setup-password">รหัสผ่าน (12+ ตัวอักษร)</label>
            <input id="setup-password" data-testid="setup-password" type="password" value={password} onChange={(e) => setPassword(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="setup-confirm">ยืนยันรหัสผ่าน</label>
            <input id="setup-confirm" data-testid="setup-confirm" type="password" value={confirm} onChange={(e) => setConfirm(e.target.value)} required />
          </div>
          {error && <p className="error">{error}</p>}
          <button className="btn btn-primary" data-testid="setup-submit" type="submit" disabled={busy}>
            {busy ? "กำลังติดตั้ง…" : "สร้างองค์กร"}
          </button>
        </form>
      </div>
    </div>
  );
}

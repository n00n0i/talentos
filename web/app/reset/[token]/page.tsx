"use client";

import { useI18N } from "@/lib/i18n";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useState } from "react";
import Icon from "@/components/Icon";

export default function ResetPasswordPage() {
  const { t: tr } = useI18N();
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
      setError(tr("auth.reset.the-two-password-fields-do-not"));
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
      if (!res.ok) throw new Error(body.message || tr("auth.reset.unable-to-set-a-new-password"));
      setDone(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : tr("auth.reset.unable-to-set-a-new-password"));
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
            <h1 style={{ fontSize: 20, marginBottom: 8 }}>{tr("auth.reset.a-new-password-has-been-set")}</h1>
            <p className="hint" style={{ marginBottom: 18 }}>
              {tr("auth.reset.this-link-can-only-be-used")}
            </p>
            <button className="btn btn-primary" onClick={() => router.replace("/login")}>
              {tr("auth.reset.go-to-login-page")}
            </button>
          </>
        ) : (
          <form onSubmit={submit} style={{ display: "grid", gap: 14 }}>
            <h1 style={{ fontSize: 20 }}>{tr("auth.reset.set-a-new-password")}</h1>
            <p className="hint">{tr("auth.reset.password-must-be-at-least-12")}</p>
            <div className="field">
              <label>{tr("auth.reset.new-password")}</label>
              <input
                type="password"
                autoComplete="new-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
            </div>
            <div className="field">
              <label>{tr("auth.reset.confirm-password")}</label>
              <input
                type="password"
                autoComplete="new-password"
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
                required
              />
            </div>
            {error && <p className="error" role="alert">{error}</p>}
            <button className="btn btn-primary" type="submit" disabled={busy}>
              {busy ? tr("auth.reset.saving") : tr("auth.reset.set-password")}
            </button>
            <Link href="/login" className="hint">
              {tr("auth.reset.return-to-login-page")}
            </Link>
          </form>
        )}
      </div>
    </div>
  );
}

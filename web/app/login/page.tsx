"use client";

import { useI18N } from "@/lib/i18n";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState } from "react";
import Icon from "@/components/Icon";

export default function LoginPage() {
  const { t: tr } = useI18N();
  const router = useRouter();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [tenant, setTenant] = useState("e2e");
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
          setError(body.error === "invalid_totp" ? tr("auth.login.verification-code-is-incorrect-try-again") : "");
          return;
        }
        throw new Error(
          [body.error === "gateway_unreachable" ? tr("common.serverUnreachable") : body.message, ...((body.details ?? []) as Array<{ field: string; message: string }>)
            .map((d) => `${d.field}: ${d.message}`)].join(" — ") || tr("auth.login.login-failed"));
      }
      router.replace("/");
      router.refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : tr("auth.login.login-failed"));
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
        <h1 style={{ fontSize: 20, marginBottom: 4 }}>{tr("auth.login.login")}</h1>
        <p className="hint" style={{ marginBottom: 20 }}>
          {tr("auth.login.hr-and-salary-system-for-thai")}
        </p>
        <form onSubmit={submit}>
          <div className="field">
            <label htmlFor="tenant">{tr("auth.login.organization")}</label>
            <input
              id="tenant"
              aria-invalid={error ? true : undefined}
              aria-describedby={error ? "login-error" : undefined}
              value={tenant}
              onChange={(e) => {
                setTenant(e.target.value);
                setErrorCode("");
                setTotp("");
                setError("");
              }}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="email">{tr("auth.login.email")}</label>
            <input
              id="email"
              aria-invalid={error ? true : undefined}
              aria-describedby={error ? "login-error" : undefined}
              type="email"
              autoComplete="username"
              value={email}
              onChange={(e) => {
                setEmail(e.target.value);
                setErrorCode("");
                setTotp("");
                setError("");
              }}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="password">{tr("auth.login.password")}</label>
            <input
              id="password"
              aria-invalid={error ? true : undefined}
              aria-describedby={error ? "login-error" : undefined}
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
            />
          </div>
          {errorCode && (
            <div className="field">
              <label htmlFor="totp">{tr("auth.login.enter-the-code-from-the-app")}</label>
              <input
                id="totp"
              aria-invalid={error ? true : undefined}
              aria-describedby={error ? "login-error" : undefined}
                data-testid="login-totp"
                autoComplete="one-time-code"
                pattern="[A-Z0-9\-]{6,9}"
                maxLength={9}
                autoFocus
                value={totp}
                onChange={(e) => setTotp(e.target.value)}
                required
              />
            </div>
          )}
          {error && <p className="error" id="login-error" role="alert">{error}</p>}
          <button className="btn btn-primary" type="submit" disabled={busy}>
            {busy ? tr("auth.login.logging-in") : tr("auth.login.login")}
          </button>
        </form>
        <p className="hint" style={{ marginTop: 16 }}>
          <Link href="/setup">{tr("auth.login.install-the-system-for-the-first")}</Link>
        </p>
      </div>
    </div>
  );
}

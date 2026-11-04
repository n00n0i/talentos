"use client";

import { useI18N } from "@/lib/i18n";

import { useRouter } from "next/navigation";
import { useState } from "react";
import Icon from "@/components/Icon";

export default function SetupPage() {
  const { t: tr } = useI18N();
  const router = useRouter();
  const [org, setOrg] = useState("");
  const [slug, setSlug] = useState("");
  const [entity, setEntity] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [setupToken, setSetupToken] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    if (password !== confirm) {
      setError(tr("auth.setup.the-two-password-fields-do-not"));
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
          setup_token: setupToken || null,
        }),
      });
      const body = await res.json().catch(() => ({}));
      if (!res.ok) throw new Error((body.error === "gateway_unreachable" ? tr("common.serverUnreachable") : body.message) || tr("auth.setup.system-setup-failed"));
      router.replace("/login");
    } catch (err) {
      setError(err instanceof Error ? err.message : tr("auth.setup.system-setup-failed"));
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
        <h1 style={{ fontSize: 20, marginBottom: 4 }}>{tr("auth.setup.install-the-system-for-the-first")}</h1>
        <p className="hint" style={{ marginBottom: 16 }}>
          {tr("auth.setup.create-your-first-organization-branch-and")}
        </p>
        <form onSubmit={submit} style={{ display: "grid", gap: 12 }}>
          <div className="field">
            <label htmlFor="setup-org">{tr("auth.setup.organization-name")}</label>
            <input id="setup-org" data-testid="setup-org" value={org} onChange={(e) => setOrg(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="setup-slug">{tr("auth.setup.organization-code-a-z-0-9")}</label>
            <input id="setup-slug" data-testid="setup-slug" value={slug} onChange={(e) => setSlug(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="setup-entity">{tr("auth.setup.branch-juristic-person-name")}</label>
            <input id="setup-entity" data-testid="setup-entity" value={entity} onChange={(e) => setEntity(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="setup-email">{tr("auth.setup.administrator-email")}</label>
            <input id="setup-email" data-testid="setup-email" type="email" value={email} onChange={(e) => setEmail(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="setup-password">{tr("auth.setup.password-12-characters")}</label>
            <input id="setup-password" data-testid="setup-password" type="password" value={password} onChange={(e) => setPassword(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="setup-token">{tr("auth.setup.setup-token-only-for-additional-installation")}</label>
            <input id="setup-token" data-testid="setup-token" value={setupToken} onChange={(e) => setSetupToken(e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="setup-confirm">{tr("auth.setup.confirm-password")}</label>
            <input id="setup-confirm" data-testid="setup-confirm" type="password" value={confirm} onChange={(e) => setConfirm(e.target.value)} required />
          </div>
          {error && <p className="error" role="alert" data-testid="setup-error">{error}</p>}
          <button className="btn btn-primary" data-testid="setup-submit" type="submit" disabled={busy}>
            {busy ? tr("auth.setup.installing") : tr("auth.setup.create-an-organization")}
          </button>
        </form>
      </div>
    </div>
  );
}

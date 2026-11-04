"use client";

import type { TranslationKey } from "@/lib/i18n-catalog";

import { useI18N, tNow } from "@/lib/i18n";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/i18n-format";

type User = {
  id: string;
  email: string;
  full_name_th: string | null;
  is_active: boolean;
  roles: string[];
  last_login_at: string | null;
  totp_enabled: boolean;
  line_user_id: string | null;
};

type InviteResult = { invite_path: string; email: string; role: string; expires_at: string };

const ROLES: { value: string; labelKey: TranslationKey }[] = [
  { value: "hr_admin", labelKey: "users.hr-administrator" },
  { value: "payroll_manager", labelKey: "users.payroll-manager" },
  { value: "manager", labelKey: "users.manager" },
  { value: "employee", labelKey: "users.employee" },
];

export default function UsersPage() {
  const { t: tr } = useI18N();
  const [users, setUsers] = useState<User[] | null>(null);
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [role, setRole] = useState("employee");
  const [invite, setInvite] = useState<InviteResult | null>(null);
  const [reset, setReset] = useState<{ email: string; reset_path: string; expires_at: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [lines, setLines] = useState<Record<string, string>>({});

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
      setNotice(tr("users.roleAdded", { role: newRole, email: user.email }));
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

  async function reset2fa(user: User) {
    if (!confirm(tr("users.reset2faConfirm", { email: user.email }))) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/users/${user.id}/2fa/reset`, { method: "POST" });
      setNotice(tr("users.reset2faNotice", { email: user.email }));
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function saveLine(user: User, value: string) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw(`api/v1/users/${user.id}/line`, {
        method: "PUT",
        body: JSON.stringify({ line_user_id: value.trim() || null }),
      });
      setNotice(tr("users.line-id-saved"));
      setLines((prev) => ({ ...prev, [user.id]: value.trim() }));
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function deactivate(user: User) {
    if (!confirm(tr("users.deactivateConfirm", { email: user.email }))) return;
    setBusy(true);
    try {
      await gw(`api/v1/users/${user.id}`, { method: "DELETE" });
      setNotice(tr("users.account-has-been-deactivated"));
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
          <h1>{tr("users.users")}</h1>
          <p className="page-sub">{tr("users.invite-users-and-assign-roles")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="badge success" role="status" aria-live="polite">{notice}</p>}

      <form className="card card-pad" onSubmit={submitInvite} style={{ display: "grid", gap: 14 }}>
        <h2>{tr("users.invite-user")}</h2>
        <div className="form-row">
          <div className="field">
            <label htmlFor="invite-email">{tr("users.email")}</label>
            <input
              id="invite-email"
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="invite-name">{tr("users.name-thai")}</label>
            <input id="invite-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="invite-role">{tr("users.role")}</label>
            <select id="invite-role" value={role} onChange={(e) => setRole(e.target.value)}>
              {ROLES.map((r) => (
                <option key={r.value} value={r.value}>
                  {tr(r.labelKey)}
                </option>
              ))}
            </select>
          </div>
        </div>
        <div>
          <button className="btn btn-primary" type="submit" disabled={busy}>
            {busy ? tr("users.creating") : tr("users.create-an-invitation-link")}
          </button>
        </div>
        {invite && (
          <div style={{ background: "#eef0ff", borderRadius: 6, padding: 12, display: "grid", gap: 6 }}>
            <strong>{tr("users.invitation-link-can-be-used-once")}</strong>
            <a href={inviteUrl} style={{ wordBreak: "break-all", fontSize: 12, color: "var(--teal)" }}>
              {inviteUrl}
            </a>
            <div>
              <button
                type="button"
                className="btn btn-sm"
                onClick={() => navigator.clipboard?.writeText(inviteUrl)}
              >
                {tr("users.copy-link")}
              </button>
            </div>
          </div>
        )}
      </form>

      {reset && (
        <div className="card card-pad" style={{ display: "grid", gap: 6 }}>
          <strong>{tr("users.text")} {reset.email}</strong>
          <span className="hint">{tr("users.can-be-used-once-expires-in")}</span>
          <a href={resetUrl} style={{ wordBreak: "break-all", fontSize: 12, color: "var(--teal)" }}>
            {resetUrl}
          </a>
          <div>
            <button
              type="button"
              className="btn btn-sm"
              onClick={() => navigator.clipboard?.writeText(resetUrl)}
            >
              {tr("users.copy-link")}
            </button>
          </div>
        </div>
      )}

      <div className="card">
        <p className="hint" style={{ padding: "12px 16px 0" }}>
          {tr("users.line-id-must-be-the-user")}
        </p>
        {users === null ? (
          <div className="spinner">{tr("users.loading")}</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{tr("users.email")}</th>
                <th scope="col">{tr("users.name")}</th>
                <th scope="col">{tr("users.role")}</th>
                <th scope="col">{tr("users.status")}</th>
                <th scope="col">{tr("users.latest-login")}</th>
                <th scope="col" />
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
                      {u.is_active ? tr("users.use") : tr("users.close")}
                    </span>
                  </td>
                  <td>{u.last_login_at ? formatDateTime(u.last_login_at) : "—"}</td>
                  <td className="num" style={{ whiteSpace: "normal" }}>
                    <div className="row-actions">
                    <select
                      aria-label={tr("users.addRoleAria", { email: u.email })}
                      defaultValue=""
                      onChange={(e) => {
                        if (e.target.value) assign(u, e.target.value);
                      }}
                      className="compact-select"
                    >
                      <option value="">{tr("users.add-a-role")}</option>
                      {ROLES.map((r) => (
                        <option key={r.value} value={r.value}>
                          {tr(r.labelKey)}
                        </option>
                      ))}
                    </select>{" "}
                    <input
                      data-testid="line-id"
                      aria-label={tr("users.lineIdAria", { email: u.email })}
                      placeholder="U0123…"
                      title={tr("users.the-line-user-id-from-the")}
                      value={lines[u.id] ?? u.line_user_id ?? ""}
                      onChange={(e) => setLines({ ...lines, [u.id]: e.target.value })}
                      maxLength={64}
                      className="compact-select"
                      style={{ width: 200 }}
                    />{" "}
                    <button
                      className="btn btn-sm"
                      data-testid="save-line"
                      onClick={() => saveLine(u, lines[u.id] ?? u.line_user_id ?? "")}
                      disabled={busy}
                    >
                      {tr("users.save-line")}
                    </button>{" "}
                    {u.is_active && (
                      <>
                        <button className="btn btn-sm" onClick={() => issueReset(u)} disabled={busy}>
                          {tr("users.reset")}
                        </button>{" "}
                        {u.totp_enabled && (
                          <>
                            <button
                              className="btn btn-sm"
                              data-testid="reset-2fa"
                              onClick={() => reset2fa(u)}
                              disabled={busy}
                            >
                              {tr("users.reset-2fa")}
                            </button>{" "}
                          </>
                        )}
                        <button className="btn btn-sm btn-danger" onClick={() => deactivate(u)} disabled={busy}>
                          {tr("users.close")}
                        </button>
                      </>
                    )}
                    </div>
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

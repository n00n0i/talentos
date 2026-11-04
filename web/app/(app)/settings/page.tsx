"use client";

import { useEffect, useState } from "react";
import { gw, handleApiError, Me } from "@/lib/api";
import { useI18N, tNow } from "@/lib/i18n";

type Num = string | number;
type PayrollSettings = {
  unpaid_leave_divisor: Num;
  ot_weekday_rate: Num;
  ot_weekend_rate: Num;
  ot_holiday_rate: Num;
  leave_approval_chain: string[];
  ot_approval_chain: string[];
  approval_sla_hours: Num;
  gl_salary_account: string;
  gl_net_account: string;
  gl_tax_account: string;
  gl_sso_account: string;
  gl_pf_account: string;
  gl_loan_account: string;
  gl_claim_account: string;
};
type Tenant = { id: string; name: string; slug: string };
type Entity = {
  id: string;
  name: string;
  tax_id: string | null;
  sso_account: string | null;
  branch: string;
  is_active: boolean;
};
type System = {
  database: { role: string; rls_enforced: boolean; superuser: boolean; bypass_rls: boolean };
  migrations: { tracked: boolean; applied: number; latest: number | null };
  payslip_renderer: { available: boolean };
  model_gateway: { configured: boolean };
  pii_encryption: { configured: boolean };
  notifications: { email: boolean; line: boolean };
};
type Notification = {
  id: string;
  channel: string;
  recipient_masked: string;
  subject: string;
  status: string;
  attempts: number;
  last_error: string | null;
  created_at: string;
  sent_at: string | null;
};
type NotificationSummary = { pending: number; failed: number; sent: number };

type ApprovalRule = {
  id: number;
  leave_type: string | null;
  min_days: string | null;
  chain: string[];
  priority: number;
  created_at: string;
};

const DEFAULTS: PayrollSettings = {
  unpaid_leave_divisor: "30",
  ot_weekday_rate: "1.50",
  ot_weekend_rate: "2.00",
  ot_holiday_rate: "3.00",
  leave_approval_chain: ["hr_admin"],
  ot_approval_chain: ["hr_admin"],
  approval_sla_hours: "48",
  gl_salary_account: "5100",
  gl_net_account: "2100",
  gl_tax_account: "2200",
  gl_sso_account: "2210",
  gl_pf_account: "2220",
  gl_loan_account: "2230",
  gl_claim_account: "5200",
};

const LEAVE_TYPE_LABELS: Record<string, import("@/lib/i18n-catalog").TranslationKey> = {
  annual: "settings.annual-leave",
  sick: "settings.sick-leave",
  personal: "settings.personal-leave",
  maternity: "settings.maternity-leave",
  paternity: "settings.paternity-leave",
  bereavement: "settings.bereavement-leave",
  unpaid: "settings.unpaid-leave",
};

export default function SettingsPage() {
  const { t } = useI18N();
  const [system, setSystem] = useState<System | null>(null);
  const [tenant, setTenant] = useState<Tenant | null>(null);
  const [entities, setEntities] = useState<Entity[]>([]);
  const [policy, setPolicy] = useState<PayrollSettings>(DEFAULTS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [totpEnabled, setTotpEnabled] = useState(false);
  const [enroll, setEnroll] = useState<{ secret: string; otpauth_url: string } | null>(null);
  const [totpCode, setTotpCode] = useState("");
  const [recoveryCodes, setRecoveryCodes] = useState<string[] | null>(null);
  const [recoveryTotp, setRecoveryTotp] = useState("");
    const [sessions, setSessions] = useState<
    { id: number; created_at: string; expires_at: string; ip_address: string | null; user_agent: string | null }[]
  >([]);
  const [notifications, setNotifications] = useState<Notification[]>([]);
  const [notifySummary, setNotifySummary] = useState<NotificationSummary | null>(null);
  const [rules, setRules] = useState<ApprovalRule[]>([]);
  const [ruleType, setRuleType] = useState("");
  const [ruleMinDays, setRuleMinDays] = useState("");
  const [ruleChain, setRuleChain] = useState("hr_admin");
  const [canPurge, setCanPurge] = useState(false);
  const [oidc, setOidc] = useState<{
    configured: boolean;
    issuer?: string;
    client_id?: string;
    client_secret_masked?: string | null;
    allowed_domains?: string[];
    enabled?: boolean;
  } | null>(null);
  const [oidcIssuer, setOidcIssuer] = useState("");
  const [oidcClientId, setOidcClientId] = useState("");
  const [oidcClientSecret, setOidcClientSecret] = useState("");
  const [oidcAllowedDomains, setOidcAllowedDomains] = useState("");
  const [oidcEnabled, setOidcEnabled] = useState(false);
  const [canManageOidc, setCanManageOidc] = useState(false);
  const [pdpaPolicy, setPdpaPolicy] = useState<{
    payroll_retention_years: number;
    ops_retention_months: number;
  } | null>(null);

  useEffect(() => {
    gw<Me>("api/v1/auth/me")
      .then((me) => {
        setTotpEnabled(Boolean(me.totp_enabled));
        if (me.permissions.includes("privacy.purge")) {
          setCanPurge(true);
          gw<{ payroll_retention_years: number; ops_retention_months: number }>(
            "api/v1/privacy/policy"
          )
            .then(setPdpaPolicy)
            .catch(() => {});
        }
        if (me.permissions.includes("auth.oidc_manage")) {
          setCanManageOidc(true);
          loadOidcSettings();
        }
      })
      .catch((e) => handleApiError(e, setError));
    loadSessions();
    loadNotifications();
    loadRules();
    Promise.all([
      gw<System>("api/v1/settings/system"),
      gw<{ tenant: Tenant; entities: Entity[] }>("api/v1/settings/organization"),
      gw<PayrollSettings>("api/v1/settings/payroll"),
    ])
      .then(([sys, org, pol]) => {
        setSystem(sys);
        setTenant(org.tenant);
        setEntities(org.entities);
        setPolicy(pol);
      })
      .catch((e) => handleApiError(e, setError));
  }, []);

  async function loadOidcSettings() {
    try {
      const res = await gw<{
        configured: boolean;
        issuer?: string;
        client_id?: string;
        client_secret_masked?: string | null;
        allowed_domains?: string[];
        enabled?: boolean;
      }>("api/v1/auth/oidc/settings");
      setOidc(res);
      if (res.configured) {
        setOidcIssuer(res.issuer ?? "");
        setOidcClientId(res.client_id ?? "");
        setOidcAllowedDomains((res.allowed_domains ?? []).join(", "));
        setOidcEnabled(Boolean(res.enabled));
      }
    } catch {
      setOidc({ configured: false });
    }
  }

  async function saveOidc(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const domains = oidcAllowedDomains
        .split(",")
        .map((d) => d.trim())
        .filter((d) => d.length > 0);
      await gw("api/v1/auth/oidc/settings", {
        method: "POST",
        body: JSON.stringify({
          issuer: oidcIssuer,
          client_id: oidcClientId,
          client_secret: oidcClientSecret,
          allowed_domains: domains,
          enabled: oidcEnabled,
        }),
      });
      setOidcClientSecret("");
      await loadOidcSettings();
      setNotice(t("settings.oidcSavedNotice"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function purgeNow() {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const res = await gw<{ purged: number }>("api/v1/privacy/retention-purge", {
        method: "POST",
        body: "{}",
      });
      setNotice(t("settings.purgeNotice", { n: res.purged }));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function loadSessions() {
    try {
      const res = await gw<{ sessions: { id: number; created_at: string; expires_at: string; ip_address: string | null; user_agent: string | null }[] }>(
        "api/v1/auth/sessions"
      );
      setSessions(res.sessions);
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  async function loadNotifications() {
    try {
      const res = await gw<{ summary: NotificationSummary; rows: Notification[] }>("api/v1/notifications");
      setNotifySummary(res.summary);
      setNotifications(res.rows);
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  async function loadRules() {
    try {
      const res = await gw<{ rules: ApprovalRule[] }>("api/v1/leave/approval-rules");
      setRules(res.rules);
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  async function addRule(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw("api/v1/leave/approval-rules", {
        method: "POST",
        body: JSON.stringify({
          leave_type: ruleType || null,
          min_days: ruleMinDays ? Number(ruleMinDays) : null,
          chain: ruleChain.split(","),
        }),
      });
      setRuleType("");
      setRuleMinDays("");
      setRuleChain("hr_admin");
      await loadRules();
      setNotice(t("settings.ruleSavedNotice"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function deleteRule(id: number) {
    setError("");
    setNotice("");
    try {
      await gw(`api/v1/leave/approval-rules/${id}`, { method: "DELETE" });
      await loadRules();
      setNotice(t("settings.ruleDeletedNotice"));
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  async function savePolicy(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw("api/v1/settings/payroll", {
        method: "PUT",
        body: JSON.stringify({
          unpaid_leave_divisor: Number(policy.unpaid_leave_divisor),
          ot_weekday_rate: Number(policy.ot_weekday_rate),
          ot_weekend_rate: Number(policy.ot_weekend_rate),
          ot_holiday_rate: Number(policy.ot_holiday_rate),
          leave_approval_chain: policy.leave_approval_chain,
          ot_approval_chain: policy.ot_approval_chain,
          approval_sla_hours: Number(policy.approval_sla_hours),
          gl_salary_account: policy.gl_salary_account,
          gl_net_account: policy.gl_net_account,
          gl_tax_account: policy.gl_tax_account,
          gl_sso_account: policy.gl_sso_account,
          gl_pf_account: policy.gl_pf_account,
          gl_loan_account: policy.gl_loan_account,
          gl_claim_account: policy.gl_claim_account,
        }),
      });
      setNotice(t("settings.savedPolicyNotice"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function saveTenant(e: React.FormEvent) {
    e.preventDefault();
    if (!tenant) return;
    setBusy(true);
    setError("");
    try {
      await gw("api/v1/settings/tenant", {
        method: "PUT",
        body: JSON.stringify({ name: tenant.name }),
      });
      setNotice(t("settings.savedTenantNotice"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function saveEntity(entity: Entity, name: string, taxId: string, ssoAccount: string, branch: string) {
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/settings/entities/${entity.id}`, {
        method: "PUT",
        body: JSON.stringify({ name, tax_id: taxId || null, sso_account: ssoAccount || null, branch: branch || null }),
      });
      setEntities((list) =>
        list.map((e) =>
          e.id === entity.id
            ? { ...e, name, tax_id: taxId || null, sso_account: ssoAccount || null, branch: branch || "000000" }
            : e
        )
      );
      setNotice(t("settings.savedEntityNotice"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function setup2fa() {
    setError("");
    setNotice("");
    try {
      setEnroll(await gw("api/v1/auth/2fa/setup", { method: "POST" }));
      setTotpCode("");
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  async function toggle2fa(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    setNotice("");
    const enabling = !totpEnabled;
    try {
      const res = await gw<{ totp_enabled: boolean; recovery_codes?: string[] }>(
        `api/v1/auth/2fa/${enabling ? "enable" : "disable"}`,
        {
          method: "POST",
          body: JSON.stringify({ code: totpCode }),
        }
      );
      setTotpEnabled(res.totp_enabled);
      setEnroll(null);
      setTotpCode("");
      if (enabling && res.recovery_codes) {
        setRecoveryCodes(res.recovery_codes);
      }
      if (!enabling) {
        setRecoveryCodes(null);
      }
      setNotice(enabling ? t("settings.twoFaEnabledNotice") : t("settings.twoFaDisabledNotice"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  function setPolicyValue(key: keyof PayrollSettings, value: string) {
    setPolicy((p) => ({ ...p, [key]: value }));
  }

  async function regenerateRecoveryCodes(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const res = await gw<{ codes: string[] }>("api/v1/auth/2fa/recovery-codes", {
        method: "POST",
        body: JSON.stringify({ code: recoveryTotp }),
      });
      setRecoveryCodes(res.codes);
      setRecoveryTotp("");
      setNotice(t("settings.recoveryCodesRegeneratedNotice"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function revokeSession(id: number) {
    setError("");
    try {
      await gw(`api/v1/auth/sessions/${id}`, { method: "DELETE" });
      await loadSessions();
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  async function revokeAllSessions() {
    setError("");
    try {
      await gw("api/v1/auth/sessions/revoke-all", { method: "POST" });
      await loadSessions();
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  async function retryNotification(id: string) {
    setError("");
    try {
      await gw(`api/v1/notifications/${id}/retry`, { method: "POST" });
      await loadNotifications();
      setNotice(t("settings.notificationRetriedNotice"));
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{t("settings.title")}</h1>
          <p className="page-sub">{t("settings.subtitle")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="badge success" role="status" aria-live="polite">{notice}</p>}

      {system && !system.database.rls_enforced && (
        <div className="card card-pad" style={{ borderColor: "var(--danger)", background: "#fdeaea" }}>
          <h2 style={{ color: "var(--danger)" }}>{t("settings.rls-is-turned-off")}</h2>
          <p style={{ marginTop: 6 }}>
            {t("settings.app-runs-with-role")} <code>{system.database.role}</code> {t("settings.text")}
            {system.database.superuser ? " superuser" : t("settings.role-with-bypassrls")} {t("settings.text-2")} <code>DATABASE_URL</code> {t("settings.text-3")}{" "}
            <code>talentos_app</code> {t("settings.nobypassrls-and-restart")}
          </p>
        </div>
      )}

      <div className="grid-2">
        <form className="card card-pad" onSubmit={savePolicy} style={{ display: "grid", gap: 14 }}>
          <div>
            <h2>{t("settings.payrollFormulas")}</h2>
            <p className="hint" style={{ marginTop: 4 }}>
              {t("settings.these-values-are-company-policy-statutory")}
            </p>
          </div>
          <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label htmlFor="divisor">{t("settings.daily-wage-divisor-day-month")}</label>
              <input id="divisor"
                type="number"
                min="1"
                max="31"
                step="0.5"
                 data-testid="divisor"
                value={policy.unpaid_leave_divisor}
                onChange={(e) => setPolicyValue("unpaid_leave_divisor", e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="ot-weekday-rate">{t("settings.ot-working-days-times")}</label>
              <input
                id="ot-weekday-rate"
                type="number"
                min="1"
                max="5"
                step="0.25"
                value={policy.ot_weekday_rate}
                onChange={(e) => setPolicyValue("ot_weekday_rate", e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="ot-weekend-rate">{t("settings.holiday-ot-times")}</label>
              <input
                id="ot-weekend-rate"
                type="number"
                min="1"
                max="5"
                step="0.25"
                value={policy.ot_weekend_rate}
                onChange={(e) => setPolicyValue("ot_weekend_rate", e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="ot-holiday-rate">{t("settings.ot-public-holidays-times")}</label>
              <input
                id="ot-holiday-rate"
                type="number"
                min="1"
                max="5"
                step="0.25"
                value={policy.ot_holiday_rate}
                onChange={(e) => setPolicyValue("ot_holiday_rate", e.target.value)}
              />
            </div>
          </div>
          <div className="field">
            <label htmlFor="leave-chain">{t("settings.leave-approval-process")}</label>
            <select
              id="leave-chain"
              data-testid="leave-chain"
              value={policy.leave_approval_chain.join(",")}
              onChange={(e) =>
                setPolicy((p) => ({ ...p, leave_approval_chain: e.target.value.split(",") }))
              }
            >
              <option value="hr_admin">{t("settings.hr-approves-one-step")}</option>
              <option value="manager,hr_admin">{t("settings.supervisor-hr")}</option>
            </select>
          </div>
          <div className="field">
            <label htmlFor="ot-approval-chain">{t("settings.ot-approval-process")}</label>
            <select
              id="ot-approval-chain"
              data-testid="ot-approval-chain"
              value={policy.ot_approval_chain.join(",")}
              onChange={(e) =>
                setPolicy((p) => ({ ...p, ot_approval_chain: e.target.value.split(",") }))
              }
            >
              <option value="hr_admin">{t("settings.hr-approves-one-step")}</option>
              <option value="manager,hr_admin">{t("settings.supervisor-hr")}</option>
            </select>
          </div>
          <div className="field">
            <label htmlFor="approval-sla-hours">{t("settings.leave-approval-sla-hours")}</label>
            <input
              id="approval-sla-hours"
              type="number"
              min="0"
              max="720"
              step="1"
              data-testid="approval-sla-hours"
              value={policy.approval_sla_hours}
              onChange={(e) => setPolicyValue("approval_sla_hours", e.target.value)}
            />
            <p className="hint" style={{ marginTop: 4 }}>
              {t("settings.notify-hr-when-leave-requests-exceed")}
            </p>
          </div>
          <div>
            <h3>{t("settings.gl-account-code")}</h3>
            <p className="hint" style={{ marginTop: 4 }}>
              {t("settings.account-id-for-the-gl-file")}
            </p>
          </div>
          <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label htmlFor="gl-salary">{t("settings.labor-wages-debit")}</label>
              <input id="gl-salary"
                 data-testid="gl-salary"
                value={policy.gl_salary_account}
                onChange={(e) => setPolicyValue("gl_salary_account", e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="gl-net">{t("settings.accrued-salary-credit")}</label>
              <input id="gl-net"
                 data-testid="gl-net"
                value={policy.gl_net_account}
                onChange={(e) => setPolicyValue("gl_net_account", e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="gl-tax">{t("dash.summary.tax")}</label>
              <input id="gl-tax"
                 data-testid="gl-tax"
                value={policy.gl_tax_account}
                onChange={(e) => setPolicyValue("gl_tax_account", e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="gl-sso">{t("dash.summary.sso")}</label>
              <input id="gl-sso"
                 data-testid="gl-sso"
                value={policy.gl_sso_account}
                onChange={(e) => setPolicyValue("gl_sso_account", e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="gl-pf">{t("dash.summary.pf")}</label>
              <input id="gl-pf"
                 data-testid="gl-pf"
                value={policy.gl_pf_account}
                onChange={(e) => setPolicyValue("gl_pf_account", e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="gl-loan">{t("settings.employee-loans")}</label>
              <input id="gl-loan"
                 data-testid="gl-loan"
                value={policy.gl_loan_account}
                onChange={(e) => setPolicyValue("gl_loan_account", e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="gl-claim">{t("settings.employee-expenses")}</label>
              <input id="gl-claim"
                 data-testid="gl-claim"
                value={policy.gl_claim_account}
                onChange={(e) => setPolicyValue("gl_claim_account", e.target.value)}
              />
            </div>
          </div>
          <div>
            <button className="btn btn-primary" data-testid="save-policy" type="submit" disabled={busy}>
              {busy ? t("common.loading") : t("common.save")}
            </button>
          </div>
        </form>

        {canPurge && (
          <div className="card card-pad" data-testid="pdpa-purge-card" style={{ display: "grid", gap: 12 }}>
            <h2 style={{ fontSize: 16 }}>{t("settings.pdpa-data-retention-and-deletion")}</h2>
            <p className="hint">
              {t("settings.tax-payroll-record-retention")} {pdpaPolicy?.payroll_retention_years ?? 7} {t("settings.text-4")} {pdpaPolicy?.ops_retention_months ?? 12} {t("settings.month")}
            </p>
            <button
              className="btn btn-primary"
              data-testid="pdpa-purge"
              disabled={busy}
              onClick={purgeNow}
            >
              {t("settings.delete-expired-data-purge-now")}
            </button>
          </div>
        )}

        <div className="card">
          <div className="card-head">
            <h2>{t("settings.systemStatus")}</h2>
            <span className="hint">{t("settings.check-when-opening-page")}</span>
          </div>
          <table className="table">
            <tbody>
              <StatusRow
                label={t("settings.data-extraction-rls")}
                ok={system?.database.rls_enforced ?? false}
                detail={system ? `role: ${system.database.role}` : "…"}
              />
              <StatusRow
                label="Migrations"
                ok={Boolean(system?.migrations.tracked && system.migrations.applied)}
                detail={
                  !system
                    ? "…"
                    : system.migrations.tracked
                      ? t("settings.migrationsSummary", { n: system.migrations.applied, latest: system.migrations.latest ?? "—" })
                      : t("settings.sqlx-migrations-not-found-run-sqlx")
                }
              />
              <StatusRow
                label={t("settings.slip-pdf-generator")}
                ok={system?.payslip_renderer.available ?? false}
                detail={system?.payslip_renderer.available ? t("settings.chromium-ready") : t("settings.chromium-not-found-pdf-will-answer")}
              />
              <StatusRow
                label={t("settings.pii-encryption")}
                ok={system?.pii_encryption.configured ?? false}
                detail={
                  system?.pii_encryption.configured
                    ? t("settings.card-account-numbers-are-encrypted-before")
                    : t("settings.pii-encryption-key-not-set-new")
                }
              />
              <StatusRow
                label="AI Gateway"
                ok={system?.model_gateway.configured ?? false}
                detail={system?.model_gateway.configured ? t("settings.the-key-has-been-set") : t("settings.huakati-api-key-has-not-been")}
              />
            </tbody>
          </table>
          {system && (
            <p className="hint" data-testid="notify-status" style={{ padding: "0 16px 12px" }}>
              {t("settings.text-5")} {system.notifications.email ? t("settings.ready") : t("settings.not-set-yet")} · LINE{" "}
              {system.notifications.line ? t("settings.ready") : t("settings.not-set-yet")}
            </p>
          )}
        </div>
      </div>

      <div className="card">
        <div className="card-head">
          <h2>{t("settings.approvalRules")}</h2>
          <span className="hint" data-testid="rules-count">{rules.length} {t("settings.text-6")}</span>
        </div>
        <form className="form-row" onSubmit={addRule} style={{ padding: 16, gap: 10, alignItems: "end" }}>
          <div className="field">
            <label htmlFor="rule-type">{t("settings.leave-type")}</label>
            <select id="rule-type"  data-testid="rule-type" value={ruleType} onChange={(e) => setRuleType(e.target.value)}>
              <option value="">{t("settings.all-types")}</option>
              {Object.entries(LEAVE_TYPE_LABELS).map(([value, label]) => (
                <option key={value} value={value}>
                  {t(label)}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor="rule-min-days">{t("settings.minimum-number-of-days-optional")}</label>
            <input id="rule-min-days"
              type="number"
              min="0.5"
              max="365"
              step="0.5"
               data-testid="rule-min-days"
              value={ruleMinDays}
              onChange={(e) => setRuleMinDays(e.target.value)}
              placeholder={t("settings.such-as-5")}
            />
          </div>
          <div className="field">
            <label htmlFor="rule-chain">{t("settings.approval-process")}</label>
            <select id="rule-chain"  data-testid="rule-chain" value={ruleChain} onChange={(e) => setRuleChain(e.target.value)}>
              <option value="hr_admin">{t("settings.hr-approves-one-step")}</option>
              <option value="manager,hr_admin">{t("settings.supervisor-hr")}</option>
            </select>
          </div>
          <div>
            <button className="btn btn-primary" data-testid="add-rule" type="submit" disabled={busy}>
              {t("settings.addRule")}
            </button>
          </div>
        </form>
        {rules.length === 0 ? (
          <p className="hint" data-testid="rules-empty" style={{ padding: "0 16px 16px" }}>
            {t("settings.no-conditions-yet-use-the-company")}
          </p>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{t("settings.type")}</th>
                <th scope="col">{t("settings.minimum-number-of-days")}</th>
                <th scope="col">{t("settings.stage")}</th>
                <th scope="col" />
              </tr>
            </thead>
            <tbody>
              {rules.map((rule) => (
                <tr key={rule.id} data-testid="rule-row">
                  <td>{rule.leave_type ? rule.leave_type in LEAVE_TYPE_LABELS ? t(LEAVE_TYPE_LABELS[rule.leave_type]) : rule.leave_type : t("settings.all-types")}</td>
                  <td className="num">{rule.min_days ? t("settings.minDays", { n: rule.min_days }) : "—"}</td>
                  <td>
                    {rule.chain.map((r) => (r === "manager" ? t("role.manager") : "HR")).join(" → ")}
                  </td>
                  <td className="num">
                    <button className="btn btn-sm btn-danger" onClick={() => deleteRule(rule.id)}>
                      {t("common.delete")}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <div className="card">
        <div className="card-head">
          <h2>{t("settings.notifications")}</h2>
          <span className="hint" data-testid="notify-summary">
            {notifySummary ? (
              <>
                <span className="badge warning">{t("settings.waiting-for-delivery")} {notifySummary.pending}</span>{" "}
                <span className="badge danger">{t("payroll.status.failed")} {notifySummary.failed}</span>{" "}
                <span className="badge success">{t("payroll.pfSent")} {notifySummary.sent}</span>
              </>
            ) : (
              "…"
            )}
          </span>
        </div>
        {notifications.length === 0 ? (
          <p className="hint" data-testid="notify-empty" style={{ padding: "12px 16px" }}>
            {t("settings.there-are-no-notifications")}
          </p>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{t("settings.channel")}</th>
                <th scope="col">{t("settings.recipient")}</th>
                <th scope="col">{t("settings.section")}</th>
                <th scope="col">{t("payroll.thStatus")}</th>
                <th scope="col">{t("settings.time")}</th>
                <th scope="col">{t("settings.latest-error")}</th>
                <th scope="col" />
              </tr>
            </thead>
            <tbody>
              {notifications.map((n) => (
                <tr key={n.id} data-testid="notification-row">
                  <td>{n.channel === "email" ? t("users.email") : "LINE"}</td>
                  <td>{n.recipient_masked}</td>
                  <td>{n.subject}</td>
                  <td>
                    <span className={`badge ${n.status === "sent" ? "success" : n.status === "failed" ? "danger" : "warning"}`}>
                      {n.status === "sent" ? t("payroll.pfSent") : n.status === "failed" ? t("payroll.status.failed") : t("settings.waiting-for-delivery")}
                    </span>
                  </td>
                  <td className="num">{n.attempts}</td>
                  <td className="hint">{n.last_error || "—"}</td>
                  <td className="num">
                    {n.status === "failed" && (
                      <button
                        className="btn btn-sm"
                        data-testid="retry-notification"
                        onClick={() => retryNotification(n.id)}
                      >
                        {t("settings.resend")}
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <form className="card card-pad" onSubmit={toggle2fa} style={{ display: "grid", gap: 12 }}>
        <h2>{t("settings.security2fa")}</h2>
        <p data-testid="totp-status">
          <span className={`badge ${totpEnabled ? "success" : ""}`}>
            {totpEnabled ? t("settings.it-s-enabled") : t("settings.inactive")}
          </span>
        </p>
        {totpEnabled ? (
          <>
            <p className="hint">
              {t("settings.text-7")}
            </p>
            <div className="form-row" style={{ gridTemplateColumns: "1fr auto" }}>
              <div className="field">
                <label htmlFor="totp-disable-code">{t("settings.verification-code")}</label>
                <input
                  id="totp-disable-code"
                  data-testid="totp-disable-code"
                  inputMode="numeric"
                  maxLength={6}
                  value={totpCode}
                  onChange={(e) => setTotpCode(e.target.value)}
                  required
                />
              </div>
              <div style={{ display: "flex", alignItems: "end" }}>
                <button className="btn btn-danger" data-testid="disable-2fa" type="submit" disabled={busy}>
                  {t("settings.disable-2fa")}
                </button>
              </div>
            </div>

            {recoveryCodes && (
              <div data-testid="recovery-codes">
                <p className="badge warning">{t("settings.keep-this-code-in-a-safe")}</p>
                <ul style={{ fontFamily: "monospace", paddingLeft: 20 }}>
                  {recoveryCodes.map((c) => (
                    <li key={c}>{c}</li>
                  ))}
                </ul>
              </div>
            )}

            <form onSubmit={regenerateRecoveryCodes} style={{ display: "grid", gap: 12, marginTop: 12 }}>
              <div className="form-row" style={{ gridTemplateColumns: "1fr auto" }}>
                <div className="field">
                  <label htmlFor="recovery-totp">{t("settings.totp-code-to-create-a-new")}</label>
                  <input
                    id="recovery-totp"
                    data-testid="recovery-totp"
                    inputMode="numeric"
                    maxLength={6}
                    value={recoveryTotp}
                    onChange={(e) => setRecoveryTotp(e.target.value)}
                    required
                  />
                </div>
                <div style={{ display: "flex", alignItems: "end" }}>
                  <button
                    className="btn btn-primary"
                    data-testid="regenerate-recovery"
                    type="submit"
                    disabled={busy}
                  >
                    {t("settings.create-a-new-backup-code")}
                  </button>
                </div>
              </div>
            </form>
          </>
        ) : enroll ? (
          <>
            <p className="hint">
              {t("settings.text-8")}
            </p>
            <code data-testid="totp-secret" style={{ wordBreak: "break-all" }}>
              {enroll.secret}
            </code>
            <code style={{ wordBreak: "break-all", fontSize: 12 }}>{enroll.otpauth_url}</code>
            <div className="form-row" style={{ gridTemplateColumns: "1fr auto" }}>
              <div className="field">
                <label htmlFor="totp-enable-code">{t("settings.verification-code")}</label>
                <input
                  id="totp-enable-code"
                  data-testid="totp-enable-code"
                  inputMode="numeric"
                  maxLength={6}
                  value={totpCode}
                  onChange={(e) => setTotpCode(e.target.value)}
                  required
                />
              </div>
              <div style={{ display: "flex", alignItems: "end" }}>
                <button className="btn btn-primary" data-testid="enable-2fa" type="submit" disabled={busy}>
                  {t("settings.verify-and-turn-on-2fa")}
                </button>
              </div>
            </div>
          </>
        ) : (
          <div>
            <button className="btn btn-primary" data-testid="setup-2fa" type="button" onClick={setup2fa}>
              {t("settings.set-up-2fa")}
            </button>
          </div>
        )}
      </form>

      {canManageOidc && (
        <form className="card card-pad" data-testid="oidc-settings" onSubmit={saveOidc} style={{ display: "grid", gap: 12 }}>
          <h2>{t("settings.ssoOidc")}</h2>
          <p data-testid="oidc-status" style={{ margin: 0 }}>
            <span className={`badge ${oidc?.configured ? "success" : ""}`}>
              {oidc?.configured ? t("settings.oidcStatusConfigured") : t("settings.oidcStatusNotConfigured")}
            </span>
            {oidc?.configured && oidc.client_secret_masked && (
              <span className="hint" style={{ marginLeft: 8 }}>
                secret: {oidc.client_secret_masked}
              </span>
            )}
          </p>
          <p className="hint">
            {t("settings.set-up-oidc-sso-for-login")}
          </p>
          <div className="field">
            <label htmlFor="oidc-issuer">{t("settings.oidcIssuer")}</label>
            <input id="oidc-issuer"
               data-testid="oidc-issuer"
              value={oidcIssuer}
              onChange={(e) => setOidcIssuer(e.target.value)}
              placeholder="https://sso.company.com"
              required
            />
          </div>
          <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label htmlFor="oidc-client-id">{t("settings.oidcClientId")}</label>
              <input id="oidc-client-id"
                 data-testid="oidc-client-id"
                value={oidcClientId}
                onChange={(e) => setOidcClientId(e.target.value)}
                required
              />
            </div>
            <div className="field">
              <label htmlFor="oidc-client-secret">{t("settings.oidcClientSecret")}</label>
              <input id="oidc-client-secret"
                 data-testid="oidc-client-secret"
                type="password"
                value={oidcClientSecret}
                onChange={(e) => setOidcClientSecret(e.target.value)}
                placeholder={oidc?.configured ? t("settings.not-available-not-changed") : "Secret"}
              />
            </div>
          </div>
          <div className="field">
            <label htmlFor="oidc-allowed-domains">{t("settings.oidcAllowedDomains")}</label>
            <input id="oidc-allowed-domains"
               data-testid="oidc-allowed-domains"
              value={oidcAllowedDomains}
              onChange={(e) => setOidcAllowedDomains(e.target.value)}
              placeholder="example.com, corp.example.com"
            />
            <p className="hint">{t("settings.not-available-all-domains-allowed-separated")}</p>
          </div>
          <label style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <input
              type="checkbox"
              data-testid="oidc-enabled"
              checked={oidcEnabled}
              onChange={(e) => setOidcEnabled(e.target.checked)}
              style={{ width: "auto" }}
            />
            <span>{t("settings.oidcEnabled")}</span>
          </label>
          <div>
            <button
              className="btn btn-primary"
              data-testid="oidc-save"
              type="submit"
              disabled={busy || (oidc?.configured && oidcClientSecret === "")}
            >
              {busy ? t("common.loading") : t("common.save")}
            </button>
          </div>
        </form>
      )}

      <div className="card">
        <div className="card-head">
          <h2>{t("sessions.devices")}</h2>
          <span className="hint">{sessions.length} {t("settings.text-9")}</span>
        </div>
        <table className="table">
          <thead>
            <tr>
              <th scope="col">{t("settings.start")}</th>
              <th scope="col">{t("training.expiry")}</th>
              <th scope="col">IP</th>
              <th scope="col">{t("settings.equipment")}</th>
              <th scope="col" />
            </tr>
          </thead>
          <tbody>
            {sessions.map((s) => (
              <tr key={s.id} data-testid="session-row">
                <td>{new Date(s.created_at).toLocaleString("th-TH")}</td>
                <td>{new Date(s.expires_at).toLocaleString("th-TH")}</td>
                <td>{s.ip_address || "—"}</td>
                <td className="hint">{s.user_agent || "—"}</td>
                <td className="num">
                  <button
                    className="btn btn-sm btn-danger"
                    data-testid="revoke-session"
                    onClick={() => revokeSession(s.id)}
                  >
                    {t("settings.exit-this-device")}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <div style={{ padding: 12 }}>
          <button className="btn btn-danger" data-testid="revoke-all-sessions" onClick={revokeAllSessions}>
            {t("settings.log-out-from-every-device")}
          </button>
        </div>
      </div>

      <form className="card card-pad" onSubmit={saveTenant} style={{ display: "grid", gap: 12 }}>
        <h2>{t("settings.organization")}</h2>
        <div className="form-row" style={{ gridTemplateColumns: "1fr auto" }}>
          <div className="field">
            <label htmlFor="organization-name">{t("settings.organization-name")}</label>
            <input
              id="organization-name"
              value={tenant?.name ?? ""}
              onChange={(e) => setTenant((t) => (t ? { ...t, name: e.target.value } : t))}
              required
            />
          </div>
          <div style={{ display: "flex", alignItems: "end" }}>
            <button className="btn" type="submit" disabled={busy || !tenant}>
              {t("settings.save-name")}
            </button>
          </div>
        </div>
        <span className="hint">slug: {tenant?.slug}</span>
      </form>

      <div className="card">
        <div className="card-head">
          <h2>{t("settings.entities")}</h2>
          <span className="hint">{entities.length} {t("settings.text-10")}</span>
        </div>
        <table className="table">
          <thead>
            <tr>
              <th scope="col">{t("employees.name")}</th>
              <th scope="col">{t("settings.tax-id")}</th>
              <th scope="col">{t("settings.social-security-account-number")}</th>
              <th scope="col">{t("settings.branch")}</th>
              <th scope="col">{t("payroll.thStatus")}</th>
              <th scope="col" />
            </tr>
          </thead>
          <tbody>
            {entities.map((entity) => (
              <EntityRow key={entity.id} entity={entity} busy={busy} onSave={saveEntity} />
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}

function StatusRow({ label, ok, detail }: { label: string; ok: boolean; detail: string }) {
  const { t } = useI18N();
  return (
    <tr>
      <td>{label}</td>
      <td>
        <span className={`badge ${ok ? "success" : "danger"}`} data-testid="status-row-badge">{ok ? t("common.ready") : t("common.fix")}</span>
      </td>
      <td className="hint">{detail}</td>
    </tr>
  );
}

function EntityRow({
  entity,
  busy,
  onSave,
}: {
  entity: Entity;
  busy: boolean;
  onSave: (entity: Entity, name: string, taxId: string, ssoAccount: string, branch: string) => void;
}) {
  const { t } = useI18N();
  const [name, setName] = useState(entity.name);
  const [taxId, setTaxId] = useState(entity.tax_id ?? "");
  const [ssoAccount, setSsoAccount] = useState(entity.sso_account ?? "");
  const [branch, setBranch] = useState(entity.branch ?? "");
  return (
    <tr>
      <td>
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          aria-label={t("settings.branchName")}
          className="compact-input"
          style={{ width: "100%", borderRadius: 10 }}
        />
      </td>
      <td>
        <input
          value={taxId}
          onChange={(e) => setTaxId(e.target.value)}
          placeholder={t("settings.13-digits")}
          className="compact-input"
          style={{ width: 160, borderRadius: 10 }}
        />
      </td>
      <td>
        <input
          value={ssoAccount}
          onChange={(e) => setSsoAccount(e.target.value)}
          placeholder={t("settings.employer-account-number")}
          data-testid="entity-sso-account"
          className="compact-input"
          style={{ width: 180, borderRadius: 10 }}
        />
      </td>
      <td>
        <input
          value={branch}
          onChange={(e) => setBranch(e.target.value)}
          placeholder="000000"
          data-testid="entity-branch"
          className="compact-input"
          style={{ width: 100, borderRadius: 10 }}
        />
      </td>
      <td>
        <span className={`badge ${entity.is_active ? "success" : "danger"}`}>
          {entity.is_active ? t("users.use") : t("chat.close")}
        </span>
      </td>
      <td className="num">
        <button className="btn btn-sm" disabled={busy} onClick={() => onSave(entity, name, taxId, ssoAccount, branch)}>
          {t("common.save")}
        </button>
      </td>
    </tr>
  );
}

"use client";

import { useEffect, useState } from "react";
import { gw, handleApiError, Me } from "@/lib/api";

type Num = string | number;
type PayrollSettings = {
  unpaid_leave_divisor: Num;
  ot_weekday_rate: Num;
  ot_weekend_rate: Num;
  ot_holiday_rate: Num;
  leave_approval_chain: string[];
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
type Entity = { id: string; name: string; tax_id: string | null; is_active: boolean };
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
  approval_sla_hours: "48",
  gl_salary_account: "5100",
  gl_net_account: "2100",
  gl_tax_account: "2200",
  gl_sso_account: "2210",
  gl_pf_account: "2220",
  gl_loan_account: "2230",
  gl_claim_account: "5200",
};

const LEAVE_TYPE_LABELS: Record<string, string> = {
  annual: "ลาพักร้อน",
  sick: "ลาป่วย",
  personal: "ลากิจ",
  maternity: "ลาคลอด",
  paternity: "ลาบิดา",
  bereavement: "ลาศพ",
  unpaid: "ลากิจไม่รับค่าจ้าง",
};

export default function SettingsPage() {
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

  useEffect(() => {
    gw<Me>("api/v1/auth/me")
      .then((me) => setTotpEnabled(Boolean(me.totp_enabled)))
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
      setNotice("บันทึกเงื่อนไขการอนุมัติแล้ว");
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
      setNotice("ลบเงื่อนไขแล้ว");
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
      setNotice("บันทึกสูตรเงินเดือนแล้ว — มีผลกับรอบถัดไป");
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
      setNotice("บันทึกชื่อองค์กรแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function saveEntity(entity: Entity, name: string, taxId: string) {
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/settings/entities/${entity.id}`, {
        method: "PUT",
        body: JSON.stringify({ name, tax_id: taxId || null }),
      });
      setEntities((list) => list.map((e) => (e.id === entity.id ? { ...e, name, tax_id: taxId || null } : e)));
      setNotice("บันทึกข้อมูลนิติบุคคลแล้ว");
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
      setNotice(enabling ? "เปิดใช้ 2FA แล้ว" : "ปิด 2FA แล้ว");
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
      setNotice("สร้างรหัสสำรองใหม่แล้ว");
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
      setNotice("ส่งซ้ำการแจ้งเตือนแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  return (
    <>
      <div className="page-head">
        <div>
          <h1>ตั้งค่า</h1>
          <p className="page-sub">บริหารจัดการองค์กร สูตรเงินเดือน และสถานะระบบ</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      {system && !system.database.rls_enforced && (
        <div className="card card-pad" style={{ borderColor: "var(--danger)", background: "#fdeaea" }}>
          <h2 style={{ color: "var(--danger)" }}>RLS ถูกปิดอยู่</h2>
          <p style={{ marginTop: 6 }}>
            แอปรันด้วย role <code>{system.database.role}</code> ซึ่งเป็น
            {system.database.superuser ? " superuser" : " role ที่มี BYPASSRLS"} — การแยกข้อมูลระหว่าง tenant
            จะไม่ถูกบังคับที่ฐานข้อมูล ให้เปลี่ยน <code>DATABASE_URL</code> เป็น role{" "}
            <code>talentos_app</code> (NOBYPASSRLS) แล้วรีสตาร์ท
          </p>
        </div>
      )}

      <div className="grid-2">
        <form className="card card-pad" onSubmit={savePolicy} style={{ display: "grid", gap: 14 }}>
          <div>
            <h2>สูตรเงินเดือนและ OT</h2>
            <p className="hint" style={{ marginTop: 4 }}>
              ค่าเหล่านี้เป็นนโยบายบริษัท — อัตราตามกฎหมาย (SSO/ภาษี) แก้ที่นี่ไม่ได้
            </p>
          </div>
          <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label>ตัวหารค่าแรงรายวัน (วัน/เดือน)</label>
              <input
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
              <label>OT วันทำงาน (เท่า)</label>
              <input
                type="number"
                min="1"
                max="5"
                step="0.25"
                value={policy.ot_weekday_rate}
                onChange={(e) => setPolicyValue("ot_weekday_rate", e.target.value)}
              />
            </div>
            <div className="field">
              <label>OT วันหยุด (เท่า)</label>
              <input
                type="number"
                min="1"
                max="5"
                step="0.25"
                value={policy.ot_weekend_rate}
                onChange={(e) => setPolicyValue("ot_weekend_rate", e.target.value)}
              />
            </div>
            <div className="field">
              <label>OT วันหยุดนักขัตฤกษ์ (เท่า)</label>
              <input
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
            <label htmlFor="leave-chain">ขั้นตอนการอนุมัติการลา</label>
            <select
              id="leave-chain"
              data-testid="leave-chain"
              value={policy.leave_approval_chain.join(",")}
              onChange={(e) =>
                setPolicy((p) => ({ ...p, leave_approval_chain: e.target.value.split(",") }))
              }
            >
              <option value="hr_admin">HR อนุมัติขั้นเดียว</option>
              <option value="manager,hr_admin">หัวหน้างาน → HR</option>
            </select>
          </div>
          <div className="field">
            <label htmlFor="approval-sla-hours">SLA การอนุมัติการลา (ชั่วโมง)</label>
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
              แจ้งเตือน HR เมื่อคำขอลาค้างเกินเวลาที่กำหนด — 0 = ปิด
            </p>
          </div>
          <div>
            <h3>รหัสบัญชี GL</h3>
            <p className="hint" style={{ marginTop: 4 }}>
              รหัสบัญชีสำหรับไฟล์ GL (ไม่เกิน 20 ตัวอักษร ตัวเลข/ตัวอักษร)
            </p>
          </div>
          <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label>ค่าแรง (เดบิต)</label>
              <input
                data-testid="gl-salary"
                value={policy.gl_salary_account}
                onChange={(e) => setPolicyValue("gl_salary_account", e.target.value)}
              />
            </div>
            <div className="field">
              <label>เงินเดือนค้างจ่าย (เครดิต)</label>
              <input
                data-testid="gl-net"
                value={policy.gl_net_account}
                onChange={(e) => setPolicyValue("gl_net_account", e.target.value)}
              />
            </div>
            <div className="field">
              <label>ภาษีหัก ณ ที่จ่าย</label>
              <input
                data-testid="gl-tax"
                value={policy.gl_tax_account}
                onChange={(e) => setPolicyValue("gl_tax_account", e.target.value)}
              />
            </div>
            <div className="field">
              <label>ประกันสังคม</label>
              <input
                data-testid="gl-sso"
                value={policy.gl_sso_account}
                onChange={(e) => setPolicyValue("gl_sso_account", e.target.value)}
              />
            </div>
            <div className="field">
              <label>กองทุนสำรองเลี้ยงชีพ</label>
              <input
                data-testid="gl-pf"
                value={policy.gl_pf_account}
                onChange={(e) => setPolicyValue("gl_pf_account", e.target.value)}
              />
            </div>
            <div className="field">
              <label>เงินให้กู้พนักงาน</label>
              <input
                data-testid="gl-loan"
                value={policy.gl_loan_account}
                onChange={(e) => setPolicyValue("gl_loan_account", e.target.value)}
              />
            </div>
            <div className="field">
              <label>ค่าใช้จ่ายพนักงาน</label>
              <input
                data-testid="gl-claim"
                value={policy.gl_claim_account}
                onChange={(e) => setPolicyValue("gl_claim_account", e.target.value)}
              />
            </div>
          </div>
          <div>
            <button className="btn btn-primary" data-testid="save-policy" type="submit" disabled={busy}>
              {busy ? "กำลังบันทึก…" : "บันทึกสูตร"}
            </button>
          </div>
        </form>

        <div className="card">
          <div className="card-head">
            <h2>สถานะระบบ</h2>
            <span className="hint">ตรวจสอบตอนเปิดหน้า</span>
          </div>
          <table className="table">
            <tbody>
              <StatusRow
                label="การแยกข้อมูล (RLS)"
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
                      ? `${system.migrations.applied} รายการ · ล่าสุด ${system.migrations.latest ?? "—"}`
                      : "ไม่พบ _sqlx_migrations — รัน `sqlx migrate run` เพื่อเริ่มติดตาม"
                }
              />
              <StatusRow
                label="ตัวสร้าง PDF สลิป"
                ok={system?.payslip_renderer.available ?? false}
                detail={system?.payslip_renderer.available ? "Chromium พร้อม" : "ไม่พบ Chromium (PDF จะตอบ 503)"}
              />
              <StatusRow
                label="การเข้ารหัส PII"
                ok={system?.pii_encryption.configured ?? false}
                detail={
                  system?.pii_encryption.configured
                    ? "เลขบัตร/บัญชีถูกเข้ารหัสก่อนบันทึก"
                    : "ยังไม่ตั้ง PII_ENCRYPTION_KEY — ข้อมูลใหม่จะเก็บเป็น plaintext"
                }
              />
              <StatusRow
                label="AI Gateway"
                ok={system?.model_gateway.configured ?? false}
                detail={system?.model_gateway.configured ? "ตั้งค่า key แล้ว" : "ยังไม่ตั้ง HUAKATI_API_KEY"}
              />
            </tbody>
          </table>
          {system && (
            <p className="hint" data-testid="notify-status" style={{ padding: "0 16px 12px" }}>
              การแจ้งเตือน: อีเมล {system.notifications.email ? "พร้อม" : "ยังไม่ตั้งค่า"} · LINE{" "}
              {system.notifications.line ? "พร้อม" : "ยังไม่ตั้งค่า"}
            </p>
          )}
        </div>
      </div>

      <div className="card">
        <div className="card-head">
          <h2>เงื่อนไขการอนุมัติ</h2>
          <span className="hint" data-testid="rules-count">{rules.length} เงื่อนไข</span>
        </div>
        <form className="form-row" onSubmit={addRule} style={{ padding: 16, gap: 10, alignItems: "end" }}>
          <div className="field">
            <label>ประเภทการลา</label>
            <select data-testid="rule-type" value={ruleType} onChange={(e) => setRuleType(e.target.value)}>
              <option value="">ทุกประเภท</option>
              {Object.entries(LEAVE_TYPE_LABELS).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label>จำนวนวันขั้นต่ำ (ไม่บังคับ)</label>
            <input
              type="number"
              min="0.5"
              max="365"
              step="0.5"
              data-testid="rule-min-days"
              value={ruleMinDays}
              onChange={(e) => setRuleMinDays(e.target.value)}
              placeholder="เช่น 5"
            />
          </div>
          <div className="field">
            <label>ขั้นตอนการอนุมัติ</label>
            <select data-testid="rule-chain" value={ruleChain} onChange={(e) => setRuleChain(e.target.value)}>
              <option value="hr_admin">HR อนุมัติขั้นเดียว</option>
              <option value="manager,hr_admin">หัวหน้างาน → HR</option>
            </select>
          </div>
          <div>
            <button className="btn btn-primary" data-testid="add-rule" type="submit" disabled={busy}>
              เพิ่มเงื่อนไข
            </button>
          </div>
        </form>
        {rules.length === 0 ? (
          <p className="hint" data-testid="rules-empty" style={{ padding: "0 16px 16px" }}>
            ยังไม่มีเงื่อนไข — ใช้ขั้นตอนการอนุมัติเริ่มต้นของบริษัท
          </p>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>ประเภท</th>
                <th>จำนวนวันขั้นต่ำ</th>
                <th>ขั้นตอน</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rules.map((rule) => (
                <tr key={rule.id} data-testid="rule-row">
                  <td>{rule.leave_type ? LEAVE_TYPE_LABELS[rule.leave_type] ?? rule.leave_type : "ทุกประเภท"}</td>
                  <td className="num">{rule.min_days ? `${rule.min_days} วัน` : "—"}</td>
                  <td>
                    {rule.chain.map((r) => (r === "manager" ? "หัวหน้างาน" : "HR")).join(" → ")}
                  </td>
                  <td className="num">
                    <button className="btn btn-sm btn-danger" onClick={() => deleteRule(rule.id)}>
                      ลบ
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
          <h2>การแจ้งเตือน</h2>
          <span className="hint" data-testid="notify-summary">
            {notifySummary ? (
              <>
                <span className="badge warning">รอส่ง {notifySummary.pending}</span>{" "}
                <span className="badge danger">ล้มเหลว {notifySummary.failed}</span>{" "}
                <span className="badge success">ส่งแล้ว {notifySummary.sent}</span>
              </>
            ) : (
              "…"
            )}
          </span>
        </div>
        {notifications.length === 0 ? (
          <p className="hint" data-testid="notify-empty" style={{ padding: "12px 16px" }}>
            ไม่มีรายการแจ้งเตือน
          </p>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>ช่องทาง</th>
                <th>ผู้รับ</th>
                <th>หัวข้อ</th>
                <th>สถานะ</th>
                <th>ครั้ง</th>
                <th>ข้อผิดพลาดล่าสุด</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {notifications.map((n) => (
                <tr key={n.id} data-testid="notification-row">
                  <td>{n.channel === "email" ? "อีเมล" : "LINE"}</td>
                  <td>{n.recipient_masked}</td>
                  <td>{n.subject}</td>
                  <td>
                    <span className={`badge ${n.status === "sent" ? "success" : n.status === "failed" ? "danger" : "warning"}`}>
                      {n.status === "sent" ? "ส่งแล้ว" : n.status === "failed" ? "ล้มเหลว" : "รอส่ง"}
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
                        ส่งซ้ำ
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
        <h2>ความปลอดภัย (2FA)</h2>
        <p data-testid="totp-status">
          <span className={`badge ${totpEnabled ? "success" : ""}`}>
            {totpEnabled ? "เปิดใช้งานอยู่" : "ปิดอยู่"}
          </span>
        </p>
        {totpEnabled ? (
          <>
            <p className="hint">
              กรอกรหัส 6 หลักจากแอป Authenticator เพื่อปิด 2FA — หากเข้าถึงแอปไม่ได้
              ผู้ดูแล HR สามารถรีเซ็ต 2FA ให้ได้ที่หน้าผู้ใช้
            </p>
            <div className="form-row" style={{ gridTemplateColumns: "1fr auto" }}>
              <div className="field">
                <label htmlFor="totp-disable-code">รหัสยืนยัน</label>
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
                  ปิด 2FA
                </button>
              </div>
            </div>

            {recoveryCodes && (
              <div data-testid="recovery-codes">
                <p className="badge warning">เก็บรหัสนี้ไว้ในที่ปลอดภัย จะแสดงครั้งเดียว</p>
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
                  <label htmlFor="recovery-totp">รหัส TOTP เพื่อสร้างรหัสสำรองใหม่</label>
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
                    สร้างรหัสสำรองใหม่
                  </button>
                </div>
              </div>
            </form>
          </>
        ) : enroll ? (
          <>
            <p className="hint">
              เปิดแอป Authenticator (เช่น Google Authenticator) เลือกเพิ่มบัญชีแบบกรอกรหัสด้วยตนเอง
              แล้วใส่ secret ด้านล่าง (หรือคัดลอกลิงก์ otpauth) จากนั้นกรอกรหัส 6 หลักที่แอปแสดง
            </p>
            <code data-testid="totp-secret" style={{ wordBreak: "break-all" }}>
              {enroll.secret}
            </code>
            <code style={{ wordBreak: "break-all", fontSize: 12 }}>{enroll.otpauth_url}</code>
            <div className="form-row" style={{ gridTemplateColumns: "1fr auto" }}>
              <div className="field">
                <label htmlFor="totp-enable-code">รหัสยืนยัน</label>
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
                  ยืนยันและเปิด 2FA
                </button>
              </div>
            </div>
          </>
        ) : (
          <div>
            <button className="btn btn-primary" data-testid="setup-2fa" type="button" onClick={setup2fa}>
              ตั้งค่า 2FA
            </button>
          </div>
        )}
      </form>

      <div className="card">
        <div className="card-head">
          <h2>อุปกรณ์และเซสชัน</h2>
          <span className="hint">{sessions.length} เซสชันใช้งานอยู่</span>
        </div>
        <table className="table">
          <thead>
            <tr>
              <th>เริ่ม</th>
              <th>หมดอายุ</th>
              <th>IP</th>
              <th>อุปกรณ์</th>
              <th />
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
                    ออกจากอุปกรณ์นี้
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <div style={{ padding: 12 }}>
          <button className="btn btn-danger" data-testid="revoke-all-sessions" onClick={revokeAllSessions}>
            ออกจากทุกอุปกรณ์
          </button>
        </div>
      </div>

      <form className="card card-pad" onSubmit={saveTenant} style={{ display: "grid", gap: 12 }}>
        <h2>องค์กร</h2>
        <div className="form-row" style={{ gridTemplateColumns: "1fr auto" }}>
          <div className="field">
            <label>ชื่อองค์กร</label>
            <input
              value={tenant?.name ?? ""}
              onChange={(e) => setTenant((t) => (t ? { ...t, name: e.target.value } : t))}
              required
            />
          </div>
          <div style={{ display: "flex", alignItems: "end" }}>
            <button className="btn" type="submit" disabled={busy || !tenant}>
              บันทึกชื่อ
            </button>
          </div>
        </div>
        <span className="hint">slug: {tenant?.slug}</span>
      </form>

      <div className="card">
        <div className="card-head">
          <h2>นิติบุคคล</h2>
          <span className="hint">{entities.length} แห่ง</span>
        </div>
        <table className="table">
          <thead>
            <tr>
              <th>ชื่อ</th>
              <th>เลขผู้เสียภาษี</th>
              <th>สถานะ</th>
              <th />
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
  return (
    <tr>
      <td>{label}</td>
      <td>
        <span className={`badge ${ok ? "success" : "danger"}`}>{ok ? "พร้อม" : "ต้องแก้ไข"}</span>
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
  onSave: (entity: Entity, name: string, taxId: string) => void;
}) {
  const [name, setName] = useState(entity.name);
  const [taxId, setTaxId] = useState(entity.tax_id ?? "");
  return (
    <tr>
      <td>
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="compact-input"
          style={{ width: "100%", borderRadius: 10 }}
        />
      </td>
      <td>
        <input
          value={taxId}
          onChange={(e) => setTaxId(e.target.value)}
          placeholder="13 หลัก"
          className="compact-input"
          style={{ width: 160, borderRadius: 10 }}
        />
      </td>
      <td>
        <span className={`badge ${entity.is_active ? "success" : "danger"}`}>
          {entity.is_active ? "ใช้งาน" : "ปิด"}
        </span>
      </td>
      <td className="num">
        <button className="btn btn-sm" disabled={busy} onClick={() => onSave(entity, name, taxId)}>
          บันทึก
        </button>
      </td>
    </tr>
  );
}

"use client";

import { useEffect, useState } from "react";
import { gw, handleApiError } from "@/lib/api";

type Num = string | number;
type PayrollSettings = {
  unpaid_leave_divisor: Num;
  ot_weekday_rate: Num;
  ot_weekend_rate: Num;
  ot_holiday_rate: Num;
};
type Tenant = { id: string; name: string; slug: string };
type Entity = { id: string; name: string; tax_id: string | null; is_active: boolean };
type System = {
  database: { role: string; rls_enforced: boolean; superuser: boolean; bypass_rls: boolean };
  migrations: { tracked: boolean; applied: number; latest: number | null };
  payslip_renderer: { available: boolean };
  model_gateway: { configured: boolean };
};

const DEFAULTS: PayrollSettings = {
  unpaid_leave_divisor: "30",
  ot_weekday_rate: "1.50",
  ot_weekend_rate: "2.00",
  ot_holiday_rate: "3.00",
};

export default function SettingsPage() {
  const [system, setSystem] = useState<System | null>(null);
  const [tenant, setTenant] = useState<Tenant | null>(null);
  const [entities, setEntities] = useState<Entity[]>([]);
  const [policy, setPolicy] = useState<PayrollSettings>(DEFAULTS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  useEffect(() => {
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

  function setPolicyValue(key: keyof PayrollSettings, value: string) {
    setPolicy((p) => ({ ...p, [key]: value }));
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
                label="AI Gateway"
                ok={system?.model_gateway.configured ?? false}
                detail={system?.model_gateway.configured ? "ตั้งค่า key แล้ว" : "ยังไม่ตั้ง HUAKATI_API_KEY"}
              />
            </tbody>
          </table>
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

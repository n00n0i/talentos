"use client";

import { useEffect, useMemo, useState } from "react";
import { gw, gql, handleApiError, PayrollRun, Payslip, thDate } from "@/lib/api";

type Template = {
  company_display_name: string | null;
  logo_url: string | null;
  accent_color: string;
  footer_text: string | null;
  show_fields: string[];
  paper_size: string;
};

type Pick = { label: string; id: string };

const FIELDS: { key: string; label: string }[] = [
  { key: "ot", label: "ค่าล่วงเวลา" },
  { key: "sso", label: "ประกันสังคม" },
  { key: "pf", label: "กองทุนสำรองเลี้ยงชีพ" },
  { key: "tax", label: "ภาษีหัก ณ ที่จ่าย" },
  { key: "anomalies", label: "รายการผิดปกติ" },
  { key: "bank", label: "บัญชีธนาคาร" },
];

const EMPTY: Template = {
  company_display_name: "",
  logo_url: "",
  accent_color: "#635bff",
  footer_text: "",
  show_fields: FIELDS.map((f) => f.key),
  paper_size: "A4",
};

export default function PayslipPage() {
  const [template, setTemplate] = useState<Template | null>(null);
  const [picks, setPicks] = useState<Pick[]>([]);
  const [selected, setSelected] = useState("");
  const [version, setVersion] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  useEffect(() => {
    Promise.all([gw<Template>("api/v1/payslip/template"), loadPicks()])
      .then(([t, p]) => {
        setTemplate(t);
        setPicks(p);
        if (p[0]) setSelected(p[0].id);
      })
      .catch((e) => handleApiError(e, setError));
  }, []);

  async function loadPicks(): Promise<Pick[]> {
    const runs = await gql<{ payrollRuns: PayrollRun[] }>(
      "{ payrollRuns(limit: 6) { id periodStart periodEnd paymentDate status totalEmployees totalGross totalNet totalTax totalSso totalPf } }",
    );
    const out: Pick[] = [];
    for (const run of runs.payrollRuns) {
      try {
        const r = await gw<{ payslips: Payslip[] }>(`api/v1/payroll/runs/${run.id}/payslips`);
        for (const p of r.payslips.slice(0, 3)) {
          out.push({ id: p.payslip_id, label: `${thDate(run.periodStart)} · ${p.employee_id.slice(0, 8)}` });
        }
      } catch {
        // run without payslips yet
      }
      if (out.length >= 12) break;
    }
    return out;
  }

  const previewUrl = useMemo(() => {
    if (!selected) return "";
    return `/api/gw/api/v1/payroll/payslips/${selected}/render?v=${version}`;
  }, [selected, version]);

  function update<K extends keyof Template>(key: K, value: Template[K]) {
    setTemplate((t) => (t ? { ...t, [key]: value } : t));
  }

  function toggleField(key: string) {
    if (!template) return;
    const has = template.show_fields.includes(key);
    update("show_fields", has ? template.show_fields.filter((f) => f !== key) : [...template.show_fields, key]);
  }

  async function save(e: React.FormEvent) {
    e.preventDefault();
    if (!template) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const body = {
        ...template,
        company_display_name: template.company_display_name?.trim() || null,
        logo_url: template.logo_url?.trim() || null,
        footer_text: template.footer_text?.trim() || null,
      };
      await gw("api/v1/payslip/template", { method: "PUT", body: JSON.stringify(body) });
      setNotice("บันทึกแบบฟอร์มแล้ว");
      setVersion((v) => v + 1);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  if (!template) {
    return <div className="spinner">{error || "กำลังโหลด…"}</div>;
  }

  return (
    <>
      <div className="page-head">
        <div>
          <h1>แบบฟอร์มสลิป</h1>
          <p className="page-sub">ปรับหัวสลิป สี และรายการที่แสดง — ตัวอย่างด้านขวาใช้ข้อมูลจริง</p>
        </div>
        {selected && (
          <a className="btn" href={`/api/gw/api/v1/payroll/payslips/${selected}/pdf`} target="_blank">
            ดาวน์โหลด PDF
          </a>
        )}
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      <div className="editor">
        <form className="card card-pad" onSubmit={save} style={{ display: "grid", gap: 16 }}>
          <div className="field">
            <label htmlFor="company">ชื่อบริษัทบนสลิป</label>
            <input
              id="company"
              placeholder="เว้นว่างเพื่อใช้ชื่อนิติบุคคล"
              value={template.company_display_name ?? ""}
              onChange={(e) => update("company_display_name", e.target.value)}
            />
          </div>

          <div className="field">
            <label htmlFor="logo">URL โลโก้ (https)</label>
            <input
              id="logo"
              placeholder="https://…"
              value={template.logo_url ?? ""}
              onChange={(e) => update("logo_url", e.target.value)}
            />
          </div>

          <div className="field">
            <label htmlFor="color">สีหลัก</label>
            <div className="color-row">
              <input
                id="color"
                type="color"
                value={/^#[0-9a-fA-F]{6}$/.test(template.accent_color) ? template.accent_color : "#635bff"}
                onChange={(e) => update("accent_color", e.target.value)}
              />
              <input
                aria-label="รหัสสี"
                value={template.accent_color}
                onChange={(e) => update("accent_color", e.target.value)}
              />
            </div>
          </div>

          <div className="field">
            <label htmlFor="paper">ขนาดกระดาษ</label>
            <select id="paper" value={template.paper_size} onChange={(e) => update("paper_size", e.target.value)}>
              <option value="A4">A4</option>
              <option value="A5">A5</option>
              <option value="Letter">Letter</option>
            </select>
          </div>

          <div className="field">
            <label>รายการที่แสดง</label>
            <div className="fields-grid">
              {FIELDS.map((f) => (
                <label className="check" key={f.key}>
                  <input
                    type="checkbox"
                    checked={template.show_fields.includes(f.key)}
                    onChange={() => toggleField(f.key)}
                  />
                  {f.label}
                </label>
              ))}
            </div>
          </div>

          <div className="field">
            <label htmlFor="footer">ข้อความท้ายสลิป</label>
            <textarea
              id="footer"
              maxLength={500}
              value={template.footer_text ?? ""}
              onChange={(e) => update("footer_text", e.target.value)}
            />
            <span className="hint">ไม่เกิน 500 ตัวอักษร</span>
          </div>

          <button className="btn btn-primary" type="submit" disabled={busy}>
            {busy ? "กำลังบันทึก…" : "บันทึกแบบฟอร์ม"}
          </button>
        </form>

        <div style={{ display: "grid", gap: 10 }}>
          <div className="field">
            <label htmlFor="payslip">ตัวอย่างจากสลิป</label>
            <select id="payslip" value={selected} onChange={(e) => setSelected(e.target.value)}>
              {picks.length === 0 && <option value="">ยังไม่มีสลิปในระบบ</option>}
              {picks.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
          </div>
          {previewUrl ? (
            <iframe key={previewUrl} className="preview-frame" src={previewUrl} title="ตัวอย่างสลิป" />
          ) : (
            <div className="card empty">สร้างรอบเงินเดือนก่อนเพื่อดูตัวอย่างสลิปจริง</div>
          )}
        </div>
      </div>
    </>
  );
}

"use client";

import { useEffect, useState } from "react";
import { gw, handleApiError } from "@/lib/api";

type Field = { key: string; label: string; source: string };
type HeaderField = { key: string; label: string; source?: string | null; value?: string | null };
type Column = { label: string; source: string };
type Config = { title: string; fields?: Field[]; header?: HeaderField[]; columns?: Column[] };
type Template = { form_type: string; version: number; config: Config; is_tenant_override: boolean };

const ALLOWED_SOURCES = [
  "entity_name",
  "entity_tax_id",
  "entity_address",
  "sso_account",
  "branch",
  "employee_name",
  "employee_code",
  "national_id",
  "gross",
  "tax",
  "sso_employee",
  "sso_employer",
  "annual_income",
  "annual_tax",
  "payment_date",
  "period_start",
  "period_end",
  "income_type",
  "cert_no",
  "year",
  "bank_name",
  "bank_account",
  "net",
];

type FormType = "50tawi" | "sso1_10" | "pnd1" | "pnd1a" | "bank_payout";

function newKey(prefix: string, used: string[]) {
  let i = used.length + 1;
  while (used.includes(`${prefix}${i}`)) i++;
  return `${prefix}${i}`;
}

export default function FormsPage() {
  const [formType, setFormType] = useState<FormType>("50tawi");
  const [config, setConfig] = useState<Config | null>(null);
  const [version, setVersion] = useState<number | null>(null);
  const [isOverride, setIsOverride] = useState(false);
  const [payslipId, setPayslipId] = useState("");
  const [runId, setRunId] = useState("");
  const [year, setYear] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  async function load(type: string) {
    setError("");
    setNotice("");
    try {
      const t = await gw<Template>(`api/v1/forms/${type}`);
      setConfig(t.config);
      setVersion(t.version);
      setIsOverride(t.is_tenant_override);
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  useEffect(() => {
    load(formType);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [formType]);

  function switchType(type: FormType) {
    if (type === formType) return;
    setFormType(type);
    setConfig(null);
    setVersion(null);
  }

  function updateTitle(title: string) {
    setConfig((c) => (c ? { ...c, title } : c));
  }

  function setField(i: number, patch: Partial<Field>) {
    setConfig((c) => {
      if (!c) return c;
      const fields = (c.fields ?? []).map((f, idx) => (idx === i ? { ...f, ...patch } : f));
      return { ...c, fields };
    });
  }

  function addField() {
    setConfig((c) => {
      if (!c) return c;
      const fields = c.fields ?? [];
      const key = newKey("f", fields.map((f) => f.key));
      return { ...c, fields: [...fields, { key, label: "", source: "entity_name" }] };
    });
  }

  function removeField(i: number) {
    setConfig((c) => (c ? { ...c, fields: (c.fields ?? []).filter((_, idx) => idx !== i) } : c));
  }

  function setHeader(i: number, patch: Partial<HeaderField>) {
    setConfig((c) => {
      if (!c) return c;
      const header = (c.header ?? []).map((h, idx) => (idx === i ? { ...h, ...patch } : h));
      return { ...c, header };
    });
  }

  function addHeader() {
    setConfig((c) => {
      if (!c) return c;
      const header = c.header ?? [];
      const key = newKey("h", header.map((h) => h.key));
      return { ...c, header: [...header, { key, label: "", source: "entity_name", value: null }] };
    });
  }

  function removeHeader(i: number) {
    setConfig((c) => (c ? { ...c, header: (c.header ?? []).filter((_, idx) => idx !== i) } : c));
  }

  function setColumn(i: number, patch: Partial<Column>) {
    setConfig((c) => {
      if (!c) return c;
      const columns = (c.columns ?? []).map((col, idx) => (idx === i ? { ...col, ...patch } : col));
      return { ...c, columns };
    });
  }

  function addColumn() {
    setConfig((c) => {
      if (!c) return c;
      const columns = c.columns ?? [];
      return { ...c, columns: [...columns, { label: "", source: "gross" }] };
    });
  }

  function removeColumn(i: number) {
    setConfig((c) => (c ? { ...c, columns: (c.columns ?? []).filter((_, idx) => idx !== i) } : c));
  }

  async function save(e: React.FormEvent) {
    e.preventDefault();
    if (!config) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const body = {
        title: config.title,
        fields: formType === "50tawi" ? config.fields ?? [] : [],
        header: formType === "50tawi" ? [] : config.header ?? [],
        columns: formType === "50tawi" ? [] : config.columns ?? [],
      };
      const res = await gw<{ version: number }>(`api/v1/forms/${formType}`, {
        method: "PUT",
        body: JSON.stringify(body),
      });
      setVersion(res.version);
      setIsOverride(true);
      setNotice(`บันทึกแบบฟอร์มแล้ว (เวอร์ชัน ${res.version})`);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  const payslipPreview =
    payslipId.trim() && formType === "50tawi"
      ? `/api/gw/api/v1/payroll/payslips/${encodeURIComponent(payslipId.trim())}/50tawi`
      : "";
  const runPreview =
    runId.trim() && (formType === "sso1_10" || formType === "pnd1" || formType === "bank_payout")
      ? `/api/gw/api/v1/payroll/runs/${encodeURIComponent(runId.trim())}/${
          formType === "sso1_10" ? "sso1-10" : formType === "pnd1" ? "pnd1" : "bank-payout"
        }.csv`
      : "";
  const yearPreview =
    year.trim() && formType === "pnd1a"
      ? `/api/gw/api/v1/reports/pnd1a.csv?year=${encodeURIComponent(year.trim())}`
      : "";

  return (
    <>
      <div className="page-head">
        <div>
          <h1>แบบฟอร์ม</h1>
          <p className="page-sub">แก้ไขแม่แบบเอกสารราชการ (50 ทวิ / สปส.1-10 / ภ.ง.ด.1 / ภ.ง.ด.1ก) ก่อนส่งมอบนักบัญชี</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      <div className="card card-pad" style={{ display: "grid", gap: 14 }}>
        <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
          <div className="field">
            <label htmlFor="form-type">ประเภทแบบฟอร์ม</label>
            <select
              id="form-type"
              data-testid="form-type"
              value={formType}
              onChange={(e) => switchType(e.target.value as FormType)}
            >
              <option value="50tawi">50 ทวิ (หนังสือรับรองการหักภาษี ณ ที่จ่าย)</option>
              <option value="sso1_10">สปส.1-10 (รายการส่งเงินสมทบ)</option>
              <option value="pnd1">ภ.ง.ด.1 (ภาษีหัก ณ ที่จ่ายรายเดือน)</option>
              <option value="pnd1a">ภ.ง.ด.1ก (ภาษีหัก ณ ที่จ่ายรายปี)</option>
              <option value="bank_payout">ไฟล์โอนเงิน (Bank Payout)</option>
            </select>
          </div>
          <div className="field">
            <label>สถานะ</label>
            <p className="hint" style={{ marginTop: 4 }}>
              {isOverride ? `ใช้แม่แบบขององค์กร · เวอร์ชัน ${version}` : `ใช้ค่าเริ่มต้น · เวอร์ชัน ${version}`}
            </p>
          </div>
        </div>

        {config ? (
          <form onSubmit={save} style={{ display: "grid", gap: 14 }}>
            <div className="field">
              <label htmlFor="form-title">ชื่อเอกสาร</label>
              <input
                id="form-title"
                data-testid="form-title"
                value={config.title}
                onChange={(e) => updateTitle(e.target.value)}
                required
              />
            </div>

            {formType === "50tawi" && (
              <div>
                <div className="card-head" style={{ padding: "0 0 8px" }}>
                  <h3>ช่องข้อมูล</h3>
                  <button type="button" className="btn btn-sm" data-testid="form-add-row" onClick={addField}>
                    เพิ่มช่อง
                  </button>
                </div>
                <table className="table">
                  <thead>
                    <tr>
                      <th>ป้ายกำกับ</th>
                      <th>แหล่งข้อมูล</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {(config.fields ?? []).map((f, i) => (
                      <tr key={f.key}>
                        <td>
                          <input
                            className="compact-input"
                            style={{ width: "100%" }}
                            value={f.label}
                            placeholder="เช่น ผู้จ่ายเงิน"
                            onChange={(e) => setField(i, { label: e.target.value })}
                          />
                        </td>
                        <td>
                          <SourceSelect value={f.source} onChange={(v) => setField(i, { source: v })} />
                        </td>
                        <td className="num">
                          <button type="button" className="btn btn-sm btn-danger" onClick={() => removeField(i)}>
                            ลบ
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}

            {formType !== "50tawi" && (
              <>
                <div>
                  <div className="card-head" style={{ padding: "0 0 8px" }}>
                    <h3>ข้อมูลหัวเอกสาร</h3>
                    <button type="button" className="btn btn-sm" data-testid="form-add-row" onClick={addHeader}>
                      เพิ่มแถว
                    </button>
                  </div>
                  <table className="table">
                    <thead>
                      <tr>
                        <th>ป้ายกำกับ</th>
                        <th>แหล่งข้อมูล / ค่าคงที่</th>
                        <th />
                      </tr>
                    </thead>
                    <tbody>
                      {(config.header ?? []).map((h, i) => (
                        <tr key={h.key}>
                          <td>
                            <input
                              className="compact-input"
                              style={{ width: "100%" }}
                              value={h.label}
                              placeholder="เช่น ชื่อสถานประกอบการ"
                              onChange={(e) => setHeader(i, { label: e.target.value })}
                            />
                          </td>
                          <td>
                            <HeaderValueEditor
                              header={h}
                              onChange={(patch) => setHeader(i, patch)}
                            />
                          </td>
                          <td className="num">
                            <button type="button" className="btn btn-sm btn-danger" onClick={() => removeHeader(i)}>
                              ลบ
                            </button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>

                <div>
                  <div className="card-head" style={{ padding: "0 0 8px" }}>
                    <h3>คอลัมน์ตาราง</h3>
                    <button type="button" className="btn btn-sm" data-testid="form-add-row" onClick={addColumn}>
                      เพิ่มคอลัมน์
                    </button>
                  </div>
                  <table className="table">
                    <thead>
                      <tr>
                        <th>หัวคอลัมน์</th>
                        <th>แหล่งข้อมูล</th>
                        <th />
                      </tr>
                    </thead>
                    <tbody>
                      {(config.columns ?? []).map((col, i) => (
                        <tr key={i}>
                          <td>
                            <input
                              className="compact-input"
                              style={{ width: "100%" }}
                              value={col.label}
                              placeholder="เช่น เลขประจำตัวประชาชน"
                              onChange={(e) => setColumn(i, { label: e.target.value })}
                            />
                          </td>
                          <td>
                            <SourceSelect value={col.source} onChange={(v) => setColumn(i, { source: v })} />
                          </td>
                          <td className="num">
                            <button type="button" className="btn btn-sm btn-danger" onClick={() => removeColumn(i)}>
                              ลบ
                            </button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </>
            )}

            <div>
              <button className="btn btn-primary" data-testid="form-save" type="submit" disabled={busy}>
                {busy ? "กำลังบันทึก…" : "บันทึกแม่แบบ"}
              </button>
            </div>
          </form>
        ) : (
          <p className="hint">กำลังโหลด…</p>
        )}

        <p className="hint" style={{ marginTop: 4 }}>
          แหล่งข้อมูลที่ใช้ได้: {ALLOWED_SOURCES.join(", ")}
        </p>
      </div>

      <div className="card card-pad" style={{ display: "grid", gap: 14 }}>
        <h2>ดูตัวอย่าง</h2>
        {formType === "50tawi" ? (
          <div className="form-row" style={{ gridTemplateColumns: "1fr auto" }}>
            <div className="field">
              <label htmlFor="preview-payslip-id">รหัสสลิป (payslip id)</label>
              <input
                id="preview-payslip-id"
                data-testid="preview-payslip-id"
                value={payslipId}
                onChange={(e) => setPayslipId(e.target.value)}
                placeholder="uuid ของสลิปที่ผ่านมา"
              />
            </div>
            <div style={{ display: "flex", alignItems: "end" }}>
              <a
                className="btn btn-primary"
                data-testid="form-preview"
                href={payslipPreview}
                target="_blank"
                rel="noreferrer"
                aria-disabled={!payslipPreview}
              >
                ดู 50 ทวิ
              </a>
            </div>
          </div>
        ) : formType === "pnd1a" ? (
          <div className="form-row" style={{ gridTemplateColumns: "1fr auto" }}>
            <div className="field">
              <label htmlFor="preview-year">ปีภาษี (คริสต์ศักราช)</label>
              <input
                id="preview-year"
                data-testid="preview-year"
                value={year}
                onChange={(e) => setYear(e.target.value)}
                placeholder="เช่น 2026"
              />
            </div>
            <div style={{ display: "flex", alignItems: "end" }}>
              <a
                className="btn btn-primary"
                data-testid="form-preview"
                href={yearPreview}
                target="_blank"
                rel="noreferrer"
                aria-disabled={!yearPreview}
              >
                ดาวน์โหลด ภ.ง.ด.1ก
              </a>
            </div>
          </div>
        ) : (
          <div className="form-row" style={{ gridTemplateColumns: "1fr auto" }}>
            <div className="field">
              <label htmlFor="preview-run-id">รหัสรอบเงินเดือน (run id)</label>
              <input
                id="preview-run-id"
                data-testid="preview-run-id"
                value={runId}
                onChange={(e) => setRunId(e.target.value)}
                placeholder="uuid ของรอบที่อนุมัติแล้ว"
              />
            </div>
            <div style={{ display: "flex", alignItems: "end" }}>
              <a
                className="btn btn-primary"
                data-testid="form-preview"
                href={runPreview}
                target="_blank"
                rel="noreferrer"
                aria-disabled={!runPreview}
              >
                ดาวน์โหลด {formType === "pnd1" ? "ภ.ง.ด.1" : formType === "bank_payout" ? "ไฟล์โอนเงิน" : "สปส.1-10"}
              </a>
            </div>
          </div>
        )}
      </div>
    </>
  );
}

function SourceSelect({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <select className="compact-input" style={{ width: "100%" }} value={value} onChange={(e) => onChange(e.target.value)}>
      {ALLOWED_SOURCES.map((s) => (
        <option key={s} value={s}>
          {s}
        </option>
      ))}
    </select>
  );
}

function HeaderValueEditor({
  header,
  onChange,
}: {
  header: HeaderField;
  onChange: (patch: Partial<HeaderField>) => void;
}) {
  const isStatic = header.source === null || header.source === undefined || header.source === "";
  return (
    <div className="form-row" style={{ gridTemplateColumns: "auto 1fr" }}>
      <select
        className="compact-input"
        value={isStatic ? "value" : "source"}
        onChange={(e) => {
          if (e.target.value === "value") onChange({ value: header.value ?? "", source: null });
          else onChange({ source: "entity_name", value: null });
        }}
      >
        <option value="source">แหล่งข้อมูล</option>
        <option value="value">ค่าคงที่</option>
      </select>
      {isStatic ? (
        <input
          className="compact-input"
          value={header.value ?? ""}
          placeholder="เช่น 0000000000"
          onChange={(e) => onChange({ value: e.target.value })}
        />
      ) : (
        <SourceSelect value={header.source ?? "entity_name"} onChange={(v) => onChange({ source: v })} />
      )}
    </div>
  );
}

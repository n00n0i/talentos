"use client";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, thb } from "@/lib/api";

type Employee = {
  id: string;
  entity_id: string;
  employee_code: string;
  first_name_th: string;
  last_name_th: string;
  first_name_en: string | null;
  last_name_en: string | null;
  position_title: string;
  department: string | null;
  base_salary: string;
  hire_date: string;
  termination_date: string | null;
  status: string;
  pf_member: boolean;
  sso_enrolled: boolean;
  bank_name: string | null;
  bank_account_number_masked: string | null;
  national_id_masked: string | null;
  sso_number_masked: string | null;
  user_id: string | null;
};

type FormState = {
  employee_code: string;
  first_name_th: string;
  last_name_th: string;
  position_title: string;
  department: string;
  base_salary: string;
  hire_date: string;
  pf_member: boolean;
  sso_enrolled: boolean;
  bank_name: string;
  bank_account_number: string;
  national_id: string;
  sso_number: string;
  user_id: string;
};

const EMPTY: FormState = {
  employee_code: "",
  first_name_th: "",
  last_name_th: "",
  position_title: "",
  department: "",
  base_salary: "",
  hire_date: new Date().toISOString().slice(0, 10),
  pf_member: false,
  sso_enrolled: true,
  bank_name: "",
  bank_account_number: "",
  national_id: "",
  sso_number: "",
  user_id: "",
};

type ImportRow = Record<string, string | boolean>;
type ImportResult = {
  dry_run: boolean;
  total: number;
  valid: number;
  invalid: number;
  created: number;
  rows: { row: number; employee_code: string; status: "valid" | "invalid"; errors: string[] }[];
};

const IMPORT_REQUIRED = [
  "employee_code",
  "first_name_th",
  "last_name_th",
  "position_title",
  "base_salary",
  "hire_date",
];
const IMPORT_OPTIONAL = [
  "department",
  "first_name_en",
  "last_name_en",
  "tax_filing_status",
  "pf_member",
  "sso_enrolled",
];
const IMPORT_BOOLS = ["pf_member", "sso_enrolled"];

function parseCsv(text: string): string[][] {
  const out: string[][] = [];
  let row: string[] = [];
  let cell = "";
  let quoted = false;
  const src = text.replace(/^\uFEFF/, "");
  for (let i = 0; i < src.length; i++) {
    const c = src[i];
    if (quoted) {
      if (c === '"' && src[i + 1] === '"') {
        cell += '"';
        i++;
      } else if (c === '"') quoted = false;
      else cell += c;
    } else if (c === '"') quoted = true;
    else if (c === ",") {
      row.push(cell);
      cell = "";
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && src[i + 1] === "\n") i++;
      row.push(cell);
      cell = "";
      out.push(row);
      row = [];
    } else cell += c;
  }
  if (cell !== "" || row.length) {
    row.push(cell);
    out.push(row);
  }
  return out.filter((r) => r.some((c) => c.trim() !== ""));
}

function csvToImportRows(text: string): ImportRow[] {
  const [header, ...body] = parseCsv(text);
  if (!header) throw new Error("ไฟล์ว่างเปล่า");
  const cols = header.map((h) => h.trim());
  const missing = IMPORT_REQUIRED.filter((h) => !cols.includes(h));
  if (missing.length) throw new Error(`ไม่พบคอลัมน์ที่จำเป็น: ${missing.join(", ")}`);
  if (body.length === 0) throw new Error("ไม่มีข้อมูลพนักงานในไฟล์");
  return body.map((cells, i) => {
    const r: ImportRow = {};
    for (const key of [...IMPORT_REQUIRED, ...IMPORT_OPTIONAL]) {
      const idx = cols.indexOf(key);
      const v = idx < 0 ? "" : (cells[idx] ?? "").trim();
      if (v === "") continue;
      if (IMPORT_BOOLS.includes(key)) {
        const b = v.toLowerCase();
        if (!["true", "1", "yes", "false", "0", "no"].includes(b))
          throw new Error(`แถว ${i + 1}: ค่า ${key} ต้องเป็น true หรือ false`);
        r[key] = ["true", "1", "yes"].includes(b);
      } else r[key] = v;
    }
    return r;
  });
}

export default function EmployeesPage() {
  const [rows, setRows] = useState<Employee[] | null>(null);
  const [query, setQuery] = useState("");
  const [showTerminated, setShowTerminated] = useState(false);
  const [editing, setEditing] = useState<Employee | "new" | null>(null);
  const [form, setForm] = useState<FormState>(EMPTY);
  const [busy, setBusy] = useState(false);
  const [users, setUsers] = useState<{ id: string; email: string; full_name_th: string | null }[]>([]);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [importRows, setImportRows] = useState<ImportRow[]>([]);
  const [importResult, setImportResult] = useState<ImportResult | null>(null);
  const [importKey, setImportKey] = useState(0);

  const load = useCallback(async (all: boolean) => {
    const r = await gw<{ employees: Employee[] }>(
      `api/v1/employees?status=${all ? "all" : "active"}`,
    );
    setRows(r.employees);
  }, []);

  useEffect(() => {
    load(showTerminated).catch((e) => handleApiError(e, setError));
  }, [load, showTerminated]);

  useEffect(() => {
    gw<{ users: { id: string; email: string; full_name_th: string | null }[] }>("api/v1/users")
      .then((r) => setUsers(r.users))
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    if (!editing || editing === "new") {
      setForm(EMPTY);
      return;
    }
    setForm({
      employee_code: editing.employee_code,
      first_name_th: editing.first_name_th,
      last_name_th: editing.last_name_th,
      position_title: editing.position_title,
      department: editing.department ?? "",
      base_salary: editing.base_salary,
      hire_date: editing.hire_date,
      pf_member: editing.pf_member,
      sso_enrolled: editing.sso_enrolled,
      bank_name: editing.bank_name ?? "",
      // Leave sensitive fields empty: empty means "keep the stored value".
      bank_account_number: "",
      national_id: "",
      sso_number: "",
      user_id: editing.user_id ?? "",
    });
  }, [editing]);

  function set<K extends keyof FormState>(key: K, value: FormState[K]) {
    setForm((f) => ({ ...f, [key]: value }));
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const body = {
        ...form,
        department: form.department.trim() || null,
        bank_name: form.bank_name.trim() || null,
        bank_account_number: form.bank_account_number.trim() || null,
        national_id: form.national_id.trim() || null,
        sso_number: form.sso_number.trim() || null,
        user_id: form.user_id || null,
      };
      if (editing === "new") {
        await gw("api/v1/employees", { method: "POST", body: JSON.stringify(body) });
        setNotice("เพิ่มพนักงานแล้ว");
      } else if (editing) {
        await gw(`api/v1/employees/${editing.id}`, {
          method: "PUT",
          body: JSON.stringify(body),
        });
        setNotice("บันทึกการแก้ไขแล้ว");
      }
      setEditing(null);
      await load(showTerminated);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function onImportFile(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    setImportResult(null);
    setImportRows([]);
    setError("");
    if (!file) return;
    try {
      setImportRows(csvToImportRows(await file.text()));
    } catch (err) {
      setError(err instanceof Error ? err.message : "อ่านไฟล์ไม่สำเร็จ");
    }
  }

  async function runImport(dryRun: boolean) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const r = await gw<ImportResult>("api/v1/employees/import", {
        method: "POST",
        body: JSON.stringify({ dry_run: dryRun, rows: importRows }),
      });
      if (dryRun) {
        setImportResult(r);
      } else {
        setNotice(`นำเข้าพนักงาน ${r.created} รายการแล้ว`);
        setImportRows([]);
        setImportResult(null);
        setImportKey((k) => k + 1);
        await load(showTerminated);
      }
    } catch (err) {
      handleApiError(err, setError);
    } finally {
      setBusy(false);
    }
  }

  async function terminate(employee: Employee) {
    if (!confirm(`ยืนยันการปิดสถานะพนักงาน ${employee.employee_code}? ประวัติเงินเดือนจะยังอยู่`)) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/employees/${employee.id}`, { method: "DELETE" });
      setNotice("ปิดสถานะพนักงานแล้ว");
      await load(showTerminated);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  const filtered = (rows ?? []).filter((e) => {
    const q = query.trim().toLowerCase();
    if (!q) return true;
    return [e.employee_code, e.first_name_th, e.last_name_th, e.position_title, e.department ?? ""]
      .join(" ")
      .toLowerCase()
      .includes(q);
  });

  return (
    <>
      <div className="page-head">
        <div>
          <h1>พนักงาน</h1>
          <p className="page-sub">ข้อมูลพนักงานในองค์กรของคุณ</p>
        </div>
        <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
          <label className="check">
            <input
              type="checkbox"
              checked={showTerminated}
              onChange={(e) => setShowTerminated(e.target.checked)}
            />
            รวมที่พ้นสภาพ
          </label>
          <input
            style={{ height: 34, width: 220, border: "1px solid var(--border)", borderRadius: 6, padding: "0 10px" }}
            placeholder="ค้นหารหัส ชื่อ หรือตำแหน่ง"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <button className="btn btn-primary" data-testid="add-employee" onClick={() => setEditing("new")}>
            เพิ่มพนักงาน
          </button>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      {editing && (
        <form className="card card-pad" onSubmit={submit} style={{ display: "grid", gap: 14 }}>
          <h2>{editing === "new" ? "เพิ่มพนักงาน" : `แก้ไข ${editing.employee_code}`}</h2>
          <div className="form-row">
            <div className="field">
              <label>รหัสพนักงาน</label>
              <input data-testid="employee-code" value={form.employee_code} onChange={(e) => set("employee_code", e.target.value)} required />
            </div>
            <div className="field">
              <label>ชื่อ (ไทย)</label>
              <input data-testid="employee-first-name" value={form.first_name_th} onChange={(e) => set("first_name_th", e.target.value)} required />
            </div>
            <div className="field">
              <label>นามสกุล (ไทย)</label>
              <input data-testid="employee-last-name" value={form.last_name_th} onChange={(e) => set("last_name_th", e.target.value)} required />
            </div>
          </div>
          <div className="form-row">
            <div className="field">
              <label>ตำแหน่ง</label>
              <input data-testid="employee-position" value={form.position_title} onChange={(e) => set("position_title", e.target.value)} required />
            </div>
            <div className="field">
              <label>แผนก</label>
              <input value={form.department} onChange={(e) => set("department", e.target.value)} />
            </div>
            <div className="field">
              <label>เงินเดือน (บาท)</label>
              <input
                type="number"
                min="0"
                step="0.01"
                data-testid="employee-salary"
                value={form.base_salary}
                onChange={(e) => set("base_salary", e.target.value)}
                required
              />
            </div>
          </div>
          <div className="form-row">
            <div className="field">
              <label>วันเริ่มงาน</label>
              <input data-testid="employee-hire-date" type="date" value={form.hire_date} onChange={(e) => set("hire_date", e.target.value)} required />
            </div>
            <div className="field">
              <label>ธนาคาร</label>
              <input value={form.bank_name} onChange={(e) => set("bank_name", e.target.value)} />
            </div>
            <div className="field">
              <label>เลขบัญชี</label>
              <input
                value={form.bank_account_number}
                placeholder={editing !== "new" && editing ? (editing.bank_account_number_masked ?? "") : ""}
                onChange={(e) => set("bank_account_number", e.target.value)}
              />
              <span className="hint">
                {editing !== "new" && editing?.bank_account_number_masked
                  ? "เว้นว่างเพื่อคงเลขบัญชีเดิม (แสดงเฉพาะ 4 ตัวท้าย)"
                  : "เข้ารหัสก่อนบันทึก"}
              </span>
            </div>
          </div>
          <div className="form-row">
            <div className="field">
              <label>เลขบัตรประชาชน</label>
              <input
                value={form.national_id}
                placeholder={editing !== "new" && editing ? (editing.national_id_masked ?? "") : ""}
                onChange={(e) => set("national_id", e.target.value)}
              />
              <span className="hint">เข้ารหัสก่อนบันทึก · แก้แล้วแสดงเฉพาะ 4 ตัวท้าย</span>
            </div>
            <div className="field">
              <label>เลขประกันสังคม</label>
              <input
                value={form.sso_number}
                placeholder={editing !== "new" && editing ? (editing.sso_number_masked ?? "") : ""}
                onChange={(e) => set("sso_number", e.target.value)}
              />
              <span className="hint">เว้นว่างเพื่อคงเดิม</span>
            </div>
            <div className="field">
              <label>ผูกบัญชีผู้ใช้</label>
              <select
                data-testid="employee-user"
                value={form.user_id}
                onChange={(e) => set("user_id", e.target.value)}
              >
                <option value="">ไม่ผูก</option>
                {users.map((u) => (
                  <option key={u.id} value={u.id}>
                    {u.email}
                    {u.full_name_th ? ` · ${u.full_name_th}` : ""}
                  </option>
                ))}
              </select>
              <span className="hint">ให้พนักงานดูสลิปของตัวเองได้</span>
            </div>
          </div>
          <div style={{ display: "flex", gap: 20, flexWrap: "wrap" }}>
            <label className="check">
              <input
                type="checkbox"
                checked={form.sso_enrolled}
                onChange={(e) => set("sso_enrolled", e.target.checked)}
              />
              อยู่ในระบบประกันสังคม (มาตรา 33)
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={form.pf_member}
                onChange={(e) => set("pf_member", e.target.checked)}
              />
              เป็นสมาชิกกองทุนสำรองเลี้ยงชีพ
            </label>
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <button className="btn btn-primary" data-testid="save-employee" type="submit" disabled={busy}>
              {busy ? "กำลังบันทึก…" : "บันทึก"}
            </button>
            <button className="btn" type="button" onClick={() => setEditing(null)}>
              ยกเลิก
            </button>
          </div>
        </form>
      )}

      <div className="card card-pad" style={{ display: "grid", gap: 12 }}>
        <h2>นำเข้าพนักงานจากไฟล์ CSV</h2>
        <p className="hint">
          คอลัมน์ที่จำเป็น: {IMPORT_REQUIRED.join(", ")} · ไม่บังคับ: {IMPORT_OPTIONAL.join(", ")}
        </p>
        <input key={importKey} type="file" accept=".csv" data-testid="import-csv" onChange={onImportFile} />
        {importRows.length > 0 && (
          <>
            <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
              <button className="btn" data-testid="dry-run-import" onClick={() => runImport(true)} disabled={busy}>
                ตรวจสอบไฟล์
              </button>
              <button
                className="btn btn-primary"
                data-testid="confirm-import"
                onClick={() => runImport(false)}
                disabled={busy || !importResult || importResult.valid === 0 || importResult.invalid > 0}
              >
                ยืนยันนำเข้า
              </button>
              {importResult && (
                <span data-testid="import-summary">
                  ทั้งหมด {importResult.total} · ถูกต้อง {importResult.valid} · ผิดพลาด {importResult.invalid}
                </span>
              )}
            </div>
            <table className="table">
              <thead>
                <tr>
                  <th>แถว</th>
                  <th>รหัส</th>
                  <th>ชื่อ</th>
                  <th>ตำแหน่ง</th>
                  <th className="num">เงินเดือน</th>
                  <th>วันเริ่มงาน</th>
                  <th>สถานะ</th>
                </tr>
              </thead>
              <tbody>
                {importRows.map((r, i) => {
                  const res = importResult?.rows[i];
                  return (
                    <tr key={i} data-testid="import-row">
                      <td>{i + 1}</td>
                      <td>{String(r.employee_code ?? "")}</td>
                      <td>
                        {String(r.first_name_th ?? "")} {String(r.last_name_th ?? "")}
                      </td>
                      <td>{String(r.position_title ?? "")}</td>
                      <td className="num">{String(r.base_salary ?? "")}</td>
                      <td>{String(r.hire_date ?? "")}</td>
                      <td>
                        {res &&
                          (res.status === "valid" ? (
                            <span className="badge success">ถูกต้อง</span>
                          ) : (
                            <span className="error">{res.errors.join(" · ")}</span>
                          ))}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </>
        )}
      </div>

      <div className="card">
        {rows === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : filtered.length === 0 ? (
          <div className="empty">ไม่พบพนักงาน</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>รหัส</th>
                <th>ชื่อ</th>
                <th>ตำแหน่ง</th>
                <th>แผนก</th>
                <th className="num">เงินเดือน</th>
                <th>สถานะ</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {filtered.map((e) => (
                <tr key={e.id}>
                  <td>{e.employee_code}</td>
                  <td>
                    {e.first_name_th} {e.last_name_th}
                  </td>
                  <td>{e.position_title}</td>
                  <td>{e.department ?? "—"}</td>
                  <td className="num">฿{thb(e.base_salary)}</td>
                  <td>
                    <span className={`badge ${e.status === "active" ? "success" : e.status === "terminated" ? "danger" : ""}`}>
                      {e.status === "active" ? "ทำงานอยู่" : e.status === "terminated" ? "พ้นสภาพ" : e.status}
                    </span>
                  </td>
                  <td className="num" style={{ whiteSpace: "nowrap" }}>
                    <button className="btn btn-sm" onClick={() => setEditing(e)}>
                      แก้ไข
                    </button>{" "}
                    {e.status !== "terminated" && (
                      <button className="btn btn-sm btn-danger" onClick={() => terminate(e)} disabled={busy}>
                        ปิดสถานะ
                      </button>
                    )}
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

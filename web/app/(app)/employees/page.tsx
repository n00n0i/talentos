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
  bank_account_number: string | null;
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
};

export default function EmployeesPage() {
  const [rows, setRows] = useState<Employee[] | null>(null);
  const [query, setQuery] = useState("");
  const [showTerminated, setShowTerminated] = useState(false);
  const [editing, setEditing] = useState<Employee | "new" | null>(null);
  const [form, setForm] = useState<FormState>(EMPTY);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

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
      bank_account_number: editing.bank_account_number ?? "",
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
              <input value={form.bank_account_number} onChange={(e) => set("bank_account_number", e.target.value)} />
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

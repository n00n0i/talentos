"use client";

import { useCallback, useEffect, useState } from "react";
import { ApiError, gw, handleApiError, Me, thDate } from "@/lib/api";

type Employee = {
  id: string;
  employee_code: string;
  first_name_th: string;
  last_name_th: string;
  status: string;
};

type Document = {
  id: string;
  employee_id: string;
  doc_type: string;
  title: string;
  doc_number: string | null;
  issued_date: string | null;
  expiry_date: string | null;
  days_left: number | null;
  has_file: boolean;
};

type ExpiringDocument = Document & {
  employee_code: string;
  employee_name: string;
};

const DOC_TYPES: Record<string, string> = {
  contract: "สัญญาจ้าง",
  work_permit: "ใบอนุญาตทำงาน",
  visa: "วีซ่า",
  id_card: "บัตรประชาชน",
  certificate: "ใบรับรอง",
  other: "อื่น ๆ",
};

const emptyForm = {
  doc_type: "contract",
  title: "",
  doc_number: "",
  issued_date: "",
  expiry_date: "",
};

function expiryBadge(days: number | null) {
  if (days === null) return null;
  if (days < 0) return <span className="badge danger">หมดอายุแล้ว {Math.abs(days)} วัน</span>;
  if (days === 0) return <span className="badge danger">หมดอายุวันนี้</span>;
  if (days <= 60) return <span className="badge warning">เหลือ {days} วัน</span>;
  return <span className="badge success">เหลือ {days} วัน</span>;
}

export default function DocumentsPage() {
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [employeeId, setEmployeeId] = useState("");
  const [docs, setDocs] = useState<Document[]>([]);
  const [expiring, setExpiring] = useState<ExpiringDocument[]>([]);
  const [form, setForm] = useState(emptyForm);
  const [file, setFile] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);
  const [canWrite, setCanWrite] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const loadExpiring = useCallback(async () => {
    const res = await gw<{ documents: ExpiringDocument[] }>("api/v1/documents/expiring?days=60");
    setExpiring(res.documents);
  }, []);

  const loadDocs = useCallback(async (id: string) => {
    if (!id) {
      setDocs([]);
      return;
    }
    const res = await gw<{ documents: Document[] }>(`api/v1/employees/${id}/documents`);
    setDocs(res.documents);
  }, []);

  useEffect(() => {
    gw<{ employees: Employee[] }>("api/v1/employees?status=all&limit=500")
      .then((res) => setEmployees(res.employees))
      .catch((e) => handleApiError(e, setError));
    gw<Me>("api/v1/auth/me")
      .then((me) => setCanWrite(me.permissions.includes("employee.update")))
      .catch((e) => handleApiError(e, setError));
    loadExpiring().catch((e) => handleApiError(e, setError));
  }, [loadExpiring]);

  useEffect(() => {
    loadDocs(employeeId).catch((e) => handleApiError(e, setError));
  }, [employeeId, loadDocs]);

  const set = (key: keyof typeof form, value: string) =>
    setForm((current) => ({ ...current, [key]: value }));

  async function save(event: React.FormEvent) {
    event.preventDefault();
    if (!employeeId) return;
    if (file && file.size > 5 * 1024 * 1024) {
      setError("ไฟล์ใหญ่เกิน 5 MB");
      return;
    }
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const body = {
        doc_type: form.doc_type,
        title: form.title,
        doc_number: form.doc_number.trim() || null,
        issued_date: form.issued_date || null,
        expiry_date: form.expiry_date || null,
      };
      const created = await gw<{ document: Document }>(
        `api/v1/employees/${employeeId}/documents`,
        { method: "POST", body: JSON.stringify(body) },
      );
      setForm(emptyForm);
      setFile(null);
      const uploaded = file ? await uploadFile(created.document.id, file) : true;
      await Promise.all([loadDocs(employeeId), loadExpiring()]);
      setNotice(
        uploaded
          ? "เพิ่มเอกสารแล้ว"
          : "สร้างเอกสารแล้ว แต่ไฟล์อัปโหลดไม่สำเร็จ — อัปโหลดใหม่ที่แถวเอกสาร",
      );
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function uploadFile(id: string, file: File) {
    if (file.size > 5 * 1024 * 1024) {
      setError("ไฟล์ใหญ่เกิน 5 MB");
      return false;
    }
    try {
      const res = await fetch(`/api/gw/api/v1/documents/${id}/file`, {
        method: "POST",
        headers: { "content-type": file.type },
        body: file,
      });
      if (!res.ok) {
        setError(`อัปโหลดไฟล์ไม่สำเร็จ (${res.status})`);
        return false;
      }
      return true;
    } catch {
      setError("อัปโหลดไฟล์ไม่สำเร็จ — ตรวจสอบการเชื่อมต่อ");
      return false;
    }
  }

  async function attach(id: string, file: File | undefined) {
    if (!file) return;
    setBusy(true);
    setError("");
    if (await uploadFile(id, file)) {
      await Promise.all([loadDocs(employeeId), loadExpiring()]);
      setNotice("อัปโหลดไฟล์แล้ว");
    }
    setBusy(false);
  }

  async function remove(id: string) {
    if (!confirm("ลบเอกสารนี้หรือไม่? ไฟล์แนบจะถูกลบด้วย")) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/documents/${id}`, { method: "DELETE" });
      await Promise.all([loadDocs(employeeId), loadExpiring()]);
      setNotice("ลบเอกสารแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <div className="page-head">
        <div>
          <h1>เอกสารพนักงาน</h1>
          <p className="page-sub">สัญญา ใบอนุญาตทำงาน วีซ่า และเอกสารที่ต้องต่ออายุ</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="success">{notice}</p>}

      <div className="card card-pad" style={{ marginBottom: 12 }}>
        <div className="card-head">
          <h2 style={{ fontSize: 16 }}>ใกล้หมดอายุ / หมดอายุแล้ว (60 วัน)</h2>
        </div>
        {expiring.length === 0 ? (
          <div className="empty" data-testid="expiring-empty">
            ไม่มีเอกสารที่ใกล้หมดอายุ
          </div>
        ) : (
          <ul className="list" data-testid="expiring-list">
            {expiring.map((d) => (
              <li key={d.id}>
                <span style={{ marginRight: 8 }}>
                  {d.employee_name} ({d.employee_code}) · {d.title}
                </span>
                {expiryBadge(d.days_left)}
                {d.expiry_date && (
                  <span style={{ marginLeft: 8, color: "var(--muted)" }}>
                    หมดอายุ {thDate(d.expiry_date)}
                  </span>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="card card-pad" style={{ marginBottom: 12 }}>
        <div className="grid-2" style={{ gridTemplateColumns: "1fr 2fr", gap: 12 }}>
          <label>
            พนักงาน
            <select
              data-testid="doc-employee"
              value={employeeId}
              onChange={(e) => setEmployeeId(e.target.value)}
            >
              <option value="">— เลือกพนักงาน —</option>
              {employees.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.employee_code} · {e.first_name_th} {e.last_name_th}
                </option>
              ))}
            </select>
          </label>
        </div>

        {canWrite ? (
        <form onSubmit={save} style={{ marginTop: 12, display: "grid", gap: 10 }}>
          <div className="grid-2" style={{ gridTemplateColumns: "1fr 2fr", gap: 12 }}>
            <label>
              ประเภท
              <select
                data-testid="doc-type"
                value={form.doc_type}
                onChange={(e) => set("doc_type", e.target.value)}
              >
                {Object.entries(DOC_TYPES).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            <label>
              ชื่อเอกสาร
              <input
                data-testid="doc-title"
                value={form.title}
                onChange={(e) => set("title", e.target.value)}
                required
              />
            </label>
          </div>
          <div className="grid-2" style={{ gridTemplateColumns: "1fr 1fr 1fr", gap: 12 }}>
            <label>
              เลขที่เอกสาร
              <input
                data-testid="doc-number"
                value={form.doc_number}
                onChange={(e) => set("doc_number", e.target.value)}
              />
            </label>
            <label>
              วันที่ออก
              <input
                data-testid="doc-issued"
                type="date"
                value={form.issued_date}
                onChange={(e) => set("issued_date", e.target.value)}
              />
            </label>
            <label>
              วันหมดอายุ
              <input
                data-testid="doc-expiry"
                type="date"
                value={form.expiry_date}
                onChange={(e) => set("expiry_date", e.target.value)}
              />
            </label>
          </div>
          <label>
            ไฟล์แนบ (PDF, JPEG, PNG ≤ 5 MB)
            <input
              data-testid="doc-file"
              type="file"
              accept="application/pdf,image/jpeg,image/png"
              onChange={(e) => setFile(e.target.files?.[0] ?? null)}
            />
          </label>
          <button
            className="btn btn-primary"
            data-testid="save-document"
            type="submit"
            disabled={busy || !employeeId}
          >
            เพิ่มเอกสาร
          </button>
        </form>
        ) : (
          <p className="empty" style={{ marginTop: 12 }}>บัญชีนี้ดูเอกสารได้อย่างเดียว</p>
        )}
      </div>

      <div className="card">
        {docs.length === 0 ? (
          <div className="empty">ยังไม่มีเอกสารของพนักงานคนนี้</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>ประเภท</th>
                <th>ชื่อเอกสาร</th>
                <th>เลขที่</th>
                <th>วันหมดอายุ</th>
                <th>สถานะ</th>
                <th>ไฟล์</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {docs.map((d) => (
                <tr key={d.id} data-testid="document-row">
                  <td>{DOC_TYPES[d.doc_type] ?? d.doc_type}</td>
                  <td>{d.title}</td>
                  <td>{d.doc_number ?? "—"}</td>
                  <td>{d.expiry_date ? thDate(d.expiry_date) : "—"}</td>
                  <td>{expiryBadge(d.days_left)}</td>
                  <td>
                    {d.has_file ? (
                      <a
                        className="badge accent"
                        data-testid="doc-download"
                        href={`/api/gw/api/v1/documents/${d.id}/file`}
                        target="_blank"
                        rel="noreferrer"
                        style={{ marginRight: 6 }}
                      >
                        ดาวน์โหลด
                      </a>
                    ) : (
                      "—"
                    )}
                    {canWrite && (
                      <label className="badge" style={{ cursor: "pointer" }}>
                        อัปโหลด
                        <input
                          data-testid="doc-upload"
                          type="file"
                          accept="application/pdf,image/jpeg,image/png"
                          disabled={busy}
                          style={{ display: "none" }}
                          onChange={(e) => attach(d.id, e.target.files?.[0])}
                        />
                      </label>
                    )}
                  </td>
                  <td>
                    {canWrite && (
                      <button
                        className="btn btn-sm"
                        data-testid="delete-document"
                        disabled={busy}
                        onClick={() => remove(d.id)}
                      >
                        ลบ
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

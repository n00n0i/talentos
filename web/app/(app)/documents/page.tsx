"use client";

import type { TranslationKey } from "@/lib/i18n-catalog";

import { useI18N, tNow } from "@/lib/i18n";

import ThaiDateInput from "@/components/ThaiDateInput";

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

const DOC_TYPES: Record<string, TranslationKey> = {
  contract: "documents.employment-contract",
  work_permit: "documents.work-permit",
  visa: "documents.visa",
  id_card: "documents.id-card",
  certificate: "documents.certificate",
  other: "documents.other",
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
  if (days < 0) return <span className="badge danger">{tNow("documents.text")} {Math.abs(days)} {tNow("documents.day")}</span>;
  if (days === 0) return <span className="badge danger">{tNow("documents.expires-today")}</span>;
  if (days <= 60) return <span className="badge warning">{tNow("documents.remaining")} {days} {tNow("documents.day")}</span>;
  return <span className="badge success">{tNow("documents.remaining")} {days} {tNow("documents.day")}</span>;
}

export default function DocumentsPage() {
  const { t: tr } = useI18N();
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
      setError(tr("documents.file-larger-than-5-mb"));
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
          ? tr("documents.document-added")
          : tr("documents.document-created-but-the-file-failed"),
      );
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function uploadFile(id: string, file: File) {
    if (file.size > 5 * 1024 * 1024) {
      setError(tr("documents.file-larger-than-5-mb"));
      return false;
    }
    try {
      const res = await fetch(`/api/gw/api/v1/documents/${id}/file`, {
        method: "POST",
        headers: { "content-type": file.type },
        body: file,
      });
      if (!res.ok) {
        setError(tr("documents.uploadError", { status: res.status }));
        return false;
      }
      return true;
    } catch {
      setError(tr("documents.failed-to-upload-file-check-connection"));
      return false;
    }
  }

  async function attach(id: string, file: File | undefined) {
    if (!file) return;
    setBusy(true);
    setError("");
    if (await uploadFile(id, file)) {
      await Promise.all([loadDocs(employeeId), loadExpiring()]);
      setNotice(tr("documents.file-uploaded"));
    }
    setBusy(false);
  }

  async function remove(id: string) {
    if (!confirm(tr("documents.delete-this-document-attachments-will-also"))) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/documents/${id}`, { method: "DELETE" });
      await Promise.all([loadDocs(employeeId), loadExpiring()]);
      setNotice(tr("documents.document-deleted"));
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
          <h1>{tr("documents.employee-documents")}</h1>
          <p className="page-sub">{tr("documents.contracts-work-permits-visas-and-documents")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="success" role="status" aria-live="polite">{notice}</p>}

      <div className="card card-pad" style={{ marginBottom: 12 }}>
        <div className="card-head">
          <h2 style={{ fontSize: 16 }}>{tr("documents.expiring-expired-60-days")}</h2>
        </div>
        {expiring.length === 0 ? (
          <div className="empty" data-testid="expiring-empty">
            {tr("documents.there-are-no-documents-about-to")}
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
                    {tr("documents.expiry")} {thDate(d.expiry_date)}
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
            {tr("documents.employee")}
            <select
              data-testid="doc-employee"
              value={employeeId}
              onChange={(e) => setEmployeeId(e.target.value)}
            >
              <option value="">{tr("documents.select-employee")}</option>
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
              {tr("documents.type")}
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
              {tr("documents.document-name")}
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
              {tr("documents.document-number")}
              <input
                data-testid="doc-number"
                value={form.doc_number}
                onChange={(e) => set("doc_number", e.target.value)}
              />
            </label>
            <label>
              {tr("documents.issue-date")}
              <ThaiDateInput testId="doc-issued" value={form.issued_date} onChange={(v) => set("issued_date", v)} />
            </label>
            <label>
              {tr("documents.expiry-date")}
              <ThaiDateInput testId="doc-expiry" value={form.expiry_date} onChange={(v) => set("expiry_date", v)} />
            </label>
          </div>
          <label>
            {tr("documents.attached-files-pdf-jpeg-png-5")}
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
            {tr("documents.add-document")}
          </button>
        </form>
        ) : (
          <p className="empty" style={{ marginTop: 12 }}>{tr("documents.this-account-can-only-view-documents")}</p>
        )}
      </div>

      <div className="card">
        {docs.length === 0 ? (
          <div className="empty">{tr("documents.there-are-no-documents-for-this")}</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{tr("documents.type")}</th>
                <th scope="col">{tr("documents.document-name")}</th>
                <th scope="col">{tr("documents.number")}</th>
                <th scope="col">{tr("documents.expiry-date")}</th>
                <th scope="col">{tr("documents.status")}</th>
                <th scope="col">{tr("documents.file")}</th>
                <th scope="col" />
              </tr>
            </thead>
            <tbody>
              {docs.map((d) => (
                <tr key={d.id} data-testid="document-row">
                  <td>{d.doc_type in DOC_TYPES ? tr(DOC_TYPES[d.doc_type]) : d.doc_type}</td>
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
                        {tr("documents.download")}
                      </a>
                    ) : (
                      "—"
                    )}
                    {canWrite && (
                      <label className="badge" style={{ cursor: "pointer" }}>
                        {tr("documents.upload")}
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
                        {tr("documents.delete")}
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

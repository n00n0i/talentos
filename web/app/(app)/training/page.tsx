"use client";

import { useI18N, tNow } from "@/lib/i18n";

import ThaiDateInput from "@/components/ThaiDateInput";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, Me, thDate } from "@/lib/api";

type Employee = {
  id: string;
  employee_code: string;
  first_name_th: string;
  last_name_th: string;
};

type Training = {
  id: string;
  employee_id: string;
  title: string;
  provider: string | null;
  completed_on: string | null;
  expires_on: string | null;
  days_left: number | null;
};

type ExpiringTraining = Training & {
  employee_code: string;
  employee_name: string;
};

const emptyForm = { title: "", provider: "", completed_on: "", expires_on: "" };

function expiryBadge(days: number | null) {
  if (days === null) return <span className="badge">{tNow("training.no-expiration-date")}</span>;
  if (days < 0) return <span className="badge danger">{tNow("training.text")} {Math.abs(days)} {tNow("training.day")}</span>;
  if (days === 0) return <span className="badge danger">{tNow("training.expires-today")}</span>;
  if (days <= 90) return <span className="badge warning">{tNow("training.remaining")} {days} {tNow("training.day")}</span>;
  return <span className="badge success">{tNow("training.remaining")} {days} {tNow("training.day")}</span>;
}

export default function TrainingPage() {
  const { t: tr } = useI18N();
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [employeeId, setEmployeeId] = useState("");
  const [records, setRecords] = useState<Training[]>([]);
  const [expiring, setExpiring] = useState<ExpiringTraining[]>([]);
  const [form, setForm] = useState(emptyForm);
  const [canWrite, setCanWrite] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const loadExpiring = useCallback(async () => {
    const res = await gw<{ training: ExpiringTraining[] }>("api/v1/training/expiring?days=90");
    setExpiring(res.training);
  }, []);

  const loadRecords = useCallback(async (id: string) => {
    if (!id) {
      setRecords([]);
      return;
    }
    const res = await gw<{ training: Training[] }>(`api/v1/employees/${id}/training`);
    setRecords(res.training);
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
    loadRecords(employeeId).catch((e) => handleApiError(e, setError));
  }, [employeeId, loadRecords]);

  async function save(event: React.FormEvent) {
    event.preventDefault();
    if (!employeeId) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw(`api/v1/employees/${employeeId}/training`, {
        method: "POST",
        body: JSON.stringify({
          title: form.title,
          provider: form.provider.trim() || null,
          completed_on: form.completed_on || null,
          expires_on: form.expires_on || null,
        }),
      });
      setForm(emptyForm);
      await Promise.all([loadRecords(employeeId), loadExpiring()]);
      setNotice(tr("training.training-recorded"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function remove(id: string) {
    if (!confirm(tr("training.delete-this-training-history-or-not"))) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/training/${id}`, { method: "DELETE" });
      await Promise.all([loadRecords(employeeId), loadExpiring()]);
      setNotice(tr("training.training-history-has-been-deleted"));
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
          <h1>{tr("training.training-certificates")}</h1>
          <p className="page-sub">{tr("training.training-history-and-certificates-to-renew")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="success" role="status" aria-live="polite">{notice}</p>}

      <div className="card card-pad" style={{ marginBottom: 12 }}>
        <div className="card-head">
          <h2 style={{ fontSize: 16 }}>{tr("training.certificates-within-90-days-expired-within")}</h2>
        </div>
        {expiring.length === 0 ? (
          <div className="empty">{tr("training.there-are-no-certificates-about-to")}</div>
        ) : (
          <ul className="list" data-testid="training-expiring">
            {expiring.map((t) => (
              <li key={t.id}>
                <span style={{ marginRight: 8 }}>
                  {t.employee_name} ({t.employee_code}) · {t.title}
                </span>
                {expiryBadge(t.days_left)}
                {t.expires_on && (
                  <span style={{ marginLeft: 8, color: "var(--muted)" }}>
                    {tr("training.expiry")} {thDate(t.expires_on)}
                  </span>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="card card-pad" style={{ marginBottom: 12 }}>
        <label>
          {tr("training.employee")}
          <select
            data-testid="training-employee"
            value={employeeId}
            onChange={(e) => {
              setRecords([]);
              setEmployeeId(e.target.value);
            }}
          >
            <option value="">{tr("training.select-employee")}</option>
            {employees.map((e) => (
              <option key={e.id} value={e.id}>
                {e.employee_code} · {e.first_name_th} {e.last_name_th}
              </option>
            ))}
          </select>
        </label>

        {canWrite && (
          <form onSubmit={save} style={{ marginTop: 12, display: "grid", gap: 10 }}>
            <div className="grid-2" style={{ gridTemplateColumns: "2fr 1fr", gap: 12 }}>
              <label>
                {tr("training.course-topic")}
                <input
                  data-testid="training-title"
                  value={form.title}
                  onChange={(e) => setForm({ ...form, title: e.target.value })}
                  required
                />
              </label>
              <label>
                {tr("training.training-provider")}
                <input
                  data-testid="training-provider"
                  value={form.provider}
                  onChange={(e) => setForm({ ...form, provider: e.target.value })}
                />
              </label>
            </div>
            <div className="grid-2" style={{ gridTemplateColumns: "1fr 1fr", gap: 12 }}>
              <label>
                {tr("training.date-of-completion-of-training")}
                <ThaiDateInput testId="training-completed" value={form.completed_on} onChange={(v) => setForm({ ...form, completed_on: v })} />
              </label>
              <label>
                {tr("training.certificate-expiration-date")}
                <ThaiDateInput testId="training-expires" value={form.expires_on} onChange={(v) => setForm({ ...form, expires_on: v })} />
              </label>
            </div>
            <button
              className="btn btn-primary"
              data-testid="save-training"
              type="submit"
              disabled={busy || !employeeId}
            >
              {tr("training.training-record")}
            </button>
          </form>
        )}
      </div>

      <div className="card">
        {!employeeId ? (
          <div className="empty">{tr("training.select-an-employee-to-view-their")}</div>
        ) : records.length === 0 ? (
          <div className="empty">{tr("training.there-is-no-training-history-of")}</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{tr("training.course")}</th>
                <th scope="col">{tr("training.organizer")}</th>
                <th scope="col">{tr("training.when-training")}</th>
                <th scope="col">{tr("training.expiry")}</th>
                <th scope="col">{tr("training.status")}</th>
                {canWrite && <th scope="col" />}
              </tr>
            </thead>
            <tbody>
              {records.map((t) => (
                <tr key={t.id} data-testid="training-row">
                  <td>{t.title}</td>
                  <td>{t.provider ?? "—"}</td>
                  <td>{t.completed_on ? thDate(t.completed_on) : "—"}</td>
                  <td>{t.expires_on ? thDate(t.expires_on) : "—"}</td>
                  <td>{expiryBadge(t.days_left)}</td>
                  {canWrite && (
                    <td>
                      <button
                        className="btn btn-sm"
                        data-testid="delete-training"
                        disabled={busy}
                        onClick={() => remove(t.id)}
                      >
                        {tr("training.delete")}
                      </button>
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}

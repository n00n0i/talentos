"use client";

import type { TranslationKey } from "@/lib/i18n-catalog";

import { useI18N, tNow } from "@/lib/i18n";

import ThaiDateInput from "@/components/ThaiDateInput";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, Me, thDate } from "@/lib/api";

type OtRequest = {
  id: string;
  work_date: string;
  minutes: number;
  reason: string | null;
  status: string;
};
type Pending = OtRequest & {
  employee_code: string;
  employee_name: string;
  step_no: number;
  total_steps: number;
  approver_role: string;
};

const OT_ERRORS: Record<string, TranslationKey> = {
  "a request already exists for this work date": "ot.there-is-already-an-ot-request",
  "OT request is not in pending status": "ot.this-request-is-not-in-pending",
  "OT request cannot be changed in its current status": "ot.this-request-is-not-in-pending",
  "minutes must be between 1 and 720": "ot.the-number-of-hours-must-be",
  "reason must be at most 500 characters": "ot.the-reason-must-not-exceed-500",
  "OT request not found": "ot.ot-request-not-found",
  "not found": "ot.ot-request-not-found",
  "employee not found for this tenant": "ot.employee-information-not-found",
  "no active employee record for this account": "ot.no-active-employee-information-found-for",
};

const thOtError = (message: string, t: (key: TranslationKey) => string) =>
  OT_ERRORS[message] ? t(OT_ERRORS[message]) : message;

const STATUS: Record<string, { labelKey: TranslationKey; tone: string }> = {
  pending: { labelKey: "ot.pending-approval", tone: "warning" },
  approved: { labelKey: "ot.approved", tone: "success" },
  rejected: { labelKey: "ot.reject", tone: "danger" },
  cancelled: { labelKey: "ot.canceled", tone: "" },
};

const hours = (minutes: number) => String(minutes / 60);

const roleLabel = (role: string) => (role === "manager" ? tNow("ot.manager") : "HR");

export default function OtPage() {
  const { t: tr } = useI18N();
  const [isHr, setIsHr] = useState(false);
  const [linked, setLinked] = useState(true);
  const [requests, setRequests] = useState<OtRequest[] | null>(null);
  const [pending, setPending] = useState<Pending[]>([]);
  const [history, setHistory] = useState<Pending[]>([]);
  const [date, setDate] = useState("");
  const [hrs, setHrs] = useState("");
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    const me = await gw<Me>("api/v1/auth/me");
    const hr = me.permissions.includes("ot_request.view_team");
    const [mine, inbox, done] = await Promise.all([
      gw<{ employee_linked: boolean; requests: OtRequest[] }>("api/v1/me/ot"),
      hr ? gw<{ requests: Pending[] }>("api/v1/ot/approvals?status=pending") : Promise.resolve(null),
      hr ? gw<{ requests: Pending[] }>("api/v1/ot/approvals?status=approved&limit=50") : Promise.resolve(null),
    ]);
    setIsHr(hr);
    setLinked(mine.employee_linked);
    setRequests(mine.requests);
    setPending(inbox?.requests ?? []);
    setHistory(done?.requests ?? []);
  }, []);

  useEffect(() => {
    load().catch((e) => handleApiError(e, (m) => setError(thOtError(m, tr))));
  }, [load]);

  async function act(done: string, path: string, body?: object) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw(path, { method: "POST", body: body && JSON.stringify(body) });
      setNotice(done);
      await load();
      return true;
    } catch (e) {
      handleApiError(e, (m) => setError(thOtError(m, tr)));
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    const ok = await act(tr("ot.ot-request-sent"), "api/v1/me/ot", {
      work_date: date,
      minutes: Math.round(Number(hrs) * 60),
      reason: reason || undefined,
    });
    if (ok) {
      setDate("");
      setHrs("");
      setReason("");
    }
  }

  const badge = (status: string) => {
    const st = STATUS[status];
    return <span className={`badge ${st?.tone ?? ""}`}>{st ? tr(st.labelKey) : status}</span>;
  };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{tr("ot.request-ot")}</h1>
          <p className="page-sub">
            {tr("ot.approved-ot-is-just-a-record")}
          </p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="badge success" role="status" aria-live="polite">{notice}</p>}

      {isHr && (
        <div className="card">
          <div className="card-head">
            <h2>{tr("ot.waiting-for-ot-approval")}</h2>
            <span className="hint">{pending.length} {tr("ot.items")}</span>
          </div>
          {pending.length === 0 ? (
            <div className="empty">{tr("ot.there-are-no-ot-requests-pending")}</div>
          ) : (
            <div style={{ overflowX: "auto" }}>
              <table className="table">
                <thead>
                  <tr>
                    <th scope="col">{tr("ot.employee")}</th>
                    <th scope="col">{tr("ot.date")}</th>
                    <th scope="col" className="num">{tr("ot.hours")}</th>
                    <th scope="col">{tr("ot.reason")}</th>
                    <th scope="col" />
                  </tr>
                </thead>
                <tbody>
                  {pending.map((r) => (
                    <tr key={r.id} data-testid="ot-approval-row">
                      <td>
                        {r.employee_code} · {r.employee_name}
                        <span className="hint" data-testid="ot-step">
                          {tr("ot.step")} {r.step_no}/{r.total_steps} · {roleLabel(r.approver_role)}
                        </span>
                      </td>
                      <td>{thDate(r.work_date)}</td>
                      <td className="num">{hours(r.minutes)}</td>
                      <td>{r.reason ?? "—"}</td>
                      <td className="num">
                        <span style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                          <button
                            className="btn btn-sm btn-danger"
                            data-testid="reject-ot"
                            disabled={busy}
                            onClick={() =>
                              act(tr("ot.ot-request-not-approved"), `api/v1/ot/requests/${r.id}/decision`, { approve: false })
                            }
                          >
                            {tr("ot.reject")}
                          </button>
                          <button
                            className="btn btn-sm btn-primary"
                            data-testid="approve-ot"
                            disabled={busy}
                            onClick={() =>
                              act(tr("ot.ot-request-approved"), `api/v1/ot/requests/${r.id}/decision`, { approve: true })
                            }
                          >
                            {tr("ot.approve")}
                          </button>
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {isHr && (
        <div className="card">
          <div className="card-head">
            <h2>{tr("ot.approved-ot-history")}</h2>
            <span className="hint">{tr("ot.latest")} {history.length} {tr("ot.items")}</span>
          </div>
          {history.length === 0 ? (
            <div className="empty">{tr("ot.there-is-no-approved-ot-yet")}</div>
          ) : (
            <div style={{ overflowX: "auto" }}>
              <table className="table">
                <thead>
                  <tr>
                    <th scope="col">{tr("ot.employee")}</th>
                    <th scope="col">{tr("ot.date")}</th>
                    <th scope="col" className="num">{tr("ot.hours")}</th>
                    <th scope="col">{tr("ot.reason")}</th>
                    <th scope="col">{tr("ot.status")}</th>
                  </tr>
                </thead>
                <tbody>
                  {history.map((r) => (
                    <tr key={r.id} data-testid="ot-history-row">
                      <td>
                        {r.employee_code} · {r.employee_name}
                      </td>
                      <td>{thDate(r.work_date)}</td>
                      <td className="num">{hours(r.minutes)}</td>
                      <td>{r.reason ?? "—"}</td>
                      <td>{badge(r.status)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {requests === null ? (
        <div className="spinner">{tr("ot.loading")}</div>
      ) : !linked ? (
        <div className="card">
          <div className="empty">
            {tr("ot.this-account-has-not-been-linked")}
          </div>
        </div>
      ) : (
        <>
          <form className="card card-pad" onSubmit={submit} style={{ display: "grid", gap: 12 }}>
            <h2>{tr("ot.submit-an-ot-request")}</h2>
            <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
              <div className="field">
                <label htmlFor="ot-date">{tr("ot.date-of-doing-ot")}</label>
                <ThaiDateInput id="ot-date" testId="ot-date" value={date} onChange={(v) => setDate(v)} required />
              </div>
              <div className="field">
                <label htmlFor="ot-hours">{tr("ot.hours")}</label>
                <input
                  id="ot-hours"
                  data-testid="ot-hours"
                  type="number"
                  step={0.5}
                  min={0.5}
                  max={12}
                  value={hrs}
                  onChange={(e) => setHrs(e.target.value)}
                  required
                />
              </div>
            </div>
            <div className="field">
              <label htmlFor="ot-reason">{tr("ot.reason")}</label>
              <input id="ot-reason" data-testid="ot-reason" maxLength={500} value={reason} onChange={(e) => setReason(e.target.value)} />
              <span className="hint">{tr("ot.max-500-characters")}</span>
            </div>
            <div>
              <button className="btn btn-primary btn-sm" data-testid="request-ot" type="submit" disabled={busy}>
                {tr("ot.submit-request")}
              </button>
            </div>
          </form>

          <div className="card">
            <div className="card-head">
              <h2>{tr("ot.ot-request-history")}</h2>
            </div>
            {requests.length === 0 ? (
              <div className="empty">{tr("ot.there-are-no-ot-requests-yet")}</div>
            ) : (
              <div style={{ overflowX: "auto" }}>
                <table className="table">
                  <thead>
                    <tr>
                      <th scope="col">{tr("ot.date")}</th>
                      <th scope="col" className="num">{tr("ot.hours")}</th>
                      <th scope="col">{tr("ot.reason")}</th>
                      <th scope="col">{tr("ot.status")}</th>
                      <th scope="col" />
                    </tr>
                  </thead>
                  <tbody>
                    {requests.map((r) => (
                      <tr key={r.id} data-testid="ot-row">
                        <td>{thDate(r.work_date)}</td>
                        <td className="num">{hours(r.minutes)}</td>
                        <td>{r.reason ?? "—"}</td>
                        <td>{badge(r.status)}</td>
                        <td className="num">
                          {r.status === "pending" && (
                            <button
                              className="btn btn-sm"
                              data-testid="cancel-ot"
                              disabled={busy}
                              onClick={() => act(tr("ot.ot-request-canceled"), `api/v1/me/ot/${r.id}/cancel`)}
                            >
                              {tr("ot.cancel")}
                            </button>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        </>
      )}
    </>
  );
}

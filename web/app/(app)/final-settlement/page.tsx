"use client";

import { useI18N } from "@/lib/i18n";

import ThaiDateInput from "@/components/ThaiDateInput";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, thb, thDate } from "@/lib/api";

type Employee = {
  id: string;
  employee_code: string;
  first_name_th: string;
  last_name_th: string;
};

type Settlement = {
  id: string;
  employee_id: string;
  employee_code: string;
  employee_name: string;
  termination_date: string;
  unused_leave_days: number;
  notice_days: number;
  include_severance: boolean;
  years_of_service: number;
  last_month_pay: string;
  leave_pay: string;
  notice_pay: string;
  severance_pay: string;
  total_gross: string;
  breakdown: {
    daily_rate: string;
    last_month_days: number;
    severance_days: number;
  };
  status: string;
  created_at: string;
};

export default function FinalSettlementPage() {
  const { t: tr } = useI18N();
  const [settlements, setSettlements] = useState<Settlement[] | null>(null);
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [employeeId, setEmployeeId] = useState("");
  const [terminationDate, setTerminationDate] = useState("");
  const [unusedLeaveDays, setUnusedLeaveDays] = useState("");
  const [noticeDays, setNoticeDays] = useState("");
  const [includeSeverance, setIncludeSeverance] = useState(true);
  const [latest, setLatest] = useState<Settlement | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    const r = await gw<{ settlements: Settlement[] }>("api/v1/final-settlements");
    setSettlements(r.settlements);
  }, []);

  useEffect(() => {
    load().catch((e) => handleApiError(e, setError));
  }, [load]);

  useEffect(() => {
    gw<{ employees: Employee[] }>("api/v1/employees?limit=500")
      .then((r) => setEmployees(r.employees))
      .catch((e) => handleApiError(e, setError));
  }, []);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!employeeId) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const created = await gw<Settlement>(`api/v1/employees/${employeeId}/final-settlement`, {
        method: "POST",
        body: JSON.stringify({
          termination_date: terminationDate,
          unused_leave_days: Number(unusedLeaveDays),
          notice_days: Number(noticeDays),
          include_severance: includeSeverance,
        }),
      });
      setLatest(created);
      setNotice(tr("finalsettlement.wages-have-been-calculated"));
      await load();
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
          <h1>{tr("finalsettlement.offboarding")}</h1>
          <p className="page-sub">{tr("finalsettlement.calculate-final-pay-salary-accrued-leave")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="badge success" role="status" aria-live="polite">{notice}</p>}

      <form className="card card-pad" onSubmit={submit} style={{ marginBottom: 16 }}>
        <h2 style={{ marginBottom: 14 }}>{tr("finalsettlement.calculate-wages")}</h2>
        <div className="form-row" style={{ alignItems: "end" }}>
          <div className="field">
            <label htmlFor="settlement-employee">{tr("finalsettlement.employee")}</label>
            <select
              id="settlement-employee"
              data-testid="settlement-employee"
              value={employeeId}
              onChange={(e) => setEmployeeId(e.target.value)}
              required
            >
              <option value="">{tr("finalsettlement.choose-employees")}</option>
              {employees.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.employee_code} · {e.first_name_th} {e.last_name_th}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor="settlement-date">{tr("finalsettlement.end-date-of-employment")}</label>
            <ThaiDateInput id="settlement-date" testId="settlement-date" value={terminationDate} onChange={(v) => setTerminationDate(v)} required />
          </div>
          <div className="field">
            <label htmlFor="settlement-unused">{tr("finalsettlement.accrued-leave-days")}</label>
            <input
              id="settlement-unused"
              data-testid="settlement-unused"
              type="number"
              min="0"
              step="1"
              value={unusedLeaveDays}
              onChange={(e) => setUnusedLeaveDays(e.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="settlement-notice">{tr("finalsettlement.advance-notice-date-day")}</label>
            <input
              id="settlement-notice"
              data-testid="settlement-notice"
              type="number"
              min="0"
              step="1"
              value={noticeDays}
              onChange={(e) => setNoticeDays(e.target.value)}
              required
            />
          </div>
          <div className="field" style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <input
              id="settlement-severance"
              data-testid="settlement-severance"
              type="checkbox"
              checked={includeSeverance}
              onChange={(e) => setIncludeSeverance(e.target.checked)}
            />
            <label htmlFor="settlement-severance" style={{ margin: 0 }}>
              {tr("finalsettlement.calculate-compensation")}
            </label>
          </div>
          <div style={{ display: "flex", alignItems: "end" }}>
            <button
              className="btn btn-primary"
              data-testid="compute-settlement"
              type="submit"
              disabled={busy}
            >
              {busy ? tr("finalsettlement.calculating") : tr("finalsettlement.calculate")}
            </button>
          </div>
        </div>
      </form>

      {latest && (
        <div className="card card-pad" style={{ marginBottom: 16 }}>
          <h2 style={{ marginBottom: 14 }}>{tr("finalsettlement.calculation-results")}</h2>
          <div className="grid" style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(160px, 1fr))", gap: 12, marginBottom: 16 }}>
            <div className="metric">
              <span className="metric-label">{tr("finalsettlement.rate-day")}</span>
              <span className="metric-value">{tr("finalsettlement.text")}{thb(latest.breakdown.daily_rate)}</span>
            </div>
            <div className="metric">
              <span className="metric-label">{tr("finalsettlement.text-2")}{latest.breakdown.last_month_days} {tr("finalsettlement.text-3")}</span>
              <span className="metric-value">{tr("finalsettlement.text")}{thb(latest.last_month_pay)}</span>
            </div>
            <div className="metric">
              <span className="metric-label">{tr("finalsettlement.accrued-leave-pay")}</span>
              <span className="metric-value">{tr("finalsettlement.text")}{thb(latest.leave_pay)}</span>
            </div>
            <div className="metric">
              <span className="metric-label">{tr("finalsettlement.notice-pay")}</span>
              <span className="metric-value">{tr("finalsettlement.text")}{thb(latest.notice_pay)}</span>
            </div>
            <div className="metric">
              <span className="metric-label">{tr("finalsettlement.text-4")}{latest.breakdown.severance_days} {tr("finalsettlement.text-3")}</span>
              <span className="metric-value">{tr("finalsettlement.text")}{thb(latest.severance_pay)}</span>
            </div>
            <div className="metric accent">
              <span className="metric-label">{tr("finalsettlement.total-before-tax")}</span>
              <span className="metric-value" data-testid="settlement-total">
                {tr("finalsettlement.text")}{thb(latest.total_gross)}
              </span>
            </div>
          </div>

          <table className="table">
            <thead>
              <tr>
                <th scope="col">{tr("finalsettlement.items")}</th>
                <th scope="col" className="num">{tr("finalsettlement.amount")}</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>{tr("finalsettlement.text-2")}{latest.breakdown.last_month_days} {tr("finalsettlement.text-3")}</td>
                <td className="num">{tr("finalsettlement.text")}{thb(latest.last_month_pay)}</td>
              </tr>
              <tr>
                <td>{tr("finalsettlement.text-5")}{latest.unused_leave_days} {tr("finalsettlement.text-3")}</td>
                <td className="num">{tr("finalsettlement.text")}{thb(latest.leave_pay)}</td>
              </tr>
              <tr>
                <td>{tr("finalsettlement.text-6")}{latest.notice_days} {tr("finalsettlement.text-3")}</td>
                <td className="num">{tr("finalsettlement.text")}{thb(latest.notice_pay)}</td>
              </tr>
              <tr>
                <td>{tr("finalsettlement.text-4")}{latest.breakdown.severance_days} {tr("finalsettlement.text-3")}</td>
                <td className="num">{tr("finalsettlement.text")}{thb(latest.severance_pay)}</td>
              </tr>
              <tr className="total">
                <td>{tr("finalsettlement.total-before-tax")}</td>
                <td className="num">{tr("finalsettlement.text")}{thb(latest.total_gross)}</td>
              </tr>
            </tbody>
          </table>
        </div>
      )}

      <div className="card">
        {settlements === null ? (
          <div className="spinner">{tr("finalsettlement.loading")}</div>
        ) : settlements.length === 0 ? (
          <div className="empty">{tr("finalsettlement.there-are-no-calculation-items")}</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{tr("finalsettlement.employee")}</th>
                <th scope="col">{tr("finalsettlement.end-date")}</th>
                <th scope="col" className="num">{tr("finalsettlement.accrued-leave")}</th>
                <th scope="col" className="num">{tr("finalsettlement.notify")}</th>
                <th scope="col" className="num">{tr("finalsettlement.compensation")}</th>
                <th scope="col" className="num">{tr("finalsettlement.total")}</th>
                <th scope="col">{tr("finalsettlement.status")}</th>
              </tr>
            </thead>
            <tbody>
              {settlements.map((s) => (
                <tr key={s.id} data-testid="settlement-row">
                  <td>
                    {s.employee_code} · {s.employee_name}
                  </td>
                  <td>{thDate(s.termination_date)}</td>
                  <td className="num">{s.unused_leave_days}</td>
                  <td className="num">{s.notice_days}</td>
                  <td className="num">{tr("finalsettlement.text")}{thb(s.severance_pay)}</td>
                  <td className="num">{tr("finalsettlement.text")}{thb(s.total_gross)}</td>
                  <td>
                    <span className="badge">{s.status === "draft" ? tr("finalsettlement.draft") : s.status}</span>
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

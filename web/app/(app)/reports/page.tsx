"use client";

import { useI18N } from "@/lib/i18n";

import ThaiDateInput from "@/components/ThaiDateInput";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, thb } from "@/lib/api";

type Headcount = {
  active: number;
  probation: number;
  leave: number;
  terminated_last_12m: number;
  by_department: { department: string; count: number }[];
};

type CostRow = {
  department: string;
  employees: number;
  gross: string;
  net: string;
  tax: string;
  sso: string;
  pf: string;
};

type OtRow = {
  month: string;
  ot_minutes: number;
  approved_days: number;
};

function yearRange() {
  const year = new Date().getFullYear();
  return { from: `${year}-01-01`, to: `${year}-12-31` };
}

function minutesToHours(minutes: number) {
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return `${h}:${m.toString().padStart(2, "0")}`;
}

export default function ReportsPage() {
  const { t: tr } = useI18N();
  const [headcount, setHeadcount] = useState<Headcount | null>(null);
  const [costRows, setCostRows] = useState<CostRow[] | null>(null);
  const [otRows, setOtRows] = useState<OtRow[] | null>(null);
  const [from, setFrom] = useState(yearRange().from);
  const [to, setTo] = useState(yearRange().to);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const loadHeadcount = useCallback(async () => {
    const r = await gw<Headcount>("api/v1/reports/headcount");
    setHeadcount(r);
  }, []);

  const loadCostAndOt = useCallback(async (f: string, t: string) => {
    const [cost, ot] = await Promise.all([
      gw<{ rows: CostRow[] }>(`api/v1/reports/payroll-cost?from=${f}&to=${t}`),
      gw<{ rows: OtRow[] }>(`api/v1/reports/ot-trend?from=${f}&to=${t}`),
    ]);
    setCostRows(cost.rows);
    setOtRows(ot.rows);
  }, []);

  useEffect(() => {
    loadHeadcount().catch((e) => handleApiError(e, setError));
  }, [loadHeadcount]);

  useEffect(() => {
    loadCostAndOt(from, to).catch((e) => handleApiError(e, setError));
  }, [loadCostAndOt, from, to]);

  async function reloadCost(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      await loadCostAndOt(from, to);
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
          <h1>{tr("reports.reports")}</h1>
          <p className="page-sub">{tr("reports.headcount-payroll-cost-and-ot-trends")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}

      <div className="card" style={{ marginBottom: 16 }}>
        <div className="card-head"><h2>{tr("reports.number-of-employees")}</h2></div>
        <div className="card-pad">
        {headcount === null ? (
          <div className="spinner">{tr("reports.loading")}</div>
        ) : (
          <>
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "repeat(auto-fit, minmax(140px, 1fr))",
                gap: 12,
                marginBottom: 16,
              }}
            >
              <div className="mine-tile" data-testid="headcount-active">
                <span className="mine-label">{tr("reports.active")}</span>
                <span className="mine-value">{headcount.active}</span>
              </div>
              <div className="mine-tile" data-testid="headcount-probation">
                <span className="mine-label">{tr("reports.probation")}</span>
                <span className="mine-value">{headcount.probation}</span>
              </div>
              <div className="mine-tile" data-testid="headcount-leave">
                <span className="mine-label">{tr("reports.leave")}</span>
                <span className="mine-value">{headcount.leave}</span>
              </div>
              <div className="mine-tile" data-testid="headcount-terminated">
                <span className="mine-label">{tr("reports.expired-for-12-months")}</span>
                <span className="mine-value">{headcount.terminated_last_12m}</span>
              </div>
            </div>

            <table className="table" aria-label={tr("reports.number-of-employees-by-department")}>
              <thead>
                <tr>
                  <th scope="col">{tr("reports.department")}</th>
                  <th scope="col" className="num">{tr("reports.quantity")}</th>
                </tr>
              </thead>
              <tbody>
                {headcount.by_department.map((d) => (
                  <tr key={d.department}>
                    <td>{d.department}</td>
                    <td className="num">{d.count}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        )}
        </div>
      </div>

      <div className="card" style={{ marginBottom: 16 }}>
        <div className="card-head"><h2>{tr("reports.salary-costs")}</h2></div>
        <div className="card-pad">
        <form
          onSubmit={reloadCost}
          className="form-row"
          style={{ marginBottom: 16, alignItems: "end" }}
        >
          <div className="field">
            <label htmlFor="cost-from">{tr("reports.from")}</label>
            <ThaiDateInput id="cost-from" testId="cost-from" value={from} onChange={(v) => setFrom(v)} required />
          </div>
          <div className="field">
            <label htmlFor="cost-to">{tr("reports.to")}</label>
            <ThaiDateInput id="cost-to" testId="cost-to" value={to} onChange={(v) => setTo(v)} required />
          </div>
          <div className="field">
            <span aria-hidden="true">&nbsp;</span>
            <button className="btn btn-primary" data-testid="cost-load" type="submit" disabled={busy}>
              {busy ? tr("reports.loading") : tr("reports.view-report")}
            </button>
          </div>
        </form>

        {costRows === null ? (
          <div className="spinner">{tr("reports.loading")}</div>
        ) : costRows.length === 0 ? (
          <div className="empty">{tr("reports.cost-information-is-not-available-for")}</div>
        ) : (
          <table className="table" aria-label={tr("reports.salary-costs")}>
            <thead>
              <tr>
                <th scope="col">{tr("reports.department")}</th>
                <th scope="col" className="num">{tr("reports.person")}</th>
                <th scope="col" className="num">{tr("reports.total-income")}</th>
                <th scope="col" className="num">{tr("reports.net")}</th>
                <th scope="col" className="num">{tr("reports.tax")}</th>
                <th scope="col" className="num">{tr("reports.social-security")}</th>
                <th scope="col" className="num">{tr("reports.fund")}</th>
              </tr>
            </thead>
            <tbody>
              {costRows.map((r) => (
                <tr key={r.department}>
                  <td>{r.department}</td>
                  <td className="num">{r.employees}</td>
                  <td className="num">{tr("reports.text")}{thb(r.gross)}</td>
                  <td className="num">{tr("reports.text")}{thb(r.net)}</td>
                  <td className="num">{tr("reports.text")}{thb(r.tax)}</td>
                  <td className="num">{tr("reports.text")}{thb(r.sso)}</td>
                  <td className="num">{tr("reports.text")}{thb(r.pf)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        </div>
      </div>

      <div className="card">
        <div className="card-head"><h2>{tr("reports.ot-trends")}</h2></div>
        <div className="card-pad">
        {otRows === null ? (
          <div className="spinner">{tr("reports.loading")}</div>
        ) : otRows.length === 0 ? (
          <div className="empty">{tr("reports.there-is-no-ot-data-for")}</div>
        ) : (
          <table className="table" aria-label={tr("reports.ot-trends")}>
            <thead>
              <tr>
                <th scope="col">{tr("reports.month")}</th>
                <th scope="col" className="num">{tr("reports.ot-hours")}</th>
                <th scope="col" className="num">{tr("reports.days")}</th>
              </tr>
            </thead>
            <tbody>
              {otRows.map((r) => (
                <tr key={r.month}>
                  <td>{r.month}</td>
                  <td className="num">{minutesToHours(r.ot_minutes)}</td>
                  <td className="num">{r.approved_days}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        </div>
      </div>
    </>
  );
}

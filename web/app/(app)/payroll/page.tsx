"use client";

import ThaiDateInput from "@/components/ThaiDateInput";

import { useCallback, useEffect, useRef, useState } from "react";
import {
  gw,
  gql,
  handleApiError,
  PayrollAnomalies,
  PayrollRun,
  Payslip,
  RUN_STATUS,
  thb,
  thDate,
} from "@/lib/api";
import { useI18N } from "@/lib/i18n";

function monthDefaults() {
  const now = new Date();
  const start = new Date(now.getFullYear(), now.getMonth(), 1);
  const end = new Date(now.getFullYear(), now.getMonth() + 1, 0);
  const iso = (d: Date) => d.toISOString().slice(0, 10);
  return { start: iso(start), end: iso(end), pay: iso(end) };
}

const RUNS_QUERY =
  "{ payrollRuns(limit: 24) { id periodStart periodEnd paymentDate status totalEmployees totalGross totalNet totalTax totalSso totalPf paidAt paymentReference } }";

export default function PayrollPage() {
  const { t } = useI18N();
  const defaults = useRef(monthDefaults());
  const [runs, setRuns] = useState<PayrollRun[] | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [payslips, setPayslips] = useState<Record<string, Payslip[]>>({});
  const [anomalies, setAnomalies] = useState<Record<string, PayrollAnomalies>>({});
  const [periodStart, setPeriodStart] = useState(defaults.current.start);
  const [periodEnd, setPeriodEnd] = useState(defaults.current.end);
  const [paymentDate, setPaymentDate] = useState(defaults.current.pay);
  const [entityId, setEntityId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [pfStates, setPfStates] = useState<Record<string, { status: string; last_error?: string }>>({});

  const load = useCallback(async () => {
    const r = await gql<{ payrollRuns: PayrollRun[] }>(RUNS_QUERY);
    setRuns(r.payrollRuns);
    return r.payrollRuns;
  }, []);

  useEffect(() => {
    load().catch((e) => handleApiError(e, setError));
  }, [load]);

  // Runs execute in the background. A just-created run can still read as
  // "draft" before the worker flips it, so poll for both states.
  useEffect(() => {
    if (!runs?.some((r) => r.status === "draft" || r.status === "calculating")) return;
    const timer = setInterval(() => {
      load().catch(() => undefined);
    }, 2500);
    return () => clearInterval(timer);
  }, [runs, load]);

  async function createRun(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const body: Record<string, unknown> = {
        period_start: periodStart,
        period_end: periodEnd,
        payment_date: paymentDate,
      };
      if (entityId.trim()) body.entity_id = entityId.trim();
      await gw("api/v1/payroll/runs", { method: "POST", body: JSON.stringify(body) });
      setNotice(t("payroll.createdNotice"));
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function toggle(run: PayrollRun) {
    if (expanded === run.id) {
      setExpanded(null);
      return;
    }
    setExpanded(run.id);
    if (!payslips[run.id]) {
      try {
        const r = await gw<{ payslips: Payslip[] }>(`api/v1/payroll/runs/${run.id}/payslips`);
        setPayslips((prev) => ({ ...prev, [run.id]: r.payslips }));
      } catch (e) {
        handleApiError(e, setError);
      }
    }
    if (!anomalies[run.id]) {
      try {
        const a = await gw<PayrollAnomalies>(`api/v1/payroll/runs/${run.id}/anomalies`);
        setAnomalies((prev) => ({ ...prev, [run.id]: a }));
      } catch (e) {
        handleApiError(e, setError);
      }
    }
  }

  async function reviewAnomalies(run: PayrollRun) {
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/payroll/runs/${run.id}/anomalies/review`, { method: "POST" });
      setNotice(t("payroll.anomaliesReviewedNotice"));
      const a = await gw<PayrollAnomalies>(`api/v1/payroll/runs/${run.id}/anomalies`);
      setAnomalies((prev) => ({ ...prev, [run.id]: a }));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function approve(run: PayrollRun) {
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/payroll/runs/${run.id}/approve`, {
        method: "POST",
        body: JSON.stringify({ reason: null }),
      });
      setNotice(t("payroll.approvedNotice"));
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function sendPf(run: PayrollRun) {
    setBusy(true);
    setError("");
    try {
      const r = await gw<{ status: string; attempts: number; sent_at?: string; last_error?: string }>(
        `api/v1/payroll/runs/${run.id}/pf-remittance/send`,
        { method: "POST" },
      );
      setPfStates((prev) => ({ ...prev, [run.id]: { status: r.status, last_error: r.last_error } }));
      setNotice(r.status === "sent" ? t("payroll.pfSentNotice") : t("payroll.pfFailedNotice"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function markPaid(run: PayrollRun) {
    const raw = window.prompt(t("payroll.markPaid"));
    if (raw === null) return;
    const reference = raw.trim();
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/payroll/runs/${run.id}/mark-paid`, {
        method: "POST",
        body: JSON.stringify({ reference: reference || null }),
      });
      setNotice(t("payroll.markedPaidNotice"));
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
          <h1>{t("payroll.title")}</h1>
          <p className="page-sub">{t("payroll.subtitle")}</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      <form className="card card-pad" onSubmit={createRun}>
        <h2 style={{ marginBottom: 14 }}>{t("payroll.createTitle")}</h2>
        <div className="form-row">
          <div className="field">
            <label htmlFor="start" data-testid="create-start-label">{t("payroll.periodStart")}</label>
            <ThaiDateInput id="start" testId="create-start" value={periodStart} onChange={(v) => setPeriodStart(v)} required />
          </div>
          <div className="field">
            <label htmlFor="end">{t("payroll.periodEnd")}</label>
            <ThaiDateInput id="end" testId="create-end" value={periodEnd} onChange={(v) => setPeriodEnd(v)} required />
          </div>
          <div className="field">
            <label htmlFor="pay">{t("payroll.paymentDate")}</label>
            <ThaiDateInput id="pay" value={paymentDate} onChange={(v) => setPaymentDate(v)} required />
          </div>
        </div>
        <div className="form-row" style={{ marginTop: 12, gridTemplateColumns: "1fr auto" }}>
          <div className="field">
            <label htmlFor="entity">{t("payroll.entityId")}</label>
            <input
              id="entity"
              placeholder="เว้นว่างได้เมื่อโทเคนผูกกับนิติบุคคลเดียว"
              value={entityId}
              onChange={(e) => setEntityId(e.target.value)}
            />
          </div>
          <div style={{ display: "flex", alignItems: "end" }}>
            <button className="btn btn-primary" data-testid="create-run" type="submit" disabled={busy}>
              {busy ? t("common.loading") : t("payroll.createRun")}
            </button>
          </div>
        </div>
      </form>

      <div className="card">
        <div className="card-head">
          <h2>{t("payroll.allRuns")}</h2>
          <span className="hint">{runs ? t("payroll.runsCount", { n: runs.length }) : ""}</span>
        </div>
        {runs === null ? (
          <div className="spinner">{t("common.loading")}</div>
        ) : runs.length === 0 ? (
          <div className="empty">{t("payroll.emptyRuns")}</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>{t("payroll.thPeriod")}</th>
                <th>{t("payroll.paymentDate")}</th>
                <th>{t("payroll.thStatus")}</th>
                <th className="num">{t("payroll.thEmployees")}</th>
                <th className="num">{t("payroll.thGross")}</th>
                <th className="num">{t("payroll.thNet")}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {runs.map((run) => {
                const status = RUN_STATUS[run.status] ?? { label: run.status, tone: "" };
                const open = expanded === run.id;
                return (
                  <RunRow
                    key={run.id}
                    run={run}
                    status={status}
                    open={open}
                    payslips={payslips[run.id]}
                    anomalies={anomalies[run.id]}
                    busy={busy}
                    pfState={pfStates[run.id]}
                    onToggle={() => toggle(run)}
                    onApprove={() => approve(run)}
                    onReviewAnomalies={() => reviewAnomalies(run)}
                    onSendPf={() => sendPf(run)}
                    onMarkPaid={() => markPaid(run)}
                  />
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}

function RunRow({
  run,
  status,
  open,
  payslips,
  anomalies,
  busy,
  pfState,
  onToggle,
  onApprove,
  onReviewAnomalies,
  onSendPf,
  onMarkPaid,
}: {
  run: PayrollRun;
  status: { label: string; tone: string };
  open: boolean;
  payslips: Payslip[] | undefined;
  anomalies: PayrollAnomalies | undefined;
  busy: boolean;
  pfState?: { status: string; last_error?: string };
  onToggle: () => void;
  onApprove: () => void;
  onReviewAnomalies: () => void;
  onSendPf: () => void;
  onMarkPaid: () => void;
}) {
  const { t } = useI18N();
  const anomalyCount = anomalies ? anomalies.counts.error + anomalies.counts.warning : 0;
  const needsReview = anomalyCount > 0 && !anomalies?.reviewed_at;
  return (
    <>
      <tr className="clickable" data-testid="run-row" onClick={onToggle}>
        <td>
          {thDate(run.periodStart)} – {thDate(run.periodEnd)}
        </td>
        <td>{thDate(run.paymentDate)}</td>
        <td>
          <span className={`badge ${status.tone}`}>
            <span className="dot" />
            {t(`payroll.status.${run.status}`)}
          </span>
        </td>
        <td className="num">{run.totalEmployees}</td>
        <td className="num">฿{thb(run.totalGross)}</td>
        <td className="num">฿{thb(run.totalNet)}</td>
        <td className="num" onClick={(e) => e.stopPropagation()}>
          <a
            className="badge accent"
            href={`/api/gw/api/v1/payroll/runs/${run.id}/export.csv`}
            target="_blank"
            rel="noreferrer"
            style={{ marginRight: 6 }}
          >
            CSV
          </a>
          {(run.status === "approved" || run.status === "completed") && (
            <a
              className="badge accent"
              data-testid="bank-payout"
              href={`/api/gw/api/v1/payroll/runs/${run.id}/bank-payout.csv`}
              style={{ marginRight: 6 }}
            >
                {t("payroll.bankPayout")}
            </a>
          )}
          {run.status === "approved" && (
            <>
              <button
                className="btn btn-sm btn-primary"
                data-testid="mark-paid"
                disabled={busy}
                onClick={onMarkPaid}
                style={{ marginRight: 6 }}
              >
                {t("payroll.markPaid")}
              </button>
              <a
                className="badge accent"
                href={`/api/gw/api/v1/payroll/runs/${run.id}/export.gl.csv`}
                target="_blank"
                rel="noreferrer"
                style={{ marginRight: 6 }}
              >
                {t("payroll.glDownload")}
              </a>
              <a
                className="badge accent"
                href={`/api/gw/api/v1/payroll/runs/${run.id}/pf-remittance.csv`}
                target="_blank"
                rel="noreferrer"
                style={{ marginRight: 6 }}
              >
                {t("payroll.pfDownload")}
              </a>
              <button
                className="btn btn-sm btn-primary"
                data-testid="send-pf"
                disabled={busy || pfState?.status === "sent"}
                onClick={onSendPf}
              >
                {t("payroll.sendPf")}
              </button>
              {pfState && (
                <span
                  className={`badge ${pfState.status === "sent" ? "success" : "danger"}`}
                  data-testid="pf-status"
                  style={{ marginLeft: 6 }}
                >
                  {pfState.status === "sent" ? t("payroll.pfSent") : t("payroll.pfFailed")}
                </span>
              )}
            </>
          )}
          {run.status === "completed" && (
            <span className="badge" data-testid="paid-info" style={{ marginLeft: 6 }}>
              {t("payroll.paidInfo")}
              {run.paidAt ? ` · ${thDate(run.paidAt.slice(0, 10))}` : ""}
              {run.paymentReference ? ` · ${run.paymentReference}` : ""}
            </span>
          )}
          {run.status === "pending_approval" && (
            <>
              <button
                className="btn btn-sm btn-primary"
                data-testid="approve-run"
                disabled={busy || needsReview}
                onClick={onApprove}
              >
                {t("payroll.approve")}
              </button>
              {needsReview && (
                <span className="badge danger" style={{ marginLeft: 6 }} data-testid="needs-review">
                  {t("payroll.reviewBeforeApprove")}
                </span>
              )}
            </>
          )}
        </td>
      </tr>
      {open && (
        <tr>
          <td colSpan={7} style={{ background: "#fbfbfd" }}>
            <AnomalyPanel anomalies={anomalies} busy={busy} onReview={onReviewAnomalies} />
            {!payslips ? (
              <div className="spinner">{t("payroll.payslipLoading")}</div>
            ) : payslips.length === 0 ? (
              <div className="empty">{t("payroll.noPayslips")}</div>
            ) : (
              <table className="table">
                <thead>
                  <tr>
                    <th>{t("payroll.payslipEmployee")}</th>
                    <th className="num">{t("payroll.payslipSalary")}</th>
                    <th className="num">{t("payroll.payslipTax")}</th>
                    <th className="num">{t("payroll.payslipSso")}</th>
                    <th className="num">{t("payroll.payslipNet")}</th>
                    <th>{t("payroll.payslipSlip")}</th>
                  </tr>
                </thead>
                <tbody>
                  {payslips.map((p) => (
                    <tr key={p.payslip_id}>
                      <td>
                        <div style={{ fontWeight: 500 }}>{p.employee_name}</div>
                        <div className="hint">{p.employee_code}</div>
                      </td>
                      <td className="num">฿{thb(p.base_salary)}</td>
                      <td className="num">฿{thb(p.tax_withheld)}</td>
                      <td className="num">฿{thb(p.sso_employee)}</td>
                      <td className="num">฿{thb(p.net_pay)}</td>
                      <td>
                        <a
                          className="badge accent"
                          href={`/api/gw/api/v1/payroll/payslips/${p.payslip_id}/pdf`}
                          target="_blank"
                        >
                          PDF
                        </a>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </td>
        </tr>
      )}
    </>
  );
}

function AnomalyPanel({
  anomalies,
  busy,
  onReview,
}: {
  anomalies: PayrollAnomalies | undefined;
  busy: boolean;
  onReview: () => void;
}) {
  const { t } = useI18N();
  if (!anomalies) return null;
  const total = anomalies.counts.error + anomalies.counts.warning;
  return (
    <div className="card card-pad" style={{ marginBottom: 12 }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <h3 style={{ margin: 0 }}>{t("payroll.anomalyTitle")}</h3>
        {total > 0 &&
          (anomalies.reviewed_at ? (
            <span className="badge success" data-testid="anomaly-reviewed">
              {t("payroll.reviewAnomalies")}
            </span>
          ) : (
            <button className="btn btn-sm btn-primary" data-testid="review-anomalies" disabled={busy} onClick={onReview}>
              {t("payroll.reviewAnomalies")}
            </button>
          ))}
      </div>
      {total === 0 ? (
        <div className="empty">{t("payroll.noAnomalies")}</div>
      ) : (
        <>
          <div style={{ margin: "8px 0" }}>
            <span className="badge danger">{t("payroll.errorBadge")} {anomalies.counts.error}</span>{" "}
            <span className="badge warning">{t("payroll.warningBadge")} {anomalies.counts.warning}</span>
          </div>
          <table className="table">
            <thead>
              <tr>
                <th>{t("payroll.anomalyEmployee")}</th>
                <th>{t("payroll.anomalySeverity")}</th>
                <th>{t("payroll.anomalyCode")}</th>
                <th>{t("payroll.anomalyDetail")}</th>
              </tr>
            </thead>
            <tbody>
              {anomalies.anomalies.map((a, i) => (
                <tr key={i}>
                  <td>
                    <div style={{ fontWeight: 500 }}>{a.employee_name}</div>
                    <div className="hint">{a.employee_code}</div>
                  </td>
                  <td>
                    <span className={`badge ${a.severity === "error" ? "danger" : "warning"}`}>
                      {a.severity === "error" ? t("payroll.errorBadge") : t("payroll.warningBadge")}
                    </span>
                  </td>
                  <td className="hint">{a.code}</td>
                  <td>{a.message}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </div>
  );
}

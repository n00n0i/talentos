"use client";

import { useI18N } from "@/lib/i18n";

import { useEffect, useState } from "react";
import { gw, handleApiError, RUN_STATUS, thb, thDate } from "@/lib/api";

type SelfPayslip = {
  id: string;
  period_start: string;
  period_end: string;
  gross: string;
  net: string;
  status: string;
  acknowledged_at: string | null;
};

export default function MyPayslipsPage() {
  const { t: tr } = useI18N();
  const [linked, setLinked] = useState(true);
  const [rows, setRows] = useState<SelfPayslip[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const load = () =>
    gw<{ employee_linked: boolean; payslips: SelfPayslip[] }>("api/v1/me/payslips")
      .then((r) => {
        setLinked(r.employee_linked);
        setRows(r.payslips);
      })
      .catch((e) => handleApiError(e, setError));

  useEffect(() => {
    load();
  }, []);

  async function acknowledge(id: string) {
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/me/payslips/${id}/acknowledge`, { method: "POST" });
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function signPayslip(id: string) {
    const name = window.prompt(tr("mePayslips.enter-your-name-to-verify-signature"));
    if (!name || !name.trim()) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/me/payslips/${id}/sign`, {
        method: "POST",
        body: JSON.stringify({ typed_name: name.trim() }),
      });
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
          <h1>{tr("mePayslips.my-payslips")}</h1>
          <p className="page-sub">{tr("mePayslips.salary-slip-has-been-issued-to")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}

      <div className="card">
        {rows === null ? (
          <div className="spinner">{tr("mePayslips.loading")}</div>
        ) : !linked ? (
          <div className="empty">
            {tr("mePayslips.this-account-has-not-been-linked")}
          </div>
        ) : rows.length === 0 ? (
          <div className="empty">{tr("mePayslips.don-t-have-a-pay-slip")}</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{tr("mePayslips.period")}</th>
                <th scope="col">{tr("mePayslips.status")}</th>
                <th scope="col" className="num">{tr("mePayslips.total")}</th>
                <th scope="col" className="num">{tr("mePayslips.net")}</th>
                <th scope="col">{tr("mePayslips.slip")}</th>
                <th scope="col">{tr("mePayslips.payslip-acknowledgement")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((p) => {
                const status = RUN_STATUS[p.status];
                return (
                <tr key={p.id}>
                  <td>
                    {thDate(p.period_start)} – {thDate(p.period_end)}
                  </td>
                  <td>
                    <span className={`badge ${status?.tone ?? ""}`}>{status ? tr(status.labelKey) : p.status}</span>
                  </td>
                  <td className="num">{tr("mePayslips.text")}{thb(p.gross)}</td>
                  <td className="num">{tr("mePayslips.text")}{thb(p.net)}</td>
                  <td>
                    <a
                      className="badge accent"
                      href={`/api/gw/api/v1/me/payslips/${p.id}/pdf`}
                      target="_blank"
                      rel="noreferrer"
                    >
                      PDF
                    </a>{" "}
                    <a
                      className="badge"
                      data-testid="payslip-50tawi"
                      href={`/api/gw/api/v1/me/payslips/${p.id}/50tawi`}
                      target="_blank"
                      rel="noreferrer"
                    >
                      {tr("mePayslips.50-tawi")}
                    </a>
                  </td>
                  <td data-testid="payslip-ack-status">
                    {p.acknowledged_at ? (
                      <span className="badge success">{tr("mePayslips.confirmed")}</span>
                    ) : (
                      <button
                        className="btn btn-sm"
                        data-testid="acknowledge-payslip"
                        disabled={busy}
                        onClick={() => acknowledge(p.id)}
                      >
                        {tr("mePayslips.confirmed-receipt-of-slip")}
                      </button>
                    )}
                    <button
                      className="btn btn-sm"
                      data-testid="payslip-sign"
                      disabled={busy}
                      style={{ marginLeft: 6 }}
                      onClick={() => signPayslip(p.id)}
                    >
                      {tr("mePayslips.signature")}
                    </button>
                  </td>
                </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}

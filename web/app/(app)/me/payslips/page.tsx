"use client";

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

  return (
    <>
      <div className="page-head">
        <div>
          <h1>สลิปของฉัน</h1>
          <p className="page-sub">สลิปเงินเดือนที่ออกให้คุณแล้ว</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}

      <div className="card">
        {rows === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : !linked ? (
          <div className="empty">
            ยังไม่ได้ผูกบัญชีนี้กับข้อมูลพนักงาน — ติดต่อ HR เพื่อเชื่อมบัญชี
          </div>
        ) : rows.length === 0 ? (
          <div className="empty">ยังไม่มีสลิปเงินเดือน</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>งวด</th>
                <th>สถานะ</th>
                <th className="num">ยอดรวม</th>
                <th className="num">สุทธิ</th>
                <th>สลิป</th>
                <th>การรับสลิป</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((p) => {
                const status = RUN_STATUS[p.status] ?? { label: p.status, tone: "" };
                return (
                <tr key={p.id}>
                  <td>
                    {thDate(p.period_start)} – {thDate(p.period_end)}
                  </td>
                  <td>
                    <span className={`badge ${status.tone}`}>{status.label}</span>
                  </td>
                  <td className="num">฿{thb(p.gross)}</td>
                  <td className="num">฿{thb(p.net)}</td>
                  <td>
                    <a
                      className="badge accent"
                      href={`/api/gw/api/v1/me/payslips/${p.id}/pdf`}
                      target="_blank"
                      rel="noreferrer"
                    >
                      PDF
                    </a>
                  </td>
                  <td data-testid="payslip-ack-status">
                    {p.acknowledged_at ? (
                      <span className="badge success">ยืนยันแล้ว</span>
                    ) : (
                      <button
                        className="btn btn-sm"
                        data-testid="acknowledge-payslip"
                        disabled={busy}
                        onClick={() => acknowledge(p.id)}
                      >
                        ยืนยันรับสลิปแล้ว
                      </button>
                    )}
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

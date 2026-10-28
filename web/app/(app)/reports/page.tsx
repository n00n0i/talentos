"use client";

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
          <h1>รายงาน</h1>
          <p className="page-sub">ภาพรวมบุคลากร ต้นทุนเงินเดือน และแนวโน้ม OT</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}

      <div className="card card-pad" style={{ marginBottom: 16 }}>
        <h2 style={{ marginBottom: 14 }}>จำนวนพนักงาน</h2>
        {headcount === null ? (
          <div className="spinner">กำลังโหลด…</div>
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
              <div className="card" data-testid="headcount-active">
                <div className="hint">ทำงานอยู่</div>
                <div style={{ fontSize: 24, fontWeight: 600 }}>{headcount.active}</div>
              </div>
              <div className="card" data-testid="headcount-probation">
                <div className="hint">ทดลองงาน</div>
                <div style={{ fontSize: 24, fontWeight: 600 }}>{headcount.probation}</div>
              </div>
              <div className="card" data-testid="headcount-leave">
                <div className="hint">ลา</div>
                <div style={{ fontSize: 24, fontWeight: 600 }}>{headcount.leave}</div>
              </div>
              <div className="card" data-testid="headcount-terminated">
                <div className="hint">พ้นสภาพ 12 เดือน</div>
                <div style={{ fontSize: 24, fontWeight: 600 }}>{headcount.terminated_last_12m}</div>
              </div>
            </div>

            <table className="table" aria-label="จำนวนพนักงานตามแผนก">
              <thead>
                <tr>
                  <th>แผนก</th>
                  <th className="num">จำนวน</th>
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

      <div className="card card-pad" style={{ marginBottom: 16 }}>
        <h2 style={{ marginBottom: 14 }}>ต้นทุนเงินเดือน</h2>
        <form
          onSubmit={reloadCost}
          className="form-row"
          style={{ marginBottom: 16, alignItems: "end" }}
        >
          <div className="field">
            <label htmlFor="cost-from">จาก</label>
            <input id="cost-from" type="date" data-testid="cost-from" value={from} onChange={(e) => setFrom(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="cost-to">ถึง</label>
            <input id="cost-to" type="date" data-testid="cost-to" value={to} onChange={(e) => setTo(e.target.value)} required />
          </div>
          <div style={{ display: "flex", alignItems: "end" }}>
            <button className="btn btn-primary" data-testid="cost-load" type="submit" disabled={busy}>
              {busy ? "กำลังโหลด…" : "ดูรายงาน"}
            </button>
          </div>
        </form>

        {costRows === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : costRows.length === 0 ? (
          <div className="empty">ไม่มีข้อมูลต้นทุนในช่วงที่เลือก</div>
        ) : (
          <table className="table" aria-label="ต้นทุนเงินเดือน">
            <thead>
              <tr>
                <th>แผนก</th>
                <th className="num">คน</th>
                <th className="num">รายได้รวม</th>
                <th className="num">สุทธิ</th>
                <th className="num">ภาษี</th>
                <th className="num">ประกันสังคม</th>
                <th className="num">กองทุน</th>
              </tr>
            </thead>
            <tbody>
              {costRows.map((r) => (
                <tr key={r.department}>
                  <td>{r.department}</td>
                  <td className="num">{r.employees}</td>
                  <td className="num">฿{thb(r.gross)}</td>
                  <td className="num">฿{thb(r.net)}</td>
                  <td className="num">฿{thb(r.tax)}</td>
                  <td className="num">฿{thb(r.sso)}</td>
                  <td className="num">฿{thb(r.pf)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <div className="card card-pad">
        <h2 style={{ marginBottom: 14 }}>แนวโน้ม OT</h2>
        {otRows === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : otRows.length === 0 ? (
          <div className="empty">ไม่มีข้อมูล OT ในช่วงที่เลือก</div>
        ) : (
          <table className="table" aria-label="แนวโน้ม OT">
            <thead>
              <tr>
                <th>เดือน</th>
                <th className="num">ชั่วโมง OT</th>
                <th className="num">จำนวนวัน</th>
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
    </>
  );
}

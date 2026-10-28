"use client";

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
      setNotice("คำนวณเงินค่าจ้างแล้ว");
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
          <h1>พนักงานลาออก</h1>
          <p className="page-sub">คำนวณเงินค่าจ้างสุดท้าย (เงินเดือน, วันลาค้าง, ค่าบอกกล่าว, ค่าชดเชย)</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      <form className="card card-pad" onSubmit={submit} style={{ marginBottom: 16 }}>
        <h2 style={{ marginBottom: 14 }}>คำนวณเงินค่าจ้าง</h2>
        <div className="form-row" style={{ alignItems: "end" }}>
          <div className="field">
            <label htmlFor="settlement-employee">พนักงาน</label>
            <select
              id="settlement-employee"
              data-testid="settlement-employee"
              value={employeeId}
              onChange={(e) => setEmployeeId(e.target.value)}
              required
            >
              <option value="">เลือกพนักงาน</option>
              {employees.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.employee_code} · {e.first_name_th} {e.last_name_th}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor="settlement-date">วันที่สิ้นสุดการจ้าง</label>
            <input
              id="settlement-date"
              data-testid="settlement-date"
              type="date"
              value={terminationDate}
              onChange={(e) => setTerminationDate(e.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="settlement-unused">วันลาค้าง (วัน)</label>
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
            <label htmlFor="settlement-notice">วันบอกกล่าวล่วงหน้า (วัน)</label>
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
              คำนวณค่าชดเชย
            </label>
          </div>
          <div style={{ display: "flex", alignItems: "end" }}>
            <button
              className="btn btn-primary"
              data-testid="compute-settlement"
              type="submit"
              disabled={busy}
            >
              {busy ? "กำลังคำนวณ…" : "คำนวณ"}
            </button>
          </div>
        </div>
      </form>

      {latest && (
        <div className="card card-pad" style={{ marginBottom: 16 }}>
          <h2 style={{ marginBottom: 14 }}>ผลการคำนวณ</h2>
          <div className="grid" style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(160px, 1fr))", gap: 12, marginBottom: 16 }}>
            <div className="metric">
              <span className="metric-label">อัตรา/วัน</span>
              <span className="metric-value">฿{thb(latest.breakdown.daily_rate)}</span>
            </div>
            <div className="metric">
              <span className="metric-label">เงินเดือนสุดท้าย ({latest.breakdown.last_month_days} วัน)</span>
              <span className="metric-value">฿{thb(latest.last_month_pay)}</span>
            </div>
            <div className="metric">
              <span className="metric-label">ค่าวันลาค้าง</span>
              <span className="metric-value">฿{thb(latest.leave_pay)}</span>
            </div>
            <div className="metric">
              <span className="metric-label">ค่าบอกกล่าว</span>
              <span className="metric-value">฿{thb(latest.notice_pay)}</span>
            </div>
            <div className="metric">
              <span className="metric-label">ค่าชดเชย ({latest.breakdown.severance_days} วัน)</span>
              <span className="metric-value">฿{thb(latest.severance_pay)}</span>
            </div>
            <div className="metric accent">
              <span className="metric-label">รวมก่อนหักภาษี</span>
              <span className="metric-value" data-testid="settlement-total">
                ฿{thb(latest.total_gross)}
              </span>
            </div>
          </div>

          <table className="table">
            <thead>
              <tr>
                <th>รายการ</th>
                <th className="num">จำนวนเงิน</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>เงินเดือนสุดท้าย ({latest.breakdown.last_month_days} วัน)</td>
                <td className="num">฿{thb(latest.last_month_pay)}</td>
              </tr>
              <tr>
                <td>ค่าวันลาค้าง ({latest.unused_leave_days} วัน)</td>
                <td className="num">฿{thb(latest.leave_pay)}</td>
              </tr>
              <tr>
                <td>ค่าบอกกล่าวล่วงหน้า ({latest.notice_days} วัน)</td>
                <td className="num">฿{thb(latest.notice_pay)}</td>
              </tr>
              <tr>
                <td>ค่าชดเชย ({latest.breakdown.severance_days} วัน)</td>
                <td className="num">฿{thb(latest.severance_pay)}</td>
              </tr>
              <tr className="total">
                <td>รวมก่อนหักภาษี</td>
                <td className="num">฿{thb(latest.total_gross)}</td>
              </tr>
            </tbody>
          </table>
        </div>
      )}

      <div className="card">
        {settlements === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : settlements.length === 0 ? (
          <div className="empty">ไม่มีรายการคำนวณ</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>พนักงาน</th>
                <th>วันที่สิ้นสุด</th>
                <th className="num">วันลาค้าง</th>
                <th className="num">บอกกล่าว</th>
                <th className="num">ค่าชดเชย</th>
                <th className="num">รวม</th>
                <th>สถานะ</th>
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
                  <td className="num">฿{thb(s.severance_pay)}</td>
                  <td className="num">฿{thb(s.total_gross)}</td>
                  <td>
                    <span className="badge">{s.status === "draft" ? "ฉบับร่าง" : s.status}</span>
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

"use client";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, thb, thDate } from "@/lib/api";

type Employee = {
  id: string;
  employee_code: string;
  first_name_th: string;
  last_name_th: string;
};

type Loan = {
  id: string;
  employee_id: string;
  employee_code: string;
  employee_name: string;
  principal: string;
  balance: string;
  installment_amount: string;
  start_date: string;
  status: string;
  note: string | null;
};

const STATUS_LABEL: Record<string, string> = {
  active: "กำลังผ่อน",
  paid: "ชำระครบ",
  cancelled: "ยกเลิก",
};

export default function LoansPage() {
  const [loans, setLoans] = useState<Loan[] | null>(null);
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [employeeId, setEmployeeId] = useState("");
  const [principal, setPrincipal] = useState("");
  const [installment, setInstallment] = useState("");
  const [startDate, setStartDate] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    const r = await gw<{ loans: Loan[] }>("api/v1/loans");
    setLoans(r.loans);
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
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw("api/v1/loans", {
        method: "POST",
        body: JSON.stringify({
          employee_id: employeeId,
          principal: Number(principal),
          installment_amount: Number(installment),
          start_date: startDate,
          note: note.trim() || null,
        }),
      });
      setNotice("สร้างเงินกู้แล้ว");
      setEmployeeId("");
      setPrincipal("");
      setInstallment("");
      setStartDate("");
      setNote("");
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function cancel(loan: Loan) {
    if (!confirm(`ยืนยันยกเลิกเงินกู้ของ ${loan.employee_code}?`)) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/loans/${loan.id}/cancel`, { method: "POST" });
      setNotice("ยกเลิกเงินกู้แล้ว");
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
          <h1>สินเชื่อ/เงินกู้</h1>
          <p className="page-sub">บันทึกเงินกู้พนักงานและติดตามยอดคงเหลือ</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      <form className="card card-pad" onSubmit={submit} style={{ marginBottom: 16 }}>
        <h2 style={{ marginBottom: 14 }}>สร้างเงินกู้</h2>
        <div className="form-row" style={{ alignItems: "end" }}>
          <div className="field">
            <label htmlFor="loan-employee">พนักงาน</label>
            <select
              id="loan-employee"
              data-testid="loan-employee"
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
            <label htmlFor="loan-principal">เงินต้น (บาท)</label>
            <input
              id="loan-principal"
              data-testid="loan-principal"
              type="number"
              min="1"
              step="0.01"
              value={principal}
              onChange={(e) => setPrincipal(e.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="loan-installment">งวดผ่อนต่อรอบ (บาท)</label>
            <input
              id="loan-installment"
              data-testid="loan-installment"
              type="number"
              min="1"
              step="0.01"
              value={installment}
              onChange={(e) => setInstallment(e.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="loan-start">วันเริ่มผ่อน</label>
            <input
              id="loan-start"
              data-testid="loan-start"
              type="date"
              value={startDate}
              onChange={(e) => setStartDate(e.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="loan-note">หมายเหตุ</label>
            <input
              id="loan-note"
              data-testid="loan-note"
              value={note}
              onChange={(e) => setNote(e.target.value)}
            />
          </div>
          <div style={{ display: "flex", alignItems: "end" }}>
            <button className="btn btn-primary" data-testid="create-loan" type="submit" disabled={busy}>
              {busy ? "กำลังบันทึก…" : "สร้างเงินกู้"}
            </button>
          </div>
        </div>
      </form>

      <div className="card">
        {loans === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : loans.length === 0 ? (
          <div className="empty">ไม่มีรายการเงินกู้</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>พนักงาน</th>
                <th className="num">เงินต้น</th>
                <th className="num">คงเหลือ</th>
                <th className="num">งวดผ่อน</th>
                <th>วันเริ่มผ่อน</th>
                <th>สถานะ</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {loans.map((l) => (
                <tr key={l.id} data-testid="loan-row">
                  <td>
                    {l.employee_code} · {l.employee_name}
                  </td>
                  <td className="num">฿{thb(l.principal)}</td>
                  <td className="num">฿{thb(l.balance)}</td>
                  <td className="num">฿{thb(l.installment_amount)}</td>
                  <td>{thDate(l.start_date)}</td>
                  <td>
                    <span className={`badge ${l.status === "active" ? "warning" : l.status === "paid" ? "success" : ""}`}>
                      {STATUS_LABEL[l.status] ?? l.status}
                    </span>
                  </td>
                  <td className="num">
                    {l.status === "active" && (
                      <button className="btn btn-sm" data-testid="cancel-loan" onClick={() => cancel(l)} disabled={busy}>
                        ยกเลิก
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

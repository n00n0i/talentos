"use client";

import { Fragment, useCallback, useEffect, useState } from "react";
import { gw, handleApiError, thb, thDate, RUN_STATUS } from "@/lib/api";

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
  paid_amount: string;
  installment_amount: string;
  start_date: string;
  status: string;
  note: string | null;
};

type Installment = {
  id: number;
  loan_id: string;
  payroll_run_id: string;
  amount: string;
  created_at: string;
  reversed_at: string | null;
  run_status: string;
  period_start: string;
  period_end: string;
};

const REVERSIBLE_RUN_STATUS = ["draft", "calculating", "pending_approval"];

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
  const [installments, setInstallments] = useState<Record<string, Installment[] | null>>({});

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

  async function payoffLoan(loan: Loan) {
    const input = prompt(
      `จ่ายปิด/จ่ายบางส่วน — ${loan.employee_code} ${loan.employee_name}\nคงเหลือ ฿${thb(loan.balance)}`,
      loan.balance,
    );
    if (input === null) return;
    const amount = Number(input);
    if (!Number.isFinite(amount) || amount <= 0) {
      setError("ระบุจำนวนเงินที่มากกว่า 0");
      return;
    }
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw(`api/v1/loans/${loan.id}/payoff`, {
        method: "POST",
        body: JSON.stringify({ amount, note: null }),
      });
      setNotice("บันทึกการชำระแล้ว");
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function toggleInstallments(loan: Loan) {
    if (installments[loan.id]) {
      setInstallments((prev) => ({ ...prev, [loan.id]: null }));
      return;
    }
    try {
      const r = await gw<{ installments: Installment[] }>(
        `api/v1/loans/${loan.id}/installments`,
      );
      setInstallments((prev) => ({ ...prev, [loan.id]: r.installments }));
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  async function reverseInstallment(loan: Loan, installment: Installment) {
    if (
      !confirm(
        `ยืนยันกลับรายการงวดผ่อนของ ${loan.employee_code} ฿${thb(installment.amount)}?`,
      )
    )
      return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw(
        `api/v1/loans/${loan.id}/installments/${installment.payroll_run_id}/reverse`,
        { method: "POST" },
      );
      setNotice("กลับรายการแล้ว — ต้องคำนวณรอบเงินเดือนใหม่");
      setInstallments((prev) => ({ ...prev, [loan.id]: null }));
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
                <th className="num">ชำระแล้ว</th>
                <th className="num">คงเหลือ</th>
                <th className="num">งวดผ่อน</th>
                <th>วันเริ่มผ่อน</th>
                <th>สถานะ</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {loans.map((l) => (
                <Fragment key={l.id}>
                  <tr data-testid="loan-row">
                    <td>
                      {l.employee_code} · {l.employee_name}
                    </td>
                    <td className="num">฿{thb(l.principal)}</td>
                    <td className="num">฿{thb(l.paid_amount)}</td>
                    <td className="num">฿{thb(l.balance)}</td>
                    <td className="num">฿{thb(l.installment_amount)}</td>
                    <td>{thDate(l.start_date)}</td>
                    <td>
                      <span className={`badge ${l.status === "active" ? "warning" : l.status === "paid" ? "success" : ""}`}>
                        {STATUS_LABEL[l.status] ?? l.status}
                      </span>
                    </td>
                    <td className="num">
                      <button className="btn btn-sm" data-testid="toggle-installments" onClick={() => toggleInstallments(l)} disabled={busy}>
                        {installments[l.id] ? "ซ่อนงวด" : "ดูงวด"}
                      </button>
                      {l.status === "active" && (
                        <button className="btn btn-sm" data-testid="payoff-loan" onClick={() => payoffLoan(l)} disabled={busy}>
                          จ่ายปิด/จ่ายบางส่วน
                        </button>
                      )}
                      {l.status === "active" && (
                        <button className="btn btn-sm" data-testid="cancel-loan" onClick={() => cancel(l)} disabled={busy}>
                          ยกเลิก
                        </button>
                      )}
                    </td>
                  </tr>
                  {installments[l.id] && (
                    <tr data-testid="loan-installments">
                      <td colSpan={8}>
                        {installments[l.id]!.length === 0 ? (
                          <div className="empty">ยังไม่มีงวดผ่อน</div>
                        ) : (
                          <table className="table table-inner">
                            <thead>
                              <tr>
                                <th>รอบเงินเดือน</th>
                                <th>รันไอดี</th>
                                <th className="num">งวด</th>
                                <th>สถานะรอบ</th>
                                <th />
                              </tr>
                            </thead>
                            <tbody>
                              {installments[l.id]!.map((ins) => (
                                <tr key={ins.id}>
                                  <td>{thDate(ins.period_start)} – {thDate(ins.period_end)}</td>
                                  <td className="mono">{ins.payroll_run_id.slice(0, 8)}…</td>
                                  <td className="num">฿{thb(ins.amount)}</td>
                                  <td>
                                    {ins.reversed_at ? (
                                      <span className="badge">กลับรายการแล้ว</span>
                                    ) : (
                                      <span className="badge">{RUN_STATUS[ins.run_status]?.label ?? ins.run_status}</span>
                                    )}
                                  </td>
                                  <td className="num">
                                    {!ins.reversed_at && REVERSIBLE_RUN_STATUS.includes(ins.run_status) && (
                                      <button className="btn btn-sm" data-testid="reverse-installment" onClick={() => reverseInstallment(l, ins)} disabled={busy}>
                                        กลับรายการ
                                      </button>
                                    )}
                                  </td>
                                </tr>
                              ))}
                            </tbody>
                          </table>
                        )}
                      </td>
                    </tr>
                  )}
                </Fragment>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}

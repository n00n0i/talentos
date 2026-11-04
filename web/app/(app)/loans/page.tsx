"use client";

import type { TranslationKey } from "@/lib/i18n-catalog";

import { useI18N, tNow } from "@/lib/i18n";

import ThaiDateInput from "@/components/ThaiDateInput";

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
  interest_rate: string;
  start_date: string;
  status: string;
  note: string | null;
};

type Installment = {
  id: number;
  loan_id: string;
  payroll_run_id: string;
  amount: string;
  principal_amount: string;
  interest_amount: string;
  created_at: string;
  reversed_at: string | null;
  run_status: string;
  period_start: string;
  period_end: string;
};

const REVERSIBLE_RUN_STATUS = ["draft", "calculating", "pending_approval"];

const STATUS_LABEL: Record<string, TranslationKey> = {
  active: "loans.paying-installments",
  paid: "loans.paid-in-full",
  cancelled: "loans.cancel",
};

export default function LoansPage() {
  const { t: tr } = useI18N();
  const [loans, setLoans] = useState<Loan[] | null>(null);
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [employeeId, setEmployeeId] = useState("");
  const [principal, setPrincipal] = useState("");
  const [installment, setInstallment] = useState("");
  const [interestRate, setInterestRate] = useState("");
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
          interest_rate: Number(interestRate || "0") / 100,
          start_date: startDate,
          note: note.trim() || null,
        }),
      });
      setNotice(tr("loans.loan-created"));
      setEmployeeId("");
      setPrincipal("");
      setInstallment("");
      setInterestRate("");
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
    if (!confirm(tr("loans.cancelConfirm", { code: loan.employee_code }))) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/loans/${loan.id}/cancel`, { method: "POST" });
      setNotice(tr("loans.loan-canceled"));
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function payoffLoan(loan: Loan) {
    const input = prompt(
      tr("loans.payoffPrompt", { code: loan.employee_code, name: loan.employee_name, balance: thb(loan.balance) }),
      loan.balance,
    );
    if (input === null) return;
    const amount = Number(input);
    if (!Number.isFinite(amount) || amount <= 0) {
      setError(tr("loans.specify-an-amount-greater-than-0"));
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
      setNotice(tr("loans.payment-recorded"));
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
        tr("loans.reverseConfirm", { code: loan.employee_code, amount: thb(installment.amount) }),
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
      setNotice(tr("loans.the-transaction-has-been-reversed-the"));
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
          <h1>{tr("loans.loans")}</h1>
          <p className="page-sub">{tr("loans.record-employee-loans-and-track-balances")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="badge success" role="status" aria-live="polite">{notice}</p>}

      <form className="card card-pad" onSubmit={submit} style={{ marginBottom: 16 }}>
        <h2 style={{ marginBottom: 14 }}>{tr("loans.create-a-loan")}</h2>
        <div className="form-row" style={{ alignItems: "end" }}>
          <div className="field">
            <label htmlFor="loan-employee">{tr("loans.employee")}</label>
            <select
              id="loan-employee"
              data-testid="loan-employee"
              value={employeeId}
              onChange={(e) => setEmployeeId(e.target.value)}
              required
            >
              <option value="">{tr("loans.choose-employees")}</option>
              {employees.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.employee_code} · {e.first_name_th} {e.last_name_th}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor="loan-principal">{tr("loans.principal-baht")}</label>
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
            <label htmlFor="loan-installment">{tr("loans.installment-per-run-thb")}</label>
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
            <label htmlFor="loan-interest-rate">{tr("loans.monthly-interest")}</label>
            <input
              id="loan-interest-rate"
              data-testid="loan-interest-rate"
              type="number"
              min="0"
              max="10"
              step="0.01"
              placeholder="0"
              value={interestRate}
              onChange={(e) => setInterestRate(e.target.value)}
            />
          </div>
          <div className="field">
            <label htmlFor="loan-start">{tr("loans.installment-start-date")}</label>
            <ThaiDateInput id="loan-start" testId="loan-start" value={startDate} onChange={(v) => setStartDate(v)} required />
          </div>
          <div className="field">
            <label htmlFor="loan-note">{tr("loans.notes")}</label>
            <input
              id="loan-note"
              data-testid="loan-note"
              value={note}
              onChange={(e) => setNote(e.target.value)}
            />
          </div>
          <div style={{ display: "flex", alignItems: "end" }}>
            <button className="btn btn-primary" data-testid="create-loan" type="submit" disabled={busy}>
              {busy ? tr("loans.saving") : tr("loans.create-a-loan")}
            </button>
          </div>
        </div>
      </form>

      <div className="card">
        {loans === null ? (
          <div className="spinner">{tr("loans.loading")}</div>
        ) : loans.length === 0 ? (
          <div className="empty">{tr("loans.there-are-no-loan-items")}</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{tr("loans.employee")}</th>
                <th scope="col" className="num">{tr("loans.principal")}</th>
                <th scope="col" className="num">{tr("loans.paid")}</th>
                <th scope="col" className="num">{tr("loans.remaining")}</th>
                <th scope="col" className="num">{tr("loans.installment")}</th>
                <th scope="col" className="num">{tr("loans.interest")}</th>
                <th scope="col">{tr("loans.installment-start-date")}</th>
                <th scope="col">{tr("loans.status")}</th>
                <th scope="col" />
              </tr>
            </thead>
            <tbody>
              {loans.map((l) => (
                <Fragment key={l.id}>
                  <tr data-testid="loan-row">
                    <td>
                      {l.employee_code} · {l.employee_name}
                    </td>
                    <td className="num">{tr("loans.text")}{thb(l.principal)}</td>
                    <td className="num">{tr("loans.text")}{thb(l.paid_amount)}</td>
                    <td className="num">{tr("loans.text")}{thb(l.balance)}</td>
                    <td className="num">{tr("loans.text")}{thb(l.installment_amount)}</td>
                    <td className="num">{(Number(l.interest_rate) * 100).toFixed(2)}%</td>
                    <td>{thDate(l.start_date)}</td>
                    <td>
                      <span className={`badge ${l.status === "active" ? "warning" : l.status === "paid" ? "success" : ""}`}>
                        {l.status in STATUS_LABEL ? tr(STATUS_LABEL[l.status]) : l.status}
                      </span>
                    </td>
                    <td className="num">
                      <button className="btn btn-sm" data-testid="toggle-installments" onClick={() => toggleInstallments(l)} disabled={busy}>
                        {installments[l.id] ? tr("loans.hide-period") : tr("loans.view-installment")}
                      </button>
                      {l.status === "active" && (
                        <button className="btn btn-sm" data-testid="payoff-loan" onClick={() => payoffLoan(l)} disabled={busy}>
                          {tr("loans.pay-off-pay-in-part")}
                        </button>
                      )}
                      {l.status === "active" && (
                        <button className="btn btn-sm" data-testid="cancel-loan" onClick={() => cancel(l)} disabled={busy}>
                          {tr("loans.cancel")}
                        </button>
                      )}
                    </td>
                  </tr>
                  {installments[l.id] && (
                    <tr data-testid="loan-installments">
                      <td colSpan={9}>
                        {installments[l.id]!.length === 0 ? (
                          <div className="empty">{tr("loans.there-are-no-installments-yet")}</div>
                        ) : (
                          <table className="table table-inner">
                            <thead>
                              <tr>
                                <th scope="col">{tr("loans.payroll-runs")}</th>
                                <th scope="col">{tr("loans.run-id")}</th>
                                <th scope="col" className="num">{tr("loans.period")}</th>
                                <th scope="col" className="num">{tr("loans.principal")}</th>
                                <th scope="col" className="num">{tr("loans.interest")}</th>
                                <th scope="col">{tr("loans.round-status")}</th>
                                <th scope="col" />
                              </tr>
                            </thead>
                            <tbody>
                              {installments[l.id]!.map((ins) => (
                                <tr key={ins.id}>
                                  <td>{thDate(ins.period_start)} – {thDate(ins.period_end)}</td>
                                  <td className="mono">{ins.payroll_run_id.slice(0, 8)}…</td>
                                  <td className="num">{tr("loans.text")}{thb(ins.amount)}</td>
                                  <td className="num">{tr("loans.text")}{thb(ins.principal_amount)}</td>
                                  <td className="num">{tr("loans.text")}{thb(ins.interest_amount)}</td>
                                  <td>
                                    {ins.reversed_at ? (
                                      <span className="badge">{tr("loans.items-have-been-reversed")}</span>
                                    ) : (
                                      <span className="badge">{RUN_STATUS[ins.run_status] ? tr(RUN_STATUS[ins.run_status].labelKey) : ins.run_status}</span>
                                    )}
                                  </td>
                                  <td className="num">
                                    {!ins.reversed_at && REVERSIBLE_RUN_STATUS.includes(ins.run_status) && (
                                      <button className="btn btn-sm" data-testid="reverse-installment" onClick={() => reverseInstallment(l, ins)} disabled={busy}>
                                        {tr("loans.reversal")}
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

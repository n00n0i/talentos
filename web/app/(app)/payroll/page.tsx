"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import {
  gw,
  gql,
  handleApiError,
  PayrollRun,
  Payslip,
  RUN_STATUS,
  thb,
  thDate,
} from "@/lib/api";

function monthDefaults() {
  const now = new Date();
  const start = new Date(now.getFullYear(), now.getMonth(), 1);
  const end = new Date(now.getFullYear(), now.getMonth() + 1, 0);
  const iso = (d: Date) => d.toISOString().slice(0, 10);
  return { start: iso(start), end: iso(end), pay: iso(end) };
}

const RUNS_QUERY =
  "{ payrollRuns(limit: 24) { id periodStart periodEnd paymentDate status totalEmployees totalGross totalNet totalTax totalSso totalPf } }";

export default function PayrollPage() {
  const defaults = useRef(monthDefaults());
  const [runs, setRuns] = useState<PayrollRun[] | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [payslips, setPayslips] = useState<Record<string, Payslip[]>>({});
  const [periodStart, setPeriodStart] = useState(defaults.current.start);
  const [periodEnd, setPeriodEnd] = useState(defaults.current.end);
  const [paymentDate, setPaymentDate] = useState(defaults.current.pay);
  const [entityId, setEntityId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    const r = await gql<{ payrollRuns: PayrollRun[] }>(RUNS_QUERY);
    setRuns(r.payrollRuns);
    return r.payrollRuns;
  }, []);

  useEffect(() => {
    load().catch((e) => handleApiError(e, setError));
  }, [load]);

  // Runs execute in the background; poll while any is still calculating.
  useEffect(() => {
    if (!runs?.some((r) => r.status === "calculating")) return;
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
      setNotice("สร้างรอบเงินเดือนแล้ว — ระบบกำลังคำนวณ");
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
  }

  async function approve(run: PayrollRun) {
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/payroll/runs/${run.id}/approve`, {
        method: "POST",
        body: JSON.stringify({ reason: null }),
      });
      setNotice("อนุมัติรอบเงินเดือนแล้ว");
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
          <h1>รอบเงินเดือน</h1>
          <p className="page-sub">สร้าง ตรวจสอบ และอนุมัติรอบจ่าย</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      <form className="card card-pad" onSubmit={createRun}>
        <h2 style={{ marginBottom: 14 }}>สร้างรอบใหม่</h2>
        <div className="form-row">
          <div className="field">
            <label htmlFor="start">เริ่มงวด</label>
            <input id="start" type="date" value={periodStart} onChange={(e) => setPeriodStart(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="end">สิ้นสุดงวด</label>
            <input id="end" type="date" value={periodEnd} onChange={(e) => setPeriodEnd(e.target.value)} required />
          </div>
          <div className="field">
            <label htmlFor="pay">วันจ่าย</label>
            <input id="pay" type="date" value={paymentDate} onChange={(e) => setPaymentDate(e.target.value)} required />
          </div>
        </div>
        <div className="form-row" style={{ marginTop: 12, gridTemplateColumns: "1fr auto" }}>
          <div className="field">
            <label htmlFor="entity">รหัสนิติบุคคล (ไม่บังคับ)</label>
            <input
              id="entity"
              placeholder="เว้นว่างได้เมื่อโทเคนผูกกับนิติบุคคลเดียว"
              value={entityId}
              onChange={(e) => setEntityId(e.target.value)}
            />
          </div>
          <div style={{ display: "flex", alignItems: "end" }}>
            <button className="btn btn-primary" type="submit" disabled={busy}>
              {busy ? "กำลังสร้าง…" : "สร้างรอบ"}
            </button>
          </div>
        </div>
      </form>

      <div className="card">
        <div className="card-head">
          <h2>รอบทั้งหมด</h2>
          <span className="hint">{runs ? `${runs.length} รอบ` : ""}</span>
        </div>
        {runs === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : runs.length === 0 ? (
          <div className="empty">ยังไม่มีรอบเงินเดือน</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>งวด</th>
                <th>วันจ่าย</th>
                <th>สถานะ</th>
                <th className="num">คน</th>
                <th className="num">ยอดรวม</th>
                <th className="num">สุทธิ</th>
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
                    busy={busy}
                    onToggle={() => toggle(run)}
                    onApprove={() => approve(run)}
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
  busy,
  onToggle,
  onApprove,
}: {
  run: PayrollRun;
  status: { label: string; tone: string };
  open: boolean;
  payslips: Payslip[] | undefined;
  busy: boolean;
  onToggle: () => void;
  onApprove: () => void;
}) {
  return (
    <>
      <tr className="clickable" onClick={onToggle}>
        <td>
          {thDate(run.periodStart)} – {thDate(run.periodEnd)}
        </td>
        <td>{thDate(run.paymentDate)}</td>
        <td>
          <span className={`badge ${status.tone}`}>
            <span className="dot" />
            {status.label}
          </span>
        </td>
        <td className="num">{run.totalEmployees}</td>
        <td className="num">฿{thb(run.totalGross)}</td>
        <td className="num">฿{thb(run.totalNet)}</td>
        <td className="num" onClick={(e) => e.stopPropagation()}>
          {run.status === "pending_approval" && (
            <button className="btn btn-sm btn-primary" disabled={busy} onClick={onApprove}>
              อนุมัติ
            </button>
          )}
        </td>
      </tr>
      {open && (
        <tr>
          <td colSpan={7} style={{ background: "#fbfbfd" }}>
            {!payslips ? (
              <div className="spinner">กำลังโหลดสลิป…</div>
            ) : payslips.length === 0 ? (
              <div className="empty">รอบนี้ยังไม่มีสลิป</div>
            ) : (
              <table className="table">
                <thead>
                  <tr>
                    <th>พนักงาน</th>
                    <th className="num">เงินเดือน</th>
                    <th className="num">ภาษี</th>
                    <th className="num">ประกันสังคม</th>
                    <th className="num">สุทธิ</th>
                    <th>สลิป</th>
                  </tr>
                </thead>
                <tbody>
                  {payslips.map((p) => (
                    <tr key={p.payslip_id}>
                      <td style={{ fontFamily: "monospace", fontSize: 12 }}>{p.employee_id.slice(0, 8)}</td>
                      <td className="num">฿{thb(p.base_salary)}</td>
                      <td className="num">฿{thb(p.tax_withheld)}</td>
                      <td className="num">฿{thb(p.sso_employee)}</td>
                      <td className="num">฿{thb(p.net_pay)}</td>
                      <td>
                        <a href={`/api/gw/api/v1/payroll/payslips/${p.payslip_id}/pdf`} target="_blank">
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

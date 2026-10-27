"use client";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, Me } from "@/lib/api";

type Balance = {
  leave_type: string;
  label_th: string;
  total_days: string;
  used_days: string;
  remaining_days: string;
};
type LeaveRequest = {
  id: string;
  leave_type: string;
  start_date: string;
  end_date: string;
  days: string;
  status: string;
  reason: string | null;
};
type Pending = LeaveRequest & { employee_code: string; employee_name: string };

const STATUS: { [k: string]: { label: string; tone: string } } = {
  pending: { label: "รออนุมัติ", tone: "warning" },
  approved: { label: "อนุมัติแล้ว", tone: "success" },
  rejected: { label: "ไม่อนุมัติ", tone: "danger" },
  cancelled: { label: "ยกเลิกแล้ว", tone: "" },
};

const num = (v: string) => Number(v).toString();

export default function LeavePage() {
  const [isHr, setIsHr] = useState(false);
  const [linked, setLinked] = useState(true);
  const [balances, setBalances] = useState<Balance[]>([]);
  const [requests, setRequests] = useState<LeaveRequest[] | null>(null);
  const [pending, setPending] = useState<Pending[]>([]);
  const [leaveType, setLeaveType] = useState("annual");
  const [start, setStart] = useState("");
  const [end, setEnd] = useState("");
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    const me = await gw<Me>("api/v1/auth/me");
    const hr = me.permissions.includes("leave_request.view_team");
    const [mine, inbox] = await Promise.all([
      gw<{ employee_linked: boolean; balances: Balance[]; requests: LeaveRequest[] }>("api/v1/me/leave"),
      hr ? gw<{ requests: Pending[] }>("api/v1/leave/approvals") : Promise.resolve(null),
    ]);
    setIsHr(hr);
    setLinked(mine.employee_linked);
    setBalances(mine.balances);
    setRequests(mine.requests);
    setPending(inbox?.requests ?? []);
  }, []);

  useEffect(() => {
    load().catch((e) => handleApiError(e, setError));
  }, [load]);

  async function act(done: string, path: string, body?: object) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw(path, { method: "POST", body: body && JSON.stringify(body) });
      setNotice(done);
      await load();
      return true;
    } catch (e) {
      handleApiError(e, setError);
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    const ok = await act("ส่งคำขอลาแล้ว", "api/v1/leave/requests", {
      leave_type: leaveType,
      start_date: start,
      end_date: end,
      reason: reason || undefined,
    });
    if (ok) {
      setStart("");
      setEnd("");
      setReason("");
    }
  }

  const typeLabel = (t: string) => balances.find((b) => b.leave_type === t)?.label_th ?? t;
  const badge = (status: string) => {
    const st = STATUS[status] ?? { label: status, tone: "" };
    return <span className={`badge ${st.tone}`}>{st.label}</span>;
  };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>การลา</h1>
          <p className="page-sub">ยื่นคำขอลา ดูวันลาคงเหลือ และติดตามสถานะ</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      {isHr && (
        <div className="card">
          <div className="card-head">
            <h2>รออนุมัติ</h2>
            <span className="hint">{pending.length} รายการ</span>
          </div>
          {pending.length === 0 ? (
            <div className="empty">ไม่มีคำขอลาที่รออนุมัติ</div>
          ) : (
            <div style={{ overflowX: "auto" }}>
              <table className="table">
                <thead>
                  <tr>
                    <th>พนักงาน</th>
                    <th>ประเภท</th>
                    <th>วันที่</th>
                    <th className="num">วัน</th>
                    <th>เหตุผล</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {pending.map((r) => (
                    <tr key={r.id} data-testid="approval-row">
                      <td>
                        {r.employee_code} · {r.employee_name}
                      </td>
                      <td>{typeLabel(r.leave_type)}</td>
                      <td>
                        {r.start_date} – {r.end_date}
                      </td>
                      <td className="num">{num(r.days)}</td>
                      <td>{r.reason ?? "—"}</td>
                      <td className="num">
                        <span style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                          <button
                            className="btn btn-sm btn-danger"
                            data-testid="reject-leave"
                            disabled={busy}
                            onClick={() =>
                              act("ไม่อนุมัติคำขอลาแล้ว", `api/v1/leave/requests/${r.id}/decision`, { approve: false })
                            }
                          >
                            ไม่อนุมัติ
                          </button>
                          <button
                            className="btn btn-sm btn-primary"
                            data-testid="approve-leave"
                            disabled={busy}
                            onClick={() =>
                              act("อนุมัติคำขอลาแล้ว", `api/v1/leave/requests/${r.id}/decision`, { approve: true })
                            }
                          >
                            อนุมัติ
                          </button>
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {requests === null ? (
        <div className="spinner">กำลังโหลด…</div>
      ) : !linked ? (
        <div className="card">
          <div className="empty">
            ยังไม่ได้ผูกบัญชีนี้กับข้อมูลพนักงาน — ติดต่อ HR เพื่อเชื่อมบัญชี
          </div>
        </div>
      ) : (
        <>
          <div className="grid-4">
            {balances.map((b) => (
              <div key={b.leave_type} className="card card-pad">
                <div className="stat-label">{b.label_th}</div>
                <div className="stat-value">
                  {num(b.remaining_days)} / {num(b.total_days)}
                </div>
                <div className="stat-hint">วันคงเหลือ / วันทั้งหมด</div>
              </div>
            ))}
          </div>

          <form className="card card-pad" onSubmit={submit} style={{ display: "grid", gap: 12 }}>
            <h2>ยื่นคำขอลา</h2>
            <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr 1fr" }}>
              <div className="field">
                <label htmlFor="leave-type">ประเภท</label>
                <select id="leave-type" data-testid="leave-type" value={leaveType} onChange={(e) => setLeaveType(e.target.value)}>
                  {balances.map((b) => (
                    <option key={b.leave_type} value={b.leave_type}>
                      {b.label_th}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label htmlFor="leave-start">เริ่ม</label>
                <input id="leave-start" data-testid="leave-start" type="date" value={start} onChange={(e) => setStart(e.target.value)} required />
              </div>
              <div className="field">
                <label htmlFor="leave-end">ถึง</label>
                <input id="leave-end" data-testid="leave-end" type="date" value={end} onChange={(e) => setEnd(e.target.value)} required />
              </div>
            </div>
            <div className="field">
              <label htmlFor="leave-reason">เหตุผล</label>
              <input id="leave-reason" data-testid="leave-reason" value={reason} onChange={(e) => setReason(e.target.value)} />
            </div>
            <div>
              <button className="btn btn-primary btn-sm" data-testid="request-leave" type="submit" disabled={busy}>
                ส่งคำขอ
              </button>
            </div>
          </form>

          <div className="card">
            <div className="card-head">
              <h2>ประวัติการลา</h2>
            </div>
            {requests.length === 0 ? (
              <div className="empty">ยังไม่มีคำขอลา</div>
            ) : (
              <div style={{ overflowX: "auto" }}>
                <table className="table">
                  <thead>
                    <tr>
                      <th>ประเภท</th>
                      <th>วันที่</th>
                      <th className="num">วัน</th>
                      <th>เหตุผล</th>
                      <th>สถานะ</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {requests.map((r) => (
                      <tr key={r.id} data-testid="leave-row">
                        <td>{typeLabel(r.leave_type)}</td>
                        <td>
                          {r.start_date} – {r.end_date}
                        </td>
                        <td className="num">{num(r.days)}</td>
                        <td>{r.reason ?? "—"}</td>
                        <td>{badge(r.status)}</td>
                        <td className="num">
                          {r.status === "pending" && (
                            <button
                              className="btn btn-sm"
                              data-testid="cancel-leave"
                              disabled={busy}
                              onClick={() => act("ยกเลิกคำขอลาแล้ว", `api/v1/me/leave/${r.id}/cancel`)}
                            >
                              ยกเลิก
                            </button>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        </>
      )}
    </>
  );
}

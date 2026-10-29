"use client";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, Me, thDate } from "@/lib/api";

type OtRequest = {
  id: string;
  work_date: string;
  minutes: number;
  reason: string | null;
  status: string;
};
type Pending = OtRequest & {
  employee_code: string;
  employee_name: string;
  step_no: number;
  total_steps: number;
  approver_role: string;
};

const OT_ERRORS: { [k: string]: string } = {
  "a request already exists for this work date": "มีคำขอ OT สำหรับวันที่นี้อยู่แล้ว",
  "OT request is not in pending status": "คำขอนี้ไม่ได้อยู่ในสถานะรออนุมัติ",
  "OT request cannot be changed in its current status": "คำขอนี้ไม่ได้อยู่ในสถานะรออนุมัติ",
  "minutes must be between 1 and 720": "จำนวนชั่วโมงต้องอยู่ระหว่าง 0.5 ถึง 12 ชั่วโมง",
  "reason must be at most 500 characters": "เหตุผลต้องไม่เกิน 500 ตัวอักษร",
  "OT request not found": "ไม่พบคำขอ OT",
  "not found": "ไม่พบคำขอ OT",
  "employee not found for this tenant": "ไม่พบข้อมูลพนักงาน",
  "no active employee record for this account": "ไม่พบข้อมูลพนักงานที่ใช้งานอยู่สำหรับบัญชีนี้",
};

const thOtError = (message: string) => OT_ERRORS[message] ?? message;

const STATUS: { [k: string]: { label: string; tone: string } } = {
  pending: { label: "รออนุมัติ", tone: "warning" },
  approved: { label: "อนุมัติแล้ว", tone: "success" },
  rejected: { label: "ไม่อนุมัติ", tone: "danger" },
  cancelled: { label: "ยกเลิกแล้ว", tone: "" },
};

const hours = (minutes: number) => String(minutes / 60);

const roleLabel = (role: string) => (role === "manager" ? "หัวหน้างาน" : "HR");

export default function OtPage() {
  const [isHr, setIsHr] = useState(false);
  const [linked, setLinked] = useState(true);
  const [requests, setRequests] = useState<OtRequest[] | null>(null);
  const [pending, setPending] = useState<Pending[]>([]);
  const [history, setHistory] = useState<Pending[]>([]);
  const [date, setDate] = useState("");
  const [hrs, setHrs] = useState("");
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    const me = await gw<Me>("api/v1/auth/me");
    const hr = me.permissions.includes("ot_request.view_team");
    const [mine, inbox, done] = await Promise.all([
      gw<{ employee_linked: boolean; requests: OtRequest[] }>("api/v1/me/ot"),
      hr ? gw<{ requests: Pending[] }>("api/v1/ot/approvals?status=pending") : Promise.resolve(null),
      hr ? gw<{ requests: Pending[] }>("api/v1/ot/approvals?status=approved&limit=50") : Promise.resolve(null),
    ]);
    setIsHr(hr);
    setLinked(mine.employee_linked);
    setRequests(mine.requests);
    setPending(inbox?.requests ?? []);
    setHistory(done?.requests ?? []);
  }, []);

  useEffect(() => {
    load().catch((e) => handleApiError(e, (m) => setError(thOtError(m))));
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
      handleApiError(e, (m) => setError(thOtError(m)));
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    const ok = await act("ส่งคำขอ OT แล้ว", "api/v1/me/ot", {
      work_date: date,
      minutes: Math.round(Number(hrs) * 60),
      reason: reason || undefined,
    });
    if (ok) {
      setDate("");
      setHrs("");
      setReason("");
    }
  }

  const badge = (status: string) => {
    const st = STATUS[status] ?? { label: status, tone: "" };
    return <span className={`badge ${st.tone}`}>{st.label}</span>;
  };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>ขอ OT</h1>
          <p className="page-sub">
            OT ที่อนุมัติเป็นเพียงบันทึกการอนุญาต — การคิดเงินเดือนใช้เวลาลงเวลาจริงเท่านั้น
          </p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      {isHr && (
        <div className="card">
          <div className="card-head">
            <h2>รออนุมัติ OT</h2>
            <span className="hint">{pending.length} รายการ</span>
          </div>
          {pending.length === 0 ? (
            <div className="empty">ไม่มีคำขอ OT ที่รออนุมัติ</div>
          ) : (
            <div style={{ overflowX: "auto" }}>
              <table className="table">
                <thead>
                  <tr>
                    <th>พนักงาน</th>
                    <th>วันที่</th>
                    <th className="num">ชั่วโมง</th>
                    <th>เหตุผล</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {pending.map((r) => (
                    <tr key={r.id} data-testid="ot-approval-row">
                      <td>
                        {r.employee_code} · {r.employee_name}
                        <span className="hint" data-testid="ot-step">
                          ขั้น {r.step_no}/{r.total_steps} · {roleLabel(r.approver_role)}
                        </span>
                      </td>
                      <td>{thDate(r.work_date)}</td>
                      <td className="num">{hours(r.minutes)}</td>
                      <td>{r.reason ?? "—"}</td>
                      <td className="num">
                        <span style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                          <button
                            className="btn btn-sm btn-danger"
                            data-testid="reject-ot"
                            disabled={busy}
                            onClick={() =>
                              act("ไม่อนุมัติคำขอ OT แล้ว", `api/v1/ot/requests/${r.id}/decision`, { approve: false })
                            }
                          >
                            ไม่อนุมัติ
                          </button>
                          <button
                            className="btn btn-sm btn-primary"
                            data-testid="approve-ot"
                            disabled={busy}
                            onClick={() =>
                              act("อนุมัติคำขอ OT แล้ว", `api/v1/ot/requests/${r.id}/decision`, { approve: true })
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

      {isHr && (
        <div className="card">
          <div className="card-head">
            <h2>ประวัติ OT ที่อนุมัติแล้ว</h2>
            <span className="hint">ล่าสุด {history.length} รายการ</span>
          </div>
          {history.length === 0 ? (
            <div className="empty">ยังไม่มี OT ที่อนุมัติแล้ว</div>
          ) : (
            <div style={{ overflowX: "auto" }}>
              <table className="table">
                <thead>
                  <tr>
                    <th>พนักงาน</th>
                    <th>วันที่</th>
                    <th className="num">ชั่วโมง</th>
                    <th>เหตุผล</th>
                    <th>สถานะ</th>
                  </tr>
                </thead>
                <tbody>
                  {history.map((r) => (
                    <tr key={r.id} data-testid="ot-history-row">
                      <td>
                        {r.employee_code} · {r.employee_name}
                      </td>
                      <td>{thDate(r.work_date)}</td>
                      <td className="num">{hours(r.minutes)}</td>
                      <td>{r.reason ?? "—"}</td>
                      <td>{badge(r.status)}</td>
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
          <form className="card card-pad" onSubmit={submit} style={{ display: "grid", gap: 12 }}>
            <h2>ยื่นคำขอ OT</h2>
            <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
              <div className="field">
                <label htmlFor="ot-date">วันที่ทำ OT</label>
                <input id="ot-date" data-testid="ot-date" type="date" value={date} onChange={(e) => setDate(e.target.value)} required />
              </div>
              <div className="field">
                <label htmlFor="ot-hours">ชั่วโมง</label>
                <input
                  id="ot-hours"
                  data-testid="ot-hours"
                  type="number"
                  step={0.5}
                  min={0.5}
                  max={12}
                  value={hrs}
                  onChange={(e) => setHrs(e.target.value)}
                  required
                />
              </div>
            </div>
            <div className="field">
              <label htmlFor="ot-reason">เหตุผล</label>
              <input id="ot-reason" data-testid="ot-reason" maxLength={500} value={reason} onChange={(e) => setReason(e.target.value)} />
              <span className="hint">ไม่เกิน 500 ตัวอักษร</span>
            </div>
            <div>
              <button className="btn btn-primary btn-sm" data-testid="request-ot" type="submit" disabled={busy}>
                ส่งคำขอ
              </button>
            </div>
          </form>

          <div className="card">
            <div className="card-head">
              <h2>ประวัติคำขอ OT</h2>
            </div>
            {requests.length === 0 ? (
              <div className="empty">ยังไม่มีคำขอ OT</div>
            ) : (
              <div style={{ overflowX: "auto" }}>
                <table className="table">
                  <thead>
                    <tr>
                      <th>วันที่</th>
                      <th className="num">ชั่วโมง</th>
                      <th>เหตุผล</th>
                      <th>สถานะ</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {requests.map((r) => (
                      <tr key={r.id} data-testid="ot-row">
                        <td>{thDate(r.work_date)}</td>
                        <td className="num">{hours(r.minutes)}</td>
                        <td>{r.reason ?? "—"}</td>
                        <td>{badge(r.status)}</td>
                        <td className="num">
                          {r.status === "pending" && (
                            <button
                              className="btn btn-sm"
                              data-testid="cancel-ot"
                              disabled={busy}
                              onClick={() => act("ยกเลิกคำขอ OT แล้ว", `api/v1/me/ot/${r.id}/cancel`)}
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

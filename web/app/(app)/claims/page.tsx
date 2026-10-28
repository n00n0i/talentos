"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { gw, handleApiError, Me, thb, thDate } from "@/lib/api";

type Claim = {
  id: string;
  category: string;
  amount: string;
  expense_date: string;
  description: string | null;
  status: string;
  has_receipt: boolean;
};

type Pending = Claim & {
  employee_id: string;
  employee_code: string;
  employee_name: string;
};

const CATEGORIES: { key: string; label: string }[] = [
  { key: "travel", label: "เดินทาง" },
  { key: "meal", label: "อาหาร" },
  { key: "supplies", label: "เครื่องใช้สำนักงาน" },
  { key: "other", label: "อื่น ๆ" },
];

const CLAIM_ERRORS: { [k: string]: string } = {
  "category must be one of: travel, meal, supplies, other": "เลือกประเภทค่าใช้จ่ายไม่ถูกต้อง",
  "amount must be greater than 0": "จำนวนเงินต้องมากกว่า 0",
  "expense_date is required": "ระบุวันที่ค่าใช้จ่าย",
  "description must be at most 500 characters": "รายละเอียดต้องไม่เกิน 500 ตัวอักษร",
  "expense claim cannot be cancelled in its current status": "ไม่สามารถยกเลิกรายการนี้ได้",
  "expense claim cannot be changed in its current status": "รายการนี้ไม่ได้อยู่ในสถานะรออนุมัติ",
  "expense claim is not in pending status": "รายการนี้ไม่ได้อยู่ในสถานะรออนุมัติ",
  "expense claim not found": "ไม่พบรายการค่าใช้จ่าย",
  "not found": "ไม่พบรายการค่าใช้จ่าย",
  "no active employee record for this account": "ไม่พบข้อมูลพนักงานที่ใช้งานอยู่สำหรับบัญชีนี้",
  "receipt is too large; maximum size is 5 MB": "ไฟล์ใหญ่เกิน 5 MB",
  "receipt type must be image/jpeg, image/png, or application/pdf": "รองรับเฉพาะ JPG, PNG หรือ PDF",
  "receipt already uploaded for this claim": "แนบใบเสร็จแล้ว",
};

const thClaimError = (message: string) => CLAIM_ERRORS[message] ?? message;

const STATUS: { [k: string]: { label: string; tone: string } } = {
  pending: { label: "รออนุมัติ", tone: "warning" },
  approved: { label: "อนุมัติแล้ว", tone: "success" },
  rejected: { label: "ไม่อนุมัติ", tone: "danger" },
  cancelled: { label: "ยกเลิกแล้ว", tone: "" },
  paid: { label: "จ่ายแล้ว", tone: "success" },
};

const categoryLabel = (key: string) => CATEGORIES.find((c) => c.key === key)?.label ?? key;

export default function ClaimsPage() {
  const [isHr, setIsHr] = useState(false);
  const [linked, setLinked] = useState(true);
  const [claims, setClaims] = useState<Claim[] | null>(null);
  const [pending, setPending] = useState<Pending[]>([]);
  const [category, setCategory] = useState("meal");
  const [amount, setAmount] = useState("");
  const [date, setDate] = useState("");
  const [description, setDescription] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const receiptRef = useRef<HTMLInputElement>(null);

  const load = useCallback(async () => {
    const me = await gw<Me>("api/v1/auth/me");
    const hr = me.permissions.includes("claim.view_team");
    const [mine, inbox] = await Promise.all([
      gw<{ employee_linked: boolean; claims: Claim[] }>("api/v1/me/claims"),
      hr ? gw<{ claims: Pending[] }>("api/v1/claims/approvals") : Promise.resolve(null),
    ]);
    setIsHr(hr);
    setLinked(mine.employee_linked);
    setClaims(mine.claims);
    setPending(inbox?.claims ?? []);
  }, []);

  useEffect(() => {
    load().catch((e) => handleApiError(e, (m) => setError(thClaimError(m))));
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
      handleApiError(e, (m) => setError(thClaimError(m)));
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function uploadReceipt(claimId: string, f: File) {
    const res = await fetch(`/api/gw/api/v1/me/claims/${claimId}/receipt`, {
      method: "POST",
      headers: { "content-type": f.type || "application/octet-stream" },
      body: await f.arrayBuffer(),
      cache: "no-store",
    });
    if (!res.ok) {
      let message = res.statusText || `HTTP ${res.status}`;
      try {
        const body = await res.json();
        message = body.message || body.error || message;
      } catch {
        // non-JSON error body
      }
      throw new Error(thClaimError(message));
    }
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const claim = await gw<Claim>("api/v1/me/claims", {
        method: "POST",
        body: JSON.stringify({
          category,
          amount: Number(amount),
          expense_date: date,
          description: description || undefined,
        }),
      });
      if (file) {
        await uploadReceipt(claim.id, file);
        setNotice("ส่งคำขอเบิกค่าใช้จ่ายและแนบใบเสร็จแล้ว");
      } else {
        setNotice("ส่งคำขอเบิกค่าใช้จ่ายแล้ว");
      }
      setCategory("meal");
      setAmount("");
      setDate("");
      setDescription("");
      setFile(null);
      if (receiptRef.current) receiptRef.current.value = "";
      await load();
    } catch (e) {
      handleApiError(e, (m) => setError(thClaimError(m)));
    } finally {
      setBusy(false);
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
          <h1>ค่าใช้จ่าย</h1>
          <p className="page-sub">ยื่นคำขอเบิกค่าใช้จ่าย พร้อมใบเสร็จ และติดตามสถานะ</p>
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
            <div className="empty">ไม่มีคำขอเบิกค่าใช้จ่ายที่รออนุมัติ</div>
          ) : (
            <div style={{ overflowX: "auto" }}>
              <table className="table">
                <thead>
                  <tr>
                    <th>พนักงาน</th>
                    <th>ประเภท</th>
                    <th>วันที่</th>
                    <th className="num">จำนวนเงิน</th>
                    <th>รายละเอียด</th>
                    <th>ใบเสร็จ</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {pending.map((r) => (
                    <tr key={r.id} data-testid="claim-approval-row">
                      <td>
                        {r.employee_code} · {r.employee_name}
                      </td>
                      <td>{categoryLabel(r.category)}</td>
                      <td>{thDate(r.expense_date)}</td>
                      <td className="num">{thb(r.amount)}</td>
                      <td>{r.description ?? "—"}</td>
                      <td>
                        {r.has_receipt ? (
                          <a href={`/api/gw/api/v1/claims/${r.id}/receipt`} target="_blank" rel="noreferrer">
                            ดูใบเสร็จ
                          </a>
                        ) : (
                          <span className="hint">ไม่มี</span>
                        )}
                      </td>
                      <td className="num">
                        <span style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                          <button
                            className="btn btn-sm btn-danger"
                            data-testid="reject-claim"
                            disabled={busy}
                            onClick={() =>
                              act("ไม่อนุมัติคำขอเบิกค่าใช้จ่ายแล้ว", `api/v1/claims/${r.id}/decision`, { approve: false })
                            }
                          >
                            ไม่อนุมัติ
                          </button>
                          <button
                            className="btn btn-sm btn-primary"
                            data-testid="approve-claim"
                            disabled={busy}
                            onClick={() =>
                              act("อนุมัติคำขอเบิกค่าใช้จ่ายแล้ว", `api/v1/claims/${r.id}/decision`, { approve: true })
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

      {claims === null ? (
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
            <h2>ยื่นคำขอเบิกค่าใช้จ่าย</h2>
            <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr 1fr" }}>
              <div className="field">
                <label htmlFor="claim-category">ประเภท</label>
                <select
                  id="claim-category"
                  data-testid="claim-category"
                  value={category}
                  onChange={(e) => setCategory(e.target.value)}
                >
                  {CATEGORIES.map((c) => (
                    <option key={c.key} value={c.key}>
                      {c.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label htmlFor="claim-amount">จำนวนเงิน</label>
                <input
                  id="claim-amount"
                  data-testid="claim-amount"
                  type="number"
                  step={0.01}
                  min={0.01}
                  value={amount}
                  onChange={(e) => setAmount(e.target.value)}
                  required
                />
              </div>
              <div className="field">
                <label htmlFor="claim-date">วันที่ค่าใช้จ่าย</label>
                <input
                  id="claim-date"
                  data-testid="claim-date"
                  type="date"
                  value={date}
                  onChange={(e) => setDate(e.target.value)}
                  required
                />
              </div>
            </div>
            <div className="field">
              <label htmlFor="claim-description">รายละเอียด</label>
              <input
                id="claim-description"
                data-testid="claim-description"
                maxLength={500}
                value={description}
                onChange={(e) => setDescription(e.target.value)}
              />
              <span className="hint">ไม่เกิน 500 ตัวอักษร</span>
            </div>
            <div className="field">
              <label htmlFor="claim-receipt">ใบเสร็จ</label>
              <input
                id="claim-receipt"
                ref={receiptRef}
                data-testid="claim-receipt"
                type="file"
                accept="image/jpeg,image/png,application/pdf"
                onChange={(e) => setFile(e.target.files?.[0] ?? null)}
              />
              <span className="hint">JPG, PNG หรือ PDF ไม่เกิน 5 MB</span>
            </div>
            <div>
              <button className="btn btn-primary btn-sm" data-testid="submit-claim" type="submit" disabled={busy}>
                ส่งคำขอ
              </button>
            </div>
          </form>

          <div className="card">
            <div className="card-head">
              <h2>ประวัติค่าใช้จ่าย</h2>
            </div>
            {claims.length === 0 ? (
              <div className="empty">ยังไม่มีคำขอเบิกค่าใช้จ่าย</div>
            ) : (
              <div style={{ overflowX: "auto" }}>
                <table className="table">
                  <thead>
                    <tr>
                      <th>ประเภท</th>
                      <th>วันที่</th>
                      <th className="num">จำนวนเงิน</th>
                      <th>รายละเอียด</th>
                      <th>สถานะ</th>
                      <th>ใบเสร็จ</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {claims.map((r) => (
                      <tr key={r.id} data-testid="claim-row">
                        <td>{categoryLabel(r.category)}</td>
                        <td>{thDate(r.expense_date)}</td>
                        <td className="num">{thb(r.amount)}</td>
                        <td>{r.description ?? "—"}</td>
                        <td>{badge(r.status)}</td>
                        <td>
                          {r.has_receipt ? (
                            <a href={`/api/gw/api/v1/me/claims/${r.id}/receipt`} target="_blank" rel="noreferrer">
                              ดูใบเสร็จ
                            </a>
                          ) : (
                            <span className="hint">ไม่มี</span>
                          )}
                        </td>
                        <td className="num">
                          {r.status === "pending" && (
                            <button
                              className="btn btn-sm"
                              data-testid="cancel-claim"
                              disabled={busy}
                              onClick={() => act("ยกเลิกคำขอเบิกค่าใช้จ่ายแล้ว", `api/v1/me/claims/${r.id}/cancel`)}
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

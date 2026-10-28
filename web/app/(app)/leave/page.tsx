"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { gw, handleApiError, Me, thDate } from "@/lib/api";

type Balance = {
  leave_type: string;
  label_th: string;
  total_days: string;
  used_days: string;
  remaining_days: string;
  provisioned: boolean;
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
type Pending = LeaveRequest & {
  employee_code: string;
  employee_name: string;
  label_th: string;
  step_no: number;
  total_steps: number;
  approver_role: string;
};

type BalanceImportRow = Record<string, string>;
type MeLeaveResponse = {
  employee_linked: boolean;
  balances: Balance[];
  requests: LeaveRequest[];
  has_more: boolean;
};

type BalanceImportResult = {
  dry_run: boolean;
  total: number;
  valid: number;
  invalid: number;
  created: number;
  rows: {
    row: number;
    employee_code: string;
    leave_type: string;
    year: number;
    status: "valid" | "invalid";
    errors: string[];
  }[];
};

const BALANCE_IMPORT_REQUIRED = ["employee_code", "leave_type", "year", "total_days"];
const BALANCE_IMPORT_OPTIONAL = ["used_days"];
const BALANCE_IMPORT_MAX_BYTES = 2 * 1024 * 1024;
const BALANCE_IMPORT_MAX_ROWS = 1000;

const STATUS: { [k: string]: { label: string; tone: string } } = {
  pending: { label: "รออนุมัติ", tone: "warning" },
  approved: { label: "อนุมัติแล้ว", tone: "success" },
  rejected: { label: "ไม่อนุมัติ", tone: "danger" },
  cancelled: { label: "ยกเลิกแล้ว", tone: "" },
};

const num = (v: string) => Number(v).toString();

function parseCsv(text: string): string[][] {
  const out: string[][] = [];
  let row: string[] = [];
  let cell = "";
  let quoted = false;
  let closed = false;
  let quoteRec = 0;
  const where = (rec: number) => (rec === 0 ? "แถวหัวตาราง" : `แถว ${rec}`);
  const bad = (rec: number) =>
    new Error(`ไฟล์ CSV ไม่ถูกต้อง: เครื่องหมายอัญประกาศผิดรูปแบบที่${where(rec)}`);
  const src = text.replace(/^\uFEFF/, "");
  for (let i = 0; i < src.length; i++) {
    const c = src[i];
    if (quoted) {
      if (c === '"' && src[i + 1] === '"') {
        cell += '"';
        i++;
      } else if (c === '"') {
        quoted = false;
        closed = true;
      } else cell += c;
    } else if (c === '"') {
      if (closed || cell.trim() !== "") throw bad(out.length);
      quoted = true;
      quoteRec = out.length;
      cell = "";
    } else if (c === ",") {
      row.push(cell);
      cell = "";
      closed = false;
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && src[i + 1] === "\n") i++;
      row.push(cell);
      cell = "";
      closed = false;
      out.push(row);
      row = [];
    } else if (closed && c !== " " && c !== "\t") throw bad(out.length);
    else if (!closed) cell += c;
  }
  if (quoted) throw bad(quoteRec);
  if (cell !== "" || row.length || closed) {
    row.push(cell);
    out.push(row);
  }
  return out.filter((r) => r.some((c) => c.trim() !== ""));
}

const BALANCE_IMPORT_ERRORS: [RegExp, (m: RegExpMatchArray) => string][] = [
  [/^employee_code is required$/, () => "employee_code จำเป็นต้องระบุ"],
  [/^leave_type is required$/, () => "leave_type จำเป็นต้องระบุ"],
  [/^year is required$/, () => "year จำเป็นต้องระบุ"],
  [/^total_days is required$/, () => "total_days จำเป็นต้องระบุ"],
  [/(\w+) must be a number$/, (m) => `${m[1]} ต้องเป็นตัวเลข`],
  [/(\w+) must be an integer$/, (m) => `${m[1]} ต้องเป็นจำนวนเต็ม`],
  [/(\w+) must be >= 0$/, (m) => `${m[1]} ต้องไม่ติดลบ`],
  [/(\w+) must be <= 366$/, (m) => `${m[1]} ต้องไม่เกิน 366`],
  [/^year must be between 2000 and 2100$/, () => "year ต้องอยู่ระหว่าง 2000 ถึง 2100"],
  [/^used_days must not exceed total_days$/, () => "used_days ต้องไม่เกิน total_days"],
  [/^employee_code (.*) not found$/, (m) => `ไม่พบรหัสพนักงาน ${m[1]}`],
  [/^leave_type (.*) not found$/, (m) => `ไม่พบประเภทการลา ${m[1]}`],
  [/^duplicate row for \((.*), (.*), (.*)\) in file$/, (m) => `แถวซ้ำกัน (${m[1]}, ${m[2]}, ${m[3]})`],
];

function thBalanceImportError(message: string): string {
  for (const [re, f] of BALANCE_IMPORT_ERRORS) {
    const m = message.match(re);
    if (m) return f(m);
  }
  return message;
}

function csvToBalanceImportRows(text: string): BalanceImportRow[] {
  const [header, ...body] = parseCsv(text);
  if (!header) throw new Error("ไฟล์ว่างเปล่า");
  const cols = header.map((h) => h.trim());
  const missing = BALANCE_IMPORT_REQUIRED.filter((h) => !cols.includes(h));
  if (missing.length) throw new Error(`ไม่พบคอลัมน์ที่จำเป็น: ${missing.join(", ")}`);
  if (body.length === 0) throw new Error("ไม่มีข้อมูลในไฟล์");
  if (body.length > BALANCE_IMPORT_MAX_ROWS)
    throw new Error(`นำเข้าได้ไม่เกิน ${BALANCE_IMPORT_MAX_ROWS} แถวต่อไฟล์`);
  return body.map((cells, i) => {
    if (cells.length !== cols.length)
      throw new Error(`แถว ${i + 1}: จำนวนคอลัมน์ (${cells.length}) ไม่ตรงกับหัวตาราง (${cols.length})`);
    const r: BalanceImportRow = {};
    for (const key of [...BALANCE_IMPORT_REQUIRED, ...BALANCE_IMPORT_OPTIONAL]) {
      const idx = cols.indexOf(key);
      const v = idx < 0 ? "" : (cells[idx] ?? "").trim();
      if (v === "") continue;
      r[key] = v;
    }
    return r;
  });
}

const ROLE_LABEL: { [k: string]: string } = {
  manager: "หัวหน้างาน",
  hr_admin: "HR",
};

export default function LeavePage() {
  const [isHr, setIsHr] = useState(false);
  const [linked, setLinked] = useState(true);
  const [balances, setBalances] = useState<Balance[]>([]);
  const [requests, setRequests] = useState<LeaveRequest[] | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [pending, setPending] = useState<Pending[]>([]);
  const [leaveType, setLeaveType] = useState("annual");
  const [start, setStart] = useState("");
  const [end, setEnd] = useState("");
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [importRows, setImportRows] = useState<BalanceImportRow[]>([]);
  const [importResult, setImportResult] = useState<BalanceImportResult | null>(null);
  const [importKey, setImportKey] = useState(0);
  const importGen = useRef(0);

  const load = useCallback(async () => {
    const me = await gw<Me>("api/v1/auth/me");
    const hr = me.permissions.includes("leave_request.view_team");
    const [mine, inbox] = await Promise.all([
      gw<MeLeaveResponse>("api/v1/me/leave?limit=50&offset=0"),
      hr ? gw<{ requests: Pending[] }>("api/v1/leave/approvals") : Promise.resolve(null),
    ]);
    setIsHr(hr);
    setLinked(mine.employee_linked);
    setBalances(mine.balances);
    setRequests(mine.requests);
    setHasMore(mine.has_more);
    setPending(inbox?.requests ?? []);
  }, []);

  useEffect(() => {
    load().catch((e) => handleApiError(e, setError));
  }, [load]);

  async function onImportFile(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    const gen = ++importGen.current;
    setImportResult(null);
    setImportRows([]);
    setError("");
    if (!file) return;
    if (file.size > BALANCE_IMPORT_MAX_BYTES) {
      setError("ไฟล์ใหญ่เกิน 2 MB");
      return;
    }
    try {
      const parsed = csvToBalanceImportRows(await file.text());
      if (gen === importGen.current) setImportRows(parsed);
    } catch (err) {
      if (gen === importGen.current)
        setError(err instanceof Error ? err.message : "อ่านไฟล์ไม่สำเร็จ");
    }
  }

  async function runBalanceImport(dryRun: boolean) {
    const gen = importGen.current;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const r = await gw<BalanceImportResult>("api/v1/leave/balances/import", {
        method: "POST",
        body: JSON.stringify({ dry_run: dryRun, rows: importRows }),
      });
      if (gen !== importGen.current) return;
      if (dryRun) {
        setImportResult(r);
      } else if (r.invalid > 0 || r.created !== r.valid) {
        setImportResult(r);
        setError("ไม่มีการนำเข้าโควตา เนื่องจากข้อมูลบางแถวไม่ถูกต้อง กรุณาแก้ไขไฟล์แล้วลองใหม่");
      } else {
        setNotice(`นำเข้าโควตาการลา ${r.created} รายการแล้ว`);
        setImportRows([]);
        setImportResult(null);
        setImportKey((k) => k + 1);
        await load();
      }
    } catch (err) {
      if (gen === importGen.current) handleApiError(err, setError);
    } finally {
      setBusy(false);
    }
  }

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

  async function loadMore() {
    if (requests === null || !hasMore) return;
    setBusy(true);
    setError("");
    try {
      const r = await gw<MeLeaveResponse>(`api/v1/me/leave?limit=50&offset=${requests.length}`);
      setRequests((prev) => [...(prev ?? []), ...r.requests]);
      setHasMore(r.has_more);
    } catch (err) {
      handleApiError(err, setError);
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
                    <th>ขั้นตอน</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {pending.map((r) => (
                    <tr key={r.id} data-testid="approval-row">
                      <td>
                        {r.employee_code} · {r.employee_name}
                      </td>
                      <td>{r.label_th || typeLabel(r.leave_type)}</td>
                      <td>
                        {thDate(r.start_date)} – {thDate(r.end_date)}
                      </td>
                      <td className="num">{num(r.days)}</td>
                      <td>{r.reason ?? "—"}</td>
                      <td data-testid="leave-step">
                        ขั้น {r.step_no}/{r.total_steps} · {ROLE_LABEL[r.approver_role] ?? r.approver_role}
                      </td>
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

      {isHr && (
        <div className="card card-pad" style={{ display: "grid", gap: 12 }}>
          <h2>นำเข้าโควตาการลา (CSV)</h2>
          <p className="hint">
            คอลัมน์: {BALANCE_IMPORT_REQUIRED.join(", ")}, {BALANCE_IMPORT_OPTIONAL.join(", ")} (ไฟล์ไม่เกิน 2 MB และ{" "}
            {BALANCE_IMPORT_MAX_ROWS} แถว)
          </p>
          <label htmlFor="import-balance-csv">เลือกไฟล์ CSV โควตาการลา</label>
          <input
            key={importKey}
            id="import-balance-csv"
            type="file"
            accept=".csv"
            data-testid="import-balance-csv"
            onChange={onImportFile}
            disabled={busy}
          />
          {importRows.length > 0 && (
            <>
              <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                <button
                  className="btn"
                  data-testid="dry-run-balance-import"
                  onClick={() => runBalanceImport(true)}
                  disabled={busy}
                >
                  ตรวจสอบไฟล์
                </button>
                <button
                  className="btn btn-primary"
                  data-testid="confirm-balance-import"
                  onClick={() => runBalanceImport(false)}
                  disabled={busy || !importResult || importResult.valid === 0 || importResult.invalid > 0}
                >
                  ยืนยันนำเข้า
                </button>
                {importResult && (
                  <span data-testid="import-summary">
                    ทั้งหมด {importResult.total} · ถูกต้อง {importResult.valid} · ผิดพลาด {importResult.invalid}
                  </span>
                )}
              </div>
              <table className="table">
                <thead>
                  <tr>
                    <th>แถว</th>
                    <th>รหัสพนักงาน</th>
                    <th>ประเภท</th>
                    <th className="num">ปี</th>
                    <th className="num">วันทั้งหมด</th>
                    <th className="num">วันที่ใช้</th>
                    <th>สถานะ</th>
                  </tr>
                </thead>
                <tbody>
                  {importRows.map((r, i) => {
                    const res = importResult?.rows[i];
                    return (
                      <tr key={i} data-testid="import-row">
                        <td>{i + 1}</td>
                        <td>{String(r.employee_code ?? "")}</td>
                        <td>{String(r.leave_type ?? "")}</td>
                        <td className="num">{String(r.year ?? "")}</td>
                        <td className="num">{String(r.total_days ?? "")}</td>
                        <td className="num">{String(r.used_days ?? "")}</td>
                        <td>
                          {res &&
                            (res.status === "valid" ? (
                              <span className="badge success">ถูกต้อง</span>
                            ) : (
                              <span className="error">{res.errors.map(thBalanceImportError).join(" · ")}</span>
                            ))}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </>
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
                {b.provisioned ? (
                  <>
                    <div className="stat-value">
                      {num(b.remaining_days)} / {num(b.total_days)}
                    </div>
                    <div className="stat-hint">วันคงเหลือ / วันทั้งหมด</div>
                  </>
                ) : (
                  <>
                    <div className="stat-value" style={{ fontSize: 18 }}>ยังไม่ตั้งโควตา</div>
                    <div className="stat-hint">ให้ HR ตั้งโควตาก่อน เพื่อติดตามวันคงเหลือ</div>
                  </>
                )}
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
                          {thDate(r.start_date)} – {thDate(r.end_date)}
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
            {hasMore && (
              <div style={{ padding: 12, textAlign: "center" }}>
                <button
                  className="btn"
                  data-testid="load-more-leave"
                  onClick={loadMore}
                  disabled={busy}
                >
                  โหลดเพิ่ม
                </button>
              </div>
            )}
          </div>
        </>
      )}
    </>
  );
}

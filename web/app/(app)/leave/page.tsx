"use client";

import type { TranslationKey } from "@/lib/i18n-catalog";

import { useI18N, tNow } from "@/lib/i18n";

import ThaiDateInput from "@/components/ThaiDateInput";

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

type Candidate = { id: string; email: string; full_name_th: string | null };
type Delegation = {
  id: string;
  delegator_user_id: string;
  delegate_user_id: string;
  delegate_email: string;
  delegate_name: string | null;
  starts_on: string;
  ends_on: string;
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

const STATUS: Record<string, { labelKey: TranslationKey; tone: string }> = {
  pending: { labelKey: "leave.pending-approval", tone: "warning" },
  approved: { labelKey: "leave.approved", tone: "success" },
  rejected: { labelKey: "leave.reject", tone: "danger" },
  cancelled: { labelKey: "leave.canceled", tone: "" },
};

const num = (v: string) => Number(v).toString();

function parseCsv(text: string): string[][] {
  const out: string[][] = [];
  let row: string[] = [];
  let cell = "";
  let quoted = false;
  let closed = false;
  let quoteRec = 0;
  const where = (rec: number) => (rec === 0 ? tNow("leave.head-row") : tNow("leave.row", { rec }));
  const bad = (rec: number) =>
    new Error(tNow("leave.badCsvQuote", { where: where(rec) }));
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
  [/^employee_code is required$/, () => tNow("leave.employee-code-required")],
  [/^leave_type is required$/, () => tNow("leave.leave-type-required")],
  [/^year is required$/, () => tNow("leave.year-required")],
  [/^total_days is required$/, () => tNow("leave.total-days-required")],
  [/(\w+) must be a number$/, (m) => tNow("leave.fieldNumber", { field: m[1] })],
  [/(\w+) must be an integer$/, (m) => tNow("leave.fieldInteger", { field: m[1] })],
  [/(\w+) must be >= 0$/, (m) => tNow("leave.fieldNonNegative", { field: m[1] })],
  [/(\w+) must be <= 366$/, (m) => tNow("leave.fieldMax366", { field: m[1] })],
  [/^year must be between 2000 and 2100$/, () => tNow("leave.year-must-be-between-2000-and")],
  [/^used_days must not exceed total_days$/, () => tNow("leave.used-days-must-not-exceed-total")],
  [/^employee_code (.*) not found$/, (m) => tNow("leave.employeeCodeNotFound", { code: m[1] })],
  [/^leave_type (.*) not found$/, (m) => tNow("leave.leaveTypeNotFound", { type: m[1] })],
  [/^duplicate row for \((.*), (.*), (.*)\) in file$/, (m) => tNow("leave.duplicateRow", { a: m[1], b: m[2], c: m[3] })],
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
  if (!header) throw new Error(tNow("leave.the-file-is-empty"));
  const cols = header.map((h) => h.trim());
  const missing = BALANCE_IMPORT_REQUIRED.filter((h) => !cols.includes(h));
  if (missing.length) throw new Error(tNow("leave.missingColumns", { columns: missing.join(", ") }));
  if (body.length === 0) throw new Error(tNow("leave.there-is-no-data-in-the"));
  if (body.length > BALANCE_IMPORT_MAX_ROWS)
    throw new Error(tNow("leave.maxRows", { max: BALANCE_IMPORT_MAX_ROWS }));
  return body.map((cells, i) => {
    if (cells.length !== cols.length)
      throw new Error(tNow("leave.columnMismatch", { row: i + 1, got: cells.length, expected: cols.length }));
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

const ROLE_LABEL: Record<string, import("@/lib/i18n-catalog").TranslationKey> = {
  manager: "leave.manager",
  hr_admin: "leave.hrRole",
};

export default function LeavePage() {
  const { t: tr } = useI18N();
  const [isHr, setIsHr] = useState(false);
  const [linked, setLinked] = useState(true);
  const [balances, setBalances] = useState<Balance[]>([]);
  const [requests, setRequests] = useState<LeaveRequest[] | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [pending, setPending] = useState<Pending[]>([]);
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [delegations, setDelegations] = useState<Delegation[]>([]);
  const [delegateUser, setDelegateUser] = useState("");
  const [delegateStart, setDelegateStart] = useState("");
  const [delegateEnd, setDelegateEnd] = useState("");
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
    const [mine, inbox, deleg, cands] = await Promise.all([
      gw<MeLeaveResponse>("api/v1/me/leave?limit=50&offset=0"),
      hr ? gw<{ requests: Pending[] }>("api/v1/leave/approvals") : Promise.resolve(null),
      hr ? gw<{ delegations: Delegation[] }>("api/v1/approval-delegations") : Promise.resolve(null),
      hr ? gw<{ users: Candidate[] }>("api/v1/approval-delegations/candidates") : Promise.resolve(null),
    ]);
    setIsHr(hr);
    setLinked(mine.employee_linked);
    setBalances(mine.balances);
    setRequests(mine.requests);
    setHasMore(mine.has_more);
    setPending(inbox?.requests ?? []);
    setDelegations(deleg?.delegations ?? []);
    setCandidates((cands?.users ?? []).filter((c) => c.id !== me.user_id));
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
      setError(tr("leave.file-larger-than-2-mb"));
      return;
    }
    try {
      const parsed = csvToBalanceImportRows(await file.text());
      if (gen === importGen.current) setImportRows(parsed);
    } catch (err) {
      if (gen === importGen.current)
        setError(err instanceof Error ? err.message : tr("leave.failed-to-read-file"));
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
        setError(tr("leave.there-is-no-import-quota-because"));
      } else {
        setNotice(tr("leave.importedNotice", { n: r.created }));
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
    const ok = await act(tr("leave.leave-request-sent"), "api/v1/leave/requests", {
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

  async function createDelegation(e: React.FormEvent) {
    e.preventDefault();
    if (!delegateUser || !delegateStart || !delegateEnd) return;
    const ok = await act(tr("leave.assignment-saved"), "api/v1/approval-delegations", {
      delegate_user_id: delegateUser,
      starts_on: delegateStart,
      ends_on: delegateEnd,
    });
    if (ok) {
      setDelegateUser("");
      setDelegateStart("");
      setDelegateEnd("");
    }
  }

  async function cancelDelegation(id: string) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw(`api/v1/approval-delegations/${id}`, { method: "DELETE" });
      setNotice(tr("leave.delegation-cancelled"));
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  const typeLabel = (t: string) => balances.find((b) => b.leave_type === t)?.label_th ?? t;
  const badge = (status: string) => {
    const st = STATUS[status];
    return <span className={`badge ${st?.tone ?? ""}`}>{st ? tr(st.labelKey) : status}</span>;
  };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{tr("leave.leave")}</h1>
          <p className="page-sub">{tr("leave.submit-a-leave-request-view-remaining")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="badge success" role="status" aria-live="polite">{notice}</p>}

      {isHr && (
        <div className="card">
          <div className="card-head">
            <h2>{tr("leave.pending-approval")}</h2>
            <span className="hint">{pending.length} {tr("leave.items")}</span>
          </div>
          {pending.length === 0 ? (
            <div className="empty">{tr("leave.there-are-no-leave-requests-pending")}</div>
          ) : (
            <div style={{ overflowX: "auto" }}>
              <table className="table">
                <thead>
                  <tr>
                    <th scope="col">{tr("leave.employee")}</th>
                    <th scope="col">{tr("leave.type")}</th>
                    <th scope="col">{tr("leave.date")}</th>
                    <th scope="col" className="num">{tr("leave.day")}</th>
                    <th scope="col">{tr("leave.reason")}</th>
                    <th scope="col">{tr("leave.stage")}</th>
                    <th scope="col" />
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
                        {tr("leave.step")} {r.step_no}/{r.total_steps} · {r.approver_role in ROLE_LABEL ? tr(ROLE_LABEL[r.approver_role]) : r.approver_role}
                      </td>
                      <td className="num">
                        <span style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                          <button
                            className="btn btn-sm btn-danger"
                            data-testid="reject-leave"
                            disabled={busy}
                            onClick={() =>
                              act(tr("leave.leave-request-not-approved"), `api/v1/leave/requests/${r.id}/decision`, { approve: false })
                            }
                          >
                            {tr("leave.reject")}
                          </button>
                          <button
                            className="btn btn-sm btn-primary"
                            data-testid="approve-leave"
                            disabled={busy}
                            onClick={() =>
                              act(tr("leave.leave-request-approved"), `api/v1/leave/requests/${r.id}/decision`, { approve: true })
                            }
                          >
                            {tr("leave.approve")}
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
          <h2>{tr("leave.assign-approval")}</h2>
          <p className="hint">{tr("leave.delegate-leave-approval-rights-to-other")}</p>
          <form onSubmit={createDelegation} style={{ display: "grid", gap: 12 }}>
            <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr 1fr" }}>
              <div className="field">
                <label htmlFor="delegate-user">{tr("leave.assignee")}</label>
                <select
                  id="delegate-user"
                  data-testid="delegate-user"
                  value={delegateUser}
                  onChange={(e) => setDelegateUser(e.target.value)}
                  required
                >
                  <option value="">{tr("leave.select-user")}</option>
                  {candidates.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.full_name_th ?? c.email}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label htmlFor="delegate-start">{tr("leave.start")}</label>
                <ThaiDateInput id="delegate-start" testId="delegate-start" value={delegateStart} onChange={(v) => setDelegateStart(v)} required />
              </div>
              <div className="field">
                <label htmlFor="delegate-end">{tr("leave.to")}</label>
                <ThaiDateInput id="delegate-end" testId="delegate-end" value={delegateEnd} onChange={(v) => setDelegateEnd(v)} required />
              </div>
            </div>
            <div>
              <button className="btn btn-primary btn-sm" data-testid="create-delegation" type="submit" disabled={busy}>
                {tr("leave.record-of-assignment")}
              </button>
            </div>
          </form>
          {delegations.length > 0 && (
            <table className="table">
              <thead>
                <tr>
                  <th scope="col">{tr("leave.assignee")}</th>
                  <th scope="col">{tr("leave.start")}</th>
                  <th scope="col">{tr("leave.to")}</th>
                  <th scope="col" />
                </tr>
              </thead>
              <tbody>
                {delegations.map((dg) => (
                  <tr key={dg.id} data-testid="delegation-row">
                    <td>{dg.delegate_name ?? dg.delegate_email}</td>
                    <td>{thDate(dg.starts_on)}</td>
                    <td>{thDate(dg.ends_on)}</td>
                    <td className="num">
                      <button
                        className="btn btn-sm btn-danger"
                        data-testid="cancel-delegation"
                        disabled={busy}
                        onClick={() => cancelDelegation(dg.id)}
                      >
                        {tr("leave.cancel")}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}

      {isHr && (
        <div className="card card-pad" style={{ display: "grid", gap: 12 }}>
          <h2>{tr("leave.import-leave-quota-csv")}</h2>
          <p className="hint">
            {tr("leave.text")} {BALANCE_IMPORT_REQUIRED.join(", ")}, {BALANCE_IMPORT_OPTIONAL.join(", ")} {tr("leave.text-2")}{" "}
            {BALANCE_IMPORT_MAX_ROWS} {tr("leave.text-3")}
          </p>
          <label htmlFor="import-balance-csv">{tr("leave.select-the-leave-quota-csv-file")}</label>
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
                  {tr("leave.check-file")}
                </button>
                <button
                  className="btn btn-primary"
                  data-testid="confirm-balance-import"
                  onClick={() => runBalanceImport(false)}
                  disabled={busy || !importResult || importResult.valid === 0 || importResult.invalid > 0}
                >
                  {tr("leave.confirm-import")}
                </button>
                {importResult && (
                  <span data-testid="import-summary">
                    {tr("leave.all")} {importResult.total} {tr("leave.text-4")} {importResult.valid} {tr("leave.text-5")} {importResult.invalid}
                  </span>
                )}
              </div>
              <table className="table">
                <thead>
                  <tr>
                    <th scope="col">{tr("leave.row")}</th>
                    <th scope="col">{tr("leave.employee-id")}</th>
                    <th scope="col">{tr("leave.type")}</th>
                    <th scope="col" className="num">{tr("leave.year")}</th>
                    <th scope="col" className="num">{tr("leave.all-days")}</th>
                    <th scope="col" className="num">{tr("leave.use-date")}</th>
                    <th scope="col">{tr("leave.status")}</th>
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
                              <span className="badge success">{tr("leave.valid")}</span>
                            ) : (
                              <span className="error" role="alert">{res.errors.map(thBalanceImportError).join(" · ")}</span>
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
        <div className="spinner">{tr("leave.loading")}</div>
      ) : !linked ? (
        <div className="card">
          <div className="empty">
            {tr("leave.this-account-has-not-been-linked")}
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
                    <div className="stat-hint">{tr("leave.remaining-days-total-days")}</div>
                  </>
                ) : (
                  <>
                    <div className="stat-value" style={{ fontSize: 18 }}>{tr("leave.quota-not-yet-set")}</div>
                    <div className="stat-hint">{tr("leave.let-hr-set-the-quota-first")}</div>
                  </>
                )}
              </div>
            ))}
          </div>

          <form className="card card-pad" onSubmit={submit} style={{ display: "grid", gap: 12 }}>
            <h2>{tr("leave.submit-a-leave-request")}</h2>
            <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr 1fr" }}>
              <div className="field">
                <label htmlFor="leave-type">{tr("leave.type")}</label>
                <select id="leave-type" data-testid="leave-type" value={leaveType} onChange={(e) => setLeaveType(e.target.value)}>
                  {balances.map((b) => (
                    <option key={b.leave_type} value={b.leave_type}>
                      {b.label_th}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label htmlFor="leave-start">{tr("leave.start")}</label>
                <ThaiDateInput id="leave-start" testId="leave-start" value={start} onChange={(v) => setStart(v)} required />
              </div>
              <div className="field">
                <label htmlFor="leave-end">{tr("leave.to")}</label>
                <ThaiDateInput id="leave-end" testId="leave-end" value={end} onChange={(v) => setEnd(v)} required />
              </div>
            </div>
            <div className="field">
              <label htmlFor="leave-reason">{tr("leave.reason")}</label>
              <input id="leave-reason" data-testid="leave-reason" value={reason} onChange={(e) => setReason(e.target.value)} />
            </div>
            <div>
              <button className="btn btn-primary btn-sm" data-testid="request-leave" type="submit" disabled={busy}>
                {tr("leave.submit-request")}
              </button>
            </div>
          </form>

          <div className="card">
            <div className="card-head">
              <h2>{tr("leave.leave-history")}</h2>
            </div>
            {requests.length === 0 ? (
              <div className="empty">{tr("leave.there-is-no-leave-request-yet")}</div>
            ) : (
              <div style={{ overflowX: "auto" }}>
                <table className="table">
                  <thead>
                    <tr>
                      <th scope="col">{tr("leave.type")}</th>
                      <th scope="col">{tr("leave.date")}</th>
                      <th scope="col" className="num">{tr("leave.day")}</th>
                      <th scope="col">{tr("leave.reason")}</th>
                      <th scope="col">{tr("leave.status")}</th>
                      <th scope="col" />
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
                              onClick={() => act(tr("leave.leave-request-canceled"), `api/v1/me/leave/${r.id}/cancel`)}
                            >
                              {tr("leave.cancel")}
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
                  {tr("leave.load-more")}
                </button>
              </div>
            )}
          </div>
        </>
      )}
    </>
  );
}

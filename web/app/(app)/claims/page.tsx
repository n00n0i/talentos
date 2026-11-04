"use client";

import type { TranslationKey } from "@/lib/i18n-catalog";

import { useI18N, tNow } from "@/lib/i18n";

import ThaiDateInput from "@/components/ThaiDateInput";

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

const CATEGORIES: { key: string; labelKey: TranslationKey }[] = [
  { key: "travel", labelKey: "claims.travel" },
  { key: "meal", labelKey: "claims.food" },
  { key: "supplies", labelKey: "claims.office-supplies" },
  { key: "other", labelKey: "claims.other" },
];

const CLAIM_ERRORS: Record<string, TranslationKey> = {
  "category must be one of: travel, meal, supplies, other": "claims.selected-incorrect-expense-type",
  "amount must be greater than 0": "claims.amount-must-be-greater-than-0",
  "expense_date is required": "claims.specify-the-date-of-the-expense",
  "description must be at most 500 characters": "claims.description-must-not-exceed-500-characters",
  "expense claim cannot be cancelled in its current status": "claims.this-item-cannot-be-canceled",
  "expense claim cannot be changed in its current status": "claims.this-item-is-not-in-pending",
  "expense claim is not in pending status": "claims.this-item-is-not-in-pending",
  "expense claim not found": "claims.expense-item-not-found",
  "not found": "claims.expense-item-not-found",
  "no active employee record for this account": "claims.no-active-employee-information-found-for",
  "receipt is too large; maximum size is 5 MB": "claims.file-larger-than-5-mb",
  "receipt type must be image/jpeg, image/png, or application/pdf": "claims.only-jpg-png-or-pdf-are",
  "receipt already uploaded for this claim": "claims.attached-receipt",
};

const thClaimError = (message: string, t: (key: TranslationKey) => string) =>
  CLAIM_ERRORS[message] ? t(CLAIM_ERRORS[message]) : message;

const STATUS: Record<string, { labelKey: TranslationKey; tone: string }> = {
  pending: { labelKey: "claims.pending-approval", tone: "warning" },
  approved: { labelKey: "claims.approved", tone: "success" },
  rejected: { labelKey: "claims.reject", tone: "danger" },
  cancelled: { labelKey: "claims.canceled", tone: "" },
  paid: { labelKey: "claims.paid", tone: "success" },
};

const categoryLabel = (key: string) => {
  const found = CATEGORIES.find((c) => c.key === key);
  return found ? tNow(found.labelKey) : key;
};

export default function ClaimsPage() {
  const { t: tr } = useI18N();
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
    load().catch((e) => handleApiError(e, (m) => setError(thClaimError(m, tr))));
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
      handleApiError(e, (m) => setError(thClaimError(m, tr)));
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
      throw new Error(thClaimError(message, tr));
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
        setNotice(tr("claims.submit-expense-reimbursement-request-and-attach"));
      } else {
        setNotice(tr("claims.expense-reimbursement-request-sent"));
      }
      setCategory("meal");
      setAmount("");
      setDate("");
      setDescription("");
      setFile(null);
      if (receiptRef.current) receiptRef.current.value = "";
      await load();
    } catch (e) {
      handleApiError(e, (m) => setError(thClaimError(m, tr)));
    } finally {
      setBusy(false);
    }
  }

  const badge = (status: string) => {
    const st = STATUS[status];
    return <span className={`badge ${st?.tone ?? ""}`}>{st ? tr(st.labelKey) : status}</span>;
  };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{tr("claims.expenses")}</h1>
          <p className="page-sub">{tr("claims.submit-an-expense-reimbursement-request-with")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="badge success" role="status" aria-live="polite">{notice}</p>}

      {isHr && (
        <div className="card">
          <div className="card-head">
            <h2>{tr("claims.pending-approval")}</h2>
            <span className="hint">{pending.length} {tr("claims.items")}</span>
          </div>
          {pending.length === 0 ? (
            <div className="empty">{tr("claims.there-are-no-expense-reimbursement-requests")}</div>
          ) : (
            <div style={{ overflowX: "auto" }}>
              <table className="table">
                <thead>
                  <tr>
                    <th scope="col">{tr("claims.employee")}</th>
                    <th scope="col">{tr("claims.type")}</th>
                    <th scope="col">{tr("claims.date")}</th>
                    <th scope="col" className="num">{tr("claims.amount")}</th>
                    <th scope="col">{tr("claims.details")}</th>
                    <th scope="col">{tr("claims.receipt")}</th>
                    <th scope="col" />
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
                            {tr("claims.view-receipt")}
                          </a>
                        ) : (
                          <span className="hint">{tr("claims.do-not-have")}</span>
                        )}
                      </td>
                      <td className="num">
                        <span style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                          <button
                            className="btn btn-sm btn-danger"
                            data-testid="reject-claim"
                            disabled={busy}
                            onClick={() =>
                              act(tr("claims.the-expense-reimbursement-request-has-not"), `api/v1/claims/${r.id}/decision`, { approve: false })
                            }
                          >
                            {tr("claims.reject")}
                          </button>
                          <button
                            className="btn btn-sm btn-primary"
                            data-testid="approve-claim"
                            disabled={busy}
                            onClick={() =>
                              act(tr("claims.expense-reimbursement-request-approved"), `api/v1/claims/${r.id}/decision`, { approve: true })
                            }
                          >
                            {tr("claims.approve")}
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
        <div className="spinner">{tr("claims.loading")}</div>
      ) : !linked ? (
        <div className="card">
          <div className="empty">
            {tr("claims.this-account-has-not-been-linked")}
          </div>
        </div>
      ) : (
        <>
          <form className="card card-pad" onSubmit={submit} style={{ display: "grid", gap: 12 }}>
            <h2>{tr("claims.submit-a-request-for-expense-reimbursement")}</h2>
            <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr 1fr" }}>
              <div className="field">
                <label htmlFor="claim-category">{tr("claims.type")}</label>
                <select
                  id="claim-category"
                  data-testid="claim-category"
                  value={category}
                  onChange={(e) => setCategory(e.target.value)}
                >
                  {CATEGORIES.map((c) => (
                    <option key={c.key} value={c.key}>
                      {tr(c.labelKey)}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label htmlFor="claim-amount">{tr("claims.amount")}</label>
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
                <label htmlFor="claim-date">{tr("claims.expense-date")}</label>
                <ThaiDateInput id="claim-date" testId="claim-date" value={date} onChange={(v) => setDate(v)} required />
              </div>
            </div>
            <div className="field">
              <label htmlFor="claim-description">{tr("claims.details")}</label>
              <input
                id="claim-description"
                data-testid="claim-description"
                maxLength={500}
                value={description}
                onChange={(e) => setDescription(e.target.value)}
              />
              <span className="hint">{tr("claims.max-500-characters")}</span>
            </div>
            <div className="field">
              <label htmlFor="claim-receipt">{tr("claims.receipt")}</label>
              <input
                id="claim-receipt"
                ref={receiptRef}
                data-testid="claim-receipt"
                type="file"
                accept="image/jpeg,image/png,application/pdf"
                onChange={(e) => setFile(e.target.files?.[0] ?? null)}
              />
              <span className="hint">{tr("claims.jpg-png-or-pdf-no-more")}</span>
            </div>
            <div>
              <button className="btn btn-primary btn-sm" data-testid="submit-claim" type="submit" disabled={busy}>
                {tr("claims.submit-request")}
              </button>
            </div>
          </form>

          <div className="card">
            <div className="card-head">
              <h2>{tr("claims.expense-history")}</h2>
            </div>
            {claims.length === 0 ? (
              <div className="empty">{tr("claims.there-has-been-no-request-for")}</div>
            ) : (
              <div style={{ overflowX: "auto" }}>
                <table className="table">
                  <thead>
                    <tr>
                      <th scope="col">{tr("claims.type")}</th>
                      <th scope="col">{tr("claims.date")}</th>
                      <th scope="col" className="num">{tr("claims.amount")}</th>
                      <th scope="col">{tr("claims.details")}</th>
                      <th scope="col">{tr("claims.status")}</th>
                      <th scope="col">{tr("claims.receipt")}</th>
                      <th scope="col" />
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
                              {tr("claims.view-receipt")}
                            </a>
                          ) : (
                            <span className="hint">{tr("claims.do-not-have")}</span>
                          )}
                        </td>
                        <td className="num">
                          {r.status === "pending" && (
                            <button
                              className="btn btn-sm"
                              data-testid="cancel-claim"
                              disabled={busy}
                              onClick={() => act(tr("claims.expense-reimbursement-request-canceled"), `api/v1/me/claims/${r.id}/cancel`)}
                            >
                              {tr("claims.cancel")}
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

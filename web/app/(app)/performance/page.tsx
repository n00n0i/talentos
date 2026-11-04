"use client";

import { useI18N } from "@/lib/i18n";

import ThaiDateInput from "@/components/ThaiDateInput";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, Me, thDate } from "@/lib/api";

type Cycle = {
  id: string;
  name: string;
  period_start: string;
  period_end: string;
  status: "draft" | "active" | "closed";
  review_count: number;
  submitted_count: number;
};

type Review = {
  id: string;
  cycle_id: string;
  employee_id: string;
  employee_code: string;
  employee_name: string;
  status: "pending" | "submitted" | "acknowledged";
  rating: number | null;
  strengths: string | null;
  improvements: string | null;
  submitted_at: string | null;
};

type DirectReport = {
  id: string;
  employee_code: string;
  employee_name: string;
};

type Calibration = {
  id: string;
  cycle_id: string;
  employee_id: string;
  employee_code: string;
  employee_name: string;
  calibrated_rating: number;
  notes: string | null;
};

const emptyCycle = { name: "", period_start: "", period_end: "" };
type Draft = { rating: string; strengths: string; improvements: string };

export default function PerformancePage() {
  const { t: tr } = useI18N();
  const [cycles, setCycles] = useState<Cycle[]>([]);
  const [selected, setSelected] = useState<Cycle | null>(null);
  const [reviews, setReviews] = useState<Review[]>([]);
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const [form, setForm] = useState(emptyCycle);
  const [canWrite, setCanWrite] = useState(false);
  const [canDirect, setCanDirect] = useState(false);
  const [directReports, setDirectReports] = useState<DirectReport[]>([]);
  const [calibrations, setCalibrations] = useState<Calibration[]>([]);
  const [calibrationEmployee, setCalibrationEmployee] = useState("");
  const [calibrationRating, setCalibrationRating] = useState("");
  const [calibrationNotes, setCalibrationNotes] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const loadCycles = useCallback(async () => {
    // The manager-scoped list only surfaces cycles that already hold a review of a
    // direct report, so it hides freshly created cycles. Users who can write
    // cycles (hr_admin) must see the full list; only pure direct-managers fall
    // back to the scoped endpoint.
    const res = await gw<{ cycles: Cycle[] }>(
      canDirect && !canWrite
        ? "api/v1/me/performance/cycles"
        : "api/v1/performance/cycles",
    );
    setCycles(res.cycles);
    setSelected((current) => res.cycles.find((c) => c.id === current?.id) ?? null);
  }, [canDirect, canWrite]);

  const loadReviews = useCallback(async (cycleId: string) => {
    const res = await gw<{ reviews: Review[] }>(
      `api/v1/performance/cycles/${cycleId}/reviews`,
    );
    setReviews(res.reviews);
  }, []);

  useEffect(() => {
    loadCycles().catch((e) => handleApiError(e, setError));
    gw<Me>("api/v1/auth/me")
      .then((me) => {
        setCanWrite(me.permissions.includes("employee.update"));
        setCanDirect(me.permissions.includes("performance.manage_direct"));
      })
      .catch((e) => handleApiError(e, setError));
  }, [loadCycles]);

  useEffect(() => {
    if (!canDirect) return;
    gw<{ direct_reports: DirectReport[] }>("api/v1/me/direct-reports")
      .then((res) => setDirectReports(res.direct_reports))
      .catch((e) => handleApiError(e, setError));
  }, [canDirect]);

  const loadCalibrations = useCallback(async (cycleId: string) => {
    const res = await gw<{ calibrations: Calibration[] }>(
      `api/v1/performance/cycles/${cycleId}/calibrations`,
    );
    setCalibrations(res.calibrations);
  }, []);

  useEffect(() => {
    if (!canDirect || !selected) {
      setCalibrations([]);
      return;
    }
    loadCalibrations(selected.id).catch((e) => handleApiError(e, setError));
  }, [canDirect, selected, loadCalibrations]);

  async function saveCalibration() {
    if (!selected || !calibrationEmployee || !calibrationRating) {
      setError(tr("performance.select-employees-and-scores-before-saving"));
      return;
    }
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw(`api/v1/performance/cycles/${selected.id}/calibrations`, {
        method: "POST",
        body: JSON.stringify({
          employee_id: calibrationEmployee,
          calibrated_rating: Number(calibrationRating),
          notes: calibrationNotes.trim() || null,
        }),
      });
      setCalibrationNotes("");
      setCalibrationRating("");
      await loadCalibrations(selected.id);
      setNotice(tr("performance.level-layout-saved"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    if (!selected) {
      setReviews([]);
      return;
    }
    // The full review table is read_all-gated server-side. Managers who only
    // hold performance.manage_direct get their calibrations instead, so skip
    // the reviews call for them.
    if (!canWrite && canDirect) {
      setReviews([]);
      return;
    }
    loadReviews(selected.id).catch((e) => handleApiError(e, setError));
  }, [selected, canWrite, canDirect, loadReviews]);

  useEffect(() => {
    setDrafts(
      Object.fromEntries(
        reviews.map((r) => [
          r.id,
          {
            rating: r.rating?.toString() ?? "",
            strengths: r.strengths ?? "",
            improvements: r.improvements ?? "",
          },
        ]),
      ),
    );
  }, [reviews]);

  function setDraft(id: string, patch: Partial<Draft>) {
    setDrafts((current) => {
      const base = current[id] ?? { rating: "", strengths: "", improvements: "" };
      return { ...current, [id]: { ...base, ...patch } };
    });
  }

  async function createCycle(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const created = await gw<{ cycle: Cycle }>("api/v1/performance/cycles", {
        method: "POST",
        body: JSON.stringify(form),
      });
      setForm(emptyCycle);
      await loadCycles();
      setSelected(created.cycle);
      setNotice(tr("performance.an-evaluation-round-has-been-created"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function transition(cycle: Cycle, action: "start" | "close") {
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/performance/cycles/${cycle.id}/${action}`, { method: "POST" });
      await loadCycles();
      setNotice(action === "start" ? tr("performance.the-evaluation-round-has-begun") : tr("performance.the-evaluation-round-has-closed"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function removeCycle(cycle: Cycle) {
    if (!confirm(tr("performance.deleteCycleConfirm", { name: cycle.name }))) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/performance/cycles/${cycle.id}`, { method: "DELETE" });
      if (selected?.id === cycle.id) setSelected(null);
      await loadCycles();
      setNotice(tr("performance.evaluation-round-deleted"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function submitReview(review: Review) {
    const draft = drafts[review.id];
    if (!draft?.rating) {
      setError(tr("performance.select-score-before-saving"));
      return;
    }
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/performance/reviews/${review.id}`, {
        method: "PATCH",
        body: JSON.stringify({
          rating: Number(draft.rating),
          strengths: draft.strengths.trim() || null,
          improvements: draft.improvements.trim() || null,
        }),
      });
      await Promise.all([loadReviews(review.cycle_id), loadCycles()]);
      setNotice(tr("performance.evaluation-results-recorded"));
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
          <h1>{tr("performance.performance-reviews")}</h1>
          <p className="page-sub">{tr("performance.review-cycles-ratings-and-per-employee")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="success" role="status" aria-live="polite">{notice}</p>}

      {canWrite && (
        <div className="card card-pad" style={{ marginBottom: 12 }}>
          <h2 style={{ fontSize: 16, marginBottom: 10 }}>{tr("performance.create-an-evaluation-round")}</h2>
          <form onSubmit={createCycle} style={{ display: "grid", gap: 10 }}>
            <div className="grid-2" style={{ gridTemplateColumns: "2fr 1fr 1fr", gap: 12 }}>
              <label>
                {tr("performance.cycle-name")}
                <input
                  data-testid="cycle-name"
                  value={form.name}
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                  required
                />
              </label>
              <label>
                {tr("performance.start")}
                <ThaiDateInput testId="cycle-start" value={form.period_start} onChange={(v) => setForm({ ...form, period_start: v })} required />
              </label>
              <label>
                {tr("performance.end")}
                <ThaiDateInput testId="cycle-end" value={form.period_end} onChange={(v) => setForm({ ...form, period_end: v })} required />
              </label>
            </div>
            <button className="btn btn-primary" data-testid="create-cycle" type="submit" disabled={busy}>
              {tr("performance.create-a-round")}
            </button>
          </form>
        </div>
      )}

      <div className="card" style={{ marginBottom: 12 }}>
        {cycles.length === 0 ? (
          <div className="empty">{tr("performance.there-is-no-evaluation-round-yet")}</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{tr("performance.cycle")}</th>
                <th scope="col">{tr("performance.period")}</th>
                <th scope="col">{tr("performance.status")}</th>
                <th scope="col">{tr("performance.sent")}</th>
                <th scope="col" />
              </tr>
            </thead>
            <tbody>
              {cycles.map((cycle) => (
                <tr key={cycle.id} data-testid="cycle-row">
                  <td>
                    <button
                      className="btn btn-sm"
                      data-testid="select-cycle"
                      onClick={() => setSelected(cycle)}
                    >
                      {cycle.name}
                    </button>
                  </td>
                  <td>
                    {thDate(cycle.period_start)} – {thDate(cycle.period_end)}
                  </td>
                  <td>
                    <span className={`badge ${cycle.status === "active" ? "success" : ""}`}>
                      {cycle.status === "active" ? tr("performance.evaluating") : cycle.status === "closed" ? tr("performance.closed") : tr("performance.draft")}
                    </span>
                  </td>
                  <td>
                    {cycle.submitted_count}/{cycle.review_count}
                  </td>
                  <td>
                    {canWrite && (
                      <>
                        {cycle.status === "draft" && (
                          <button
                            className="btn btn-sm"
                            data-testid="start-cycle"
                            disabled={busy}
                            onClick={() => transition(cycle, "start")}
                            style={{ marginRight: 6 }}
                          >
                            {tr("performance.start-the-round")}
                          </button>
                        )}
                        {cycle.status === "active" && (
                          <button
                            className="btn btn-sm"
                            data-testid="close-cycle"
                            disabled={busy}
                            onClick={() => transition(cycle, "close")}
                            style={{ marginRight: 6 }}
                          >
                            {tr("performance.close-cycle")}
                          </button>
                        )}
                        {cycle.status === "draft" && (
                          <button
                            className="btn btn-sm"
                            data-testid="delete-cycle"
                            disabled={busy}
                            onClick={() => removeCycle(cycle)}
                          >
                            {tr("performance.delete")}
                          </button>
                        )}
                      </>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {selected && (
        <div className="card card-pad">
          <div className="card-head">
            <h2 style={{ fontSize: 16 }}>
              {tr("performance.text")} {selected.name}{" "}
              {selected.status === "closed" ? (
                <span className="badge warning">{tr("performance.round-closed-cannot-be-edited")}</span>
              ) : selected.status === "draft" ? (
                <span className="badge warning">{tr("performance.a-round-must-be-started-before")}</span>
              ) : null}
            </h2>
          </div>

          {reviews.length === 0 ? (
            <div className="empty">{tr("performance.there-are-no-evaluation-results-for")}</div>
          ) : (
            <table className="table">
              <thead>
                <tr>
                  <th scope="col">{tr("performance.employee")}</th>
                  <th scope="col">{tr("performance.status")}</th>
                  <th scope="col">{tr("performance.score-1-5")}</th>
                  <th scope="col">{tr("performance.strengths")}</th>
                  <th scope="col">{tr("performance.things-that-should-be-developed")}</th>
                  <th scope="col" />
                </tr>
              </thead>
              <tbody>
                {reviews.map((review) => {
                  const draft = drafts[review.id] ?? {
                    rating: "",
                    strengths: "",
                    improvements: "",
                  };
                  const editable = canWrite && selected.status === "active";
                  return (
                    <tr key={review.id} data-testid="review-row">
                      <td>
                        {review.employee_name} ({review.employee_code})
                      </td>
                      <td>
                        <span
                          className={`badge ${
                            review.status === "acknowledged"
                              ? "success"
                              : review.status === "submitted"
                                ? "warning"
                                : ""
                          }`}
                        >
                          {review.status === "acknowledged"
                            ? tr("performance.acknowledged")
                            : review.status === "submitted"
                              ? tr("performance.sent")
                              : tr("performance.waiting-for-evaluation")}
                        </span>
                      </td>
                      <td>
                        <select
                          data-testid="review-rating"
                          value={draft.rating}
                          disabled={busy || !editable}
                          onChange={(e) => setDraft(review.id, { rating: e.target.value })}
                        >
                          <option value="">—</option>
                          {[1, 2, 3, 4, 5].map((n) => (
                            <option key={n} value={n}>
                              {n}
                            </option>
                          ))}
                        </select>
                      </td>
                      <td>
                        <input
                          data-testid="review-strengths"
                          value={draft.strengths}
                          disabled={busy || !editable}
                          onChange={(e) => setDraft(review.id, { strengths: e.target.value })}
                        />
                      </td>
                      <td>
                        <input
                          data-testid="review-improvements"
                          value={draft.improvements}
                          disabled={busy || !editable}
                          onChange={(e) => setDraft(review.id, { improvements: e.target.value })}
                        />
                      </td>
                      <td>
                        {editable && (
                          <button
                            className="btn btn-sm btn-primary"
                            data-testid="submit-review"
                            disabled={busy}
                            onClick={() => submitReview(review)}
                          >
                            {tr("performance.save")}
                          </button>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </div>
      )}

      {selected && canDirect && (
        <div className="card card-pad" style={{ marginTop: 12 }}>
          <h2 style={{ fontSize: 16, marginBottom: 10 }}>
            {tr("performance.calibration-direct-reports")}
          </h2>
          {calibrations.length === 0 ? (
            <div className="empty">{tr("performance.no-calibration-for-this-cycle-yet")}</div>
          ) : (
            <table className="table" style={{ marginBottom: 14 }}>
              <thead>
                <tr>
                  <th scope="col">{tr("performance.employee")}</th>
                  <th scope="col">{tr("performance.calibration-score")}</th>
                  <th scope="col">{tr("performance.notes")}</th>
                </tr>
              </thead>
              <tbody>
                {calibrations.map((c) => (
                  <tr key={c.id} data-testid="calibration-row">
                    <td>
                      {c.employee_name} ({c.employee_code})
                    </td>
                    <td>{c.calibrated_rating}/5</td>
                    <td>{c.notes ?? "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <div style={{ display: "grid", gap: 8 }}>
            <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
              <select
                data-testid="calibration-employee"
                value={calibrationEmployee}
                onChange={(e) => setCalibrationEmployee(e.target.value)}
              >
                <option value="">{tr("performance.select-employee")}</option>
                {directReports.map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.employee_code} · {r.employee_name}
                  </option>
                ))}
              </select>
              <select
                data-testid="calibration-rating"
                value={calibrationRating}
                onChange={(e) => setCalibrationRating(e.target.value)}
              >
                <option value="">—</option>
                {[1, 2, 3, 4, 5].map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
              <input
                data-testid="calibration-notes"
                placeholder={tr("performance.notes-optional")}
                value={calibrationNotes}
                onChange={(e) => setCalibrationNotes(e.target.value)}
              />
            </div>
            <button
              className="btn btn-sm btn-primary"
              data-testid="save-calibration"
              disabled={busy}
              onClick={saveCalibration}
            >
              {tr("performance.save-calibration")}
            </button>
          </div>
        </div>
      )}
    </>
  );
}

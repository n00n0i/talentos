"use client";

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

const emptyCycle = { name: "", period_start: "", period_end: "" };
type Draft = { rating: string; strengths: string; improvements: string };

export default function PerformancePage() {
  const [cycles, setCycles] = useState<Cycle[]>([]);
  const [selected, setSelected] = useState<Cycle | null>(null);
  const [reviews, setReviews] = useState<Review[]>([]);
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const [form, setForm] = useState(emptyCycle);
  const [canWrite, setCanWrite] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const loadCycles = useCallback(async () => {
    const res = await gw<{ cycles: Cycle[] }>("api/v1/performance/cycles");
    setCycles(res.cycles);
    setSelected((current) => res.cycles.find((c) => c.id === current?.id) ?? null);
  }, []);

  const loadReviews = useCallback(async (cycleId: string) => {
    const res = await gw<{ reviews: Review[] }>(
      `api/v1/performance/cycles/${cycleId}/reviews`,
    );
    setReviews(res.reviews);
  }, []);

  useEffect(() => {
    loadCycles().catch((e) => handleApiError(e, setError));
    gw<Me>("api/v1/auth/me")
      .then((me) => setCanWrite(me.permissions.includes("employee.update")))
      .catch((e) => handleApiError(e, setError));
  }, [loadCycles]);

  useEffect(() => {
    if (!selected) {
      setReviews([]);
      return;
    }
    loadReviews(selected.id).catch((e) => handleApiError(e, setError));
  }, [selected, loadReviews]);

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
      setNotice("สร้างรอบประเมินแล้ว");
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
      setNotice(action === "start" ? "เริ่มรอบประเมินแล้ว" : "ปิดรอบประเมินแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function removeCycle(cycle: Cycle) {
    if (!confirm(`ลบรอบ ${cycle.name} และผลประเมินทั้งหมดหรือไม่?`)) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/performance/cycles/${cycle.id}`, { method: "DELETE" });
      if (selected?.id === cycle.id) setSelected(null);
      await loadCycles();
      setNotice("ลบรอบประเมินแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function submitReview(review: Review) {
    const draft = drafts[review.id];
    if (!draft?.rating) {
      setError("เลือกคะแนนก่อนบันทึก");
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
      setNotice("บันทึกผลประเมินแล้ว");
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
          <h1>ประเมินผลการทำงาน</h1>
          <p className="page-sub">รอบประเมิน คะแนน และข้อคิดเห็นรายพนักงาน</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="success">{notice}</p>}

      {canWrite && (
        <div className="card card-pad" style={{ marginBottom: 12 }}>
          <h2 style={{ fontSize: 16, marginBottom: 10 }}>สร้างรอบประเมิน</h2>
          <form onSubmit={createCycle} style={{ display: "grid", gap: 10 }}>
            <div className="grid-2" style={{ gridTemplateColumns: "2fr 1fr 1fr", gap: 12 }}>
              <label>
                ชื่อรอบ
                <input
                  data-testid="cycle-name"
                  value={form.name}
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                  required
                />
              </label>
              <label>
                เริ่ม
                <input
                  data-testid="cycle-start"
                  type="date"
                  value={form.period_start}
                  onChange={(e) => setForm({ ...form, period_start: e.target.value })}
                  required
                />
              </label>
              <label>
                สิ้นสุด
                <input
                  data-testid="cycle-end"
                  type="date"
                  value={form.period_end}
                  onChange={(e) => setForm({ ...form, period_end: e.target.value })}
                  required
                />
              </label>
            </div>
            <button className="btn btn-primary" data-testid="create-cycle" type="submit" disabled={busy}>
              สร้างรอบ
            </button>
          </form>
        </div>
      )}

      <div className="card" style={{ marginBottom: 12 }}>
        {cycles.length === 0 ? (
          <div className="empty">ยังไม่มีรอบประเมิน</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>รอบ</th>
                <th>ช่วงเวลา</th>
                <th>สถานะ</th>
                <th>ส่งแล้ว</th>
                <th />
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
                      {cycle.status === "active" ? "กำลังประเมิน" : cycle.status === "closed" ? "ปิดแล้ว" : "ร่าง"}
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
                            เริ่มรอบ
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
                            ปิดรอบ
                          </button>
                        )}
                        {cycle.status === "draft" && (
                          <button
                            className="btn btn-sm"
                            data-testid="delete-cycle"
                            disabled={busy}
                            onClick={() => removeCycle(cycle)}
                          >
                            ลบ
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
              ผลประเมิน: {selected.name}{" "}
              {selected.status === "closed" ? (
                <span className="badge warning">ปิดรอบแล้ว — แก้ไขไม่ได้</span>
              ) : selected.status === "draft" ? (
                <span className="badge warning">ต้องเริ่มรอบก่อนจึงจะบันทึกได้</span>
              ) : null}
            </h2>
          </div>

          {reviews.length === 0 ? (
            <div className="empty">ยังไม่มีผลประเมินในรอบนี้ — กด “เริ่มรอบ” เพื่อสร้างรายการ</div>
          ) : (
            <table className="table">
              <thead>
                <tr>
                  <th>พนักงาน</th>
                  <th>สถานะ</th>
                  <th>คะแนน (1–5)</th>
                  <th>จุดเด่น</th>
                  <th>สิ่งที่ควรพัฒนา</th>
                  <th />
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
                            ? "รับทราบแล้ว"
                            : review.status === "submitted"
                              ? "ส่งแล้ว"
                              : "รอประเมิน"}
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
                            บันทึก
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
    </>
  );
}

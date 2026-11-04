"use client";

import { useI18N } from "@/lib/i18n";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, thDate } from "@/lib/api";

type Review = {
  id: string;
  cycle_name: string;
  period_start: string;
  period_end: string;
  status: "pending" | "submitted" | "acknowledged";
  rating: number | null;
  strengths: string | null;
  improvements: string | null;
  submitted_at: string | null;
  acknowledged_at: string | null;
};

export default function MyReviewsPage() {
  const { t: tr } = useI18N();
  const [reviews, setReviews] = useState<Review[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    const res = await gw<{ reviews: Review[] }>("api/v1/me/reviews");
    setReviews(res.reviews);
  }, []);

  useEffect(() => {
    load().catch((e) => handleApiError(e, setError));
  }, [load]);

  async function acknowledge(id: string) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw(`api/v1/me/reviews/${id}/acknowledge`, { method: "POST" });
      await load();
      setNotice(tr("meReviews.the-evaluation-results-have-been-acknowledged"));
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
          <h1>{tr("meReviews.my-reviews")}</h1>
          <p className="page-sub">{tr("meReviews.evaluation-results-from-various-rounds-and")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="success" role="status" aria-live="polite">{notice}</p>}

      {reviews.length === 0 ? (
        <div className="card">
          <div className="empty">{tr("meReviews.your-evaluation-is-not-yet-available")}</div>
        </div>
      ) : (
        <div style={{ display: "grid", gap: 12 }}>
          {reviews.map((review) => (
            <div className="card card-pad" key={review.id} data-testid="my-review">
              <div className="card-head">
                <h2 style={{ fontSize: 16 }}>{review.cycle_name}</h2>
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
                    ? tr("meReviews.acknowledged")
                    : review.status === "submitted"
                      ? tr("meReviews.waiting-to-be-informed")
                      : tr("meReviews.waiting-for-evaluation")}
                </span>
              </div>
              <p style={{ color: "var(--muted)", fontSize: 13 }}>
                {thDate(review.period_start)} – {thDate(review.period_end)}
              </p>
              <p data-testid="my-review-rating" style={{ fontSize: 20, fontWeight: 600 }}>
                {review.rating ? `${review.rating}/5` : "—"}
              </p>
              {review.strengths && (
                <p>
                  <strong>{tr("meReviews.highlights")}</strong> {review.strengths}
                </p>
              )}
              {review.improvements && (
                <p>
                  <strong>{tr("meReviews.what-should-be-developed")}</strong> {review.improvements}
                </p>
              )}
              {review.status === "submitted" && (
                <button
                  className="btn btn-primary"
                  data-testid="acknowledge-review"
                  disabled={busy}
                  onClick={() => acknowledge(review.id)}
                >
                  {tr("meReviews.acknowledge-the-evaluation-results")}
                </button>
              )}
              {review.acknowledged_at && (
                <p style={{ color: "var(--muted)", fontSize: 13 }}>
                  {tr("meReviews.text")} {thDate(review.acknowledged_at.slice(0, 10))}
                </p>
              )}
            </div>
          ))}
        </div>
      )}
    </>
  );
}

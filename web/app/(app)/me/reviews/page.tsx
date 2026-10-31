"use client";

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
      setNotice("รับทราบผลประเมินแล้ว");
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
          <h1>ผลประเมินของฉัน</h1>
          <p className="page-sub">ผลประเมินจากรอบต่าง ๆ และการรับทราบ</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="success">{notice}</p>}

      {reviews.length === 0 ? (
        <div className="card">
          <div className="empty">ยังไม่มีผลประเมินของคุณ</div>
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
                    ? "รับทราบแล้ว"
                    : review.status === "submitted"
                      ? "รอรับทราบ"
                      : "รอประเมิน"}
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
                  <strong>จุดเด่น:</strong> {review.strengths}
                </p>
              )}
              {review.improvements && (
                <p>
                  <strong>สิ่งที่ควรพัฒนา:</strong> {review.improvements}
                </p>
              )}
              {review.status === "submitted" && (
                <button
                  className="btn btn-primary"
                  data-testid="acknowledge-review"
                  disabled={busy}
                  onClick={() => acknowledge(review.id)}
                >
                  รับทราบผลประเมิน
                </button>
              )}
              {review.acknowledged_at && (
                <p style={{ color: "var(--muted)", fontSize: 13 }}>
                  รับทราบเมื่อ {thDate(review.acknowledged_at.slice(0, 10))}
                </p>
              )}
            </div>
          ))}
        </div>
      )}
    </>
  );
}

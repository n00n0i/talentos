"use client";

import { useI18N } from "@/lib/i18n";

import { useCallback, useEffect, useState } from "react";
import { ApiError, gw, handleApiError, thDate } from "@/lib/api";

type PrivacyStatus = { notice_version: string; accepted_at: string | null };

const bangkokToday = () =>
  new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Bangkok" }).format(new Date());

export default function PrivacyPage() {
  const { t: tr } = useI18N();
  const [status, setStatus] = useState<PrivacyStatus | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setStatus(await gw<PrivacyStatus>("api/v1/me/privacy"));
  }, []);

  useEffect(() => {
    load().catch((e) => handleApiError(e, setError));
  }, [load]);

  async function accept() {
    if (!status) return;
    setBusy(true);
    setError("");
    try {
      const res = await gw<PrivacyStatus>("api/v1/me/privacy/consent", {
        method: "POST",
        body: JSON.stringify({ version: status.notice_version }),
      });
      setStatus(res);
      setNotice(tr("privacy.acknowledgment-has-been-recorded"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function exportData() {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const res = await fetch("/api/gw/api/v1/me/data-export", { cache: "no-store" });
      if (!res.ok) throw new ApiError(res.statusText || `HTTP ${res.status}`, res.status);
      const blob = await res.blob();
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `talentos-personal-data-${bangkokToday()}.json`;
      a.click();
      URL.revokeObjectURL(url);
      setNotice(tr("privacy.personal-information-has-been-downloaded"));
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
          <h1>{tr("privacy.privacy-and-personal-information")}</h1>
          <p className="page-sub">{tr("privacy.privacy-notice-rights-of-the-data")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="success" role="status" aria-live="polite">{notice}</p>}

      <div className="card card-pad" style={{ display: "grid", gap: 14 }}>
        <section>
          <h2 style={{ fontSize: 16 }}>{tr("privacy.information-we-collect")}</h2>
          <p>
            {tr("privacy.text")}
          </p>
        </section>
        <section>
          <h2 style={{ fontSize: 16 }}>{tr("privacy.objective")}</h2>
          <p>
            {tr("privacy.text-2")}
          </p>
        </section>
        <section>
          <h2 style={{ fontSize: 16 }}>{tr("privacy.storage-period")}</h2>
          <p>
            {tr("privacy.text-3")}
          </p>
        </section>
        <section>
          <h2 style={{ fontSize: 16 }}>{tr("privacy.your-rights")}</h2>
          <p>
            {tr("privacy.text-4")}
          </p>
        </section>
        <section>
          <h2 style={{ fontSize: 16 }}>{tr("privacy.export-my-data-dsar")}</h2>
          <p>
            {tr("privacy.text-5")}
          </p>
          <button className="btn btn-primary" data-testid="export-data" disabled={busy} onClick={exportData}>
            {tr("privacy.download-my-data")}
          </button>
        </section>
      </div>

      <div className="card card-pad" style={{ marginTop: 12 }}>
        {status === null ? (
          <div className="spinner">{tr("privacy.loading")}</div>
        ) : status.accepted_at ? (
          <p data-testid="consent-status">
            {tr("privacy.text-6")} {status.notice_version} {tr("privacy.text-7")}{" "}
            {thDate(status.accepted_at.slice(0, 10))}
          </p>
        ) : (
          <>
            <p>{tr("privacy.text-8")} {status.notice_version}</p>
            <button className="btn btn-primary" data-testid="accept-consent" disabled={busy} onClick={accept}>
              {tr("privacy.accept-and-acknowledge-the-announcement")}
            </button>
          </>
        )}
      </div>
    </>
  );
}

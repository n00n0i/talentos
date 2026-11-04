"use client";

import type { TranslationKey } from "@/lib/i18n-catalog";

import { useI18N, tNow } from "@/lib/i18n";

import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, gw, handleApiError, thDate } from "@/lib/api";

type Day = {
  work_date: string;
  worked_minutes: number;
  late_minutes: number;
  early_leave_minutes: number;
  ot_minutes: number;
  incomplete: boolean;
  status: string;
};

type Fix = { lat: number; lng: number; accuracy: number };

type QueuedPunch = {
  client_id: string;
  kind: "in" | "out";
  lat: number;
  lng: number;
  accuracy: number;
  at: string;
};

const QUEUE_KEY = "talentos_clock_queue";

function readQueue(): QueuedPunch[] {
  try {
    const raw = localStorage.getItem(QUEUE_KEY);
    return raw ? (JSON.parse(raw) as QueuedPunch[]) : [];
  } catch {
    return [];
  }
}

function writeQueue(items: QueuedPunch[]) {
  try {
    localStorage.setItem(QUEUE_KEY, JSON.stringify(items));
  } catch {
    // storage full/unavailable: drops the queue rather than breaking the punch
  }
}

const STATUS: Record<string, { labelKey: TranslationKey; tone: string }> = {
  pending: { labelKey: "clock.pending-approval", tone: "warning" },
  approved: { labelKey: "clock.approved", tone: "success" },
};

const bangkokToday = () =>
  new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Bangkok" }).format(new Date());

const hm = (min: number) => tNow("clock.hoursMinutes", { hours: Math.floor(min / 60), minutes: min % 60 });

function locate(): Promise<Fix> {
  return new Promise((resolve, reject) => {
    if (!navigator.geolocation) {
      reject(new Error(tNow("clock.this-device-does-not-support-geolocation")));
      return;
    }
    navigator.geolocation.getCurrentPosition(
      (p) =>
        resolve({ lat: p.coords.latitude, lng: p.coords.longitude, accuracy: p.coords.accuracy }),
      (err) =>
        reject(
          new Error(
            err.code === err.PERMISSION_DENIED
              ? tNow("clock.location-permission-not-available-turn-on")
              : tNow("clock.unable-to-determine-location-try-again"),
          ),
        ),
      { enableHighAccuracy: true, timeout: 15000, maximumAge: 0 },
    );
  });
}

export default function ClockPage() {
  const { t: tr } = useI18N();
  const [today] = useState(bangkokToday);
  const [days, setDays] = useState<Day[] | null>(null);
  const [linked, setLinked] = useState(true);
  const [forbidden, setForbidden] = useState(false);
  const [fix, setFix] = useState<Fix | null>(null);
  const [geoError, setGeoError] = useState("");
  const [distance, setDistance] = useState<{ away: string; allowed: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [syncMsg, setSyncMsg] = useState("");
  const flushing = useRef(false);

  const loadDays = useCallback(async () => {
    try {
      const r = await gw<{ employee_linked: boolean; days: Day[] }>(
        `api/v1/me/attendance?month=${today.slice(0, 7)}`,
      );
      setLinked(r.employee_linked);
      setDays(r.days);
    } catch (e) {
      if (e instanceof ApiError && e.status === 403) setForbidden(true);
      else handleApiError(e, setError);
    }
  }, [today]);

  const flushQueue = useCallback(async () => {
    const queue = readQueue();
    if (queue.length === 0) return;
    let synced = 0;
    let skipped = 0;
    const handled = new Set<string>();
    for (let i = 0; i < queue.length; i++) {
      const item = queue[i];
      try {
        await gw("api/v1/me/attendance/punch", {
          method: "POST",
          body: JSON.stringify({
            client_id: item.client_id,
            kind: item.kind,
            lat: item.lat,
            lng: item.lng,
            accuracy_m: item.accuracy,
            client_time: item.at,
          }),
        });
        handled.add(item.client_id);
        synced += 1;
      } catch (e) {
        if (
          e instanceof ApiError &&
          e.status >= 400 &&
          e.status < 500 &&
          e.status !== 408 &&
          e.status !== 429
        ) {
          // Business rejection (e.g. outside geofence, stale timestamp): retrying
          // will not change it. Transient/auth errors stay queued below.
          handled.add(item.client_id);
          skipped += 1;
        } else {
          break; // still offline/transient: keep this and the rest for later
        }
      }
    }
    // Re-read: punches queued while this flush was running must survive.
    writeQueue(readQueue().filter((q) => !handled.has(q.client_id)));
    if (synced > 0 || skipped > 0) {
      const parts = [];
      if (synced > 0) parts.push(tNow("clock.syncedNotice", { n: synced }));
      if (skipped > 0) parts.push(tNow("clock.skippedNotice", { n: skipped }));
      setSyncMsg(parts.join(" · "));
      await loadDays();
    }
  }, [loadDays]);

  const refreshLocation = useCallback(() => {
    setGeoError("");
    locate().then(setFix, (e: Error) => setGeoError(e.message));
  }, []);

  useEffect(() => {
    loadDays();
    refreshLocation();
  }, [loadDays, refreshLocation]);

  // Flush any queued offline punches on mount and whenever the network returns.
  useEffect(() => {
    const run = () => {
      if (flushing.current) return;
      flushing.current = true;
      flushQueue().finally(() => {
        flushing.current = false;
      });
    };
    run();
    window.addEventListener("online", run);
    return () => window.removeEventListener("online", run);
  }, [flushQueue]);

  async function punch(kind: "in" | "out") {
    setBusy(true);
    setError("");
    setNotice("");
    setSyncMsg("");
    setDistance(null);
    const client_id = crypto.randomUUID();
    const at = new Date().toISOString();
    let here: Fix;
    try {
      here = await locate();
      setFix(here);
    } catch (e) {
      handleApiError(e, setError);
      setBusy(false);
      return;
    }
    try {
      await gw("api/v1/me/attendance/punch", {
        method: "POST",
        body: JSON.stringify({
          client_id,
          kind,
          lat: here.lat,
          lng: here.lng,
          accuracy_m: here.accuracy,
        }),
      });
      setNotice(kind === "in" ? tr("clock.clocked-in") : tr("clock.clocked-out"));
      await loadDays();
    } catch (e) {
      if (e instanceof ApiError) {
        const m = /outside geofence: ([\d.]+)m > ([\d.]+)m/.exec(e.message);
        if (m) setDistance({ away: Math.round(Number(m[1])).toString(), allowed: m[2] });
        else handleApiError(e, setError);
      } else {
        // Network failure: queue the punch and replay it idempotently later.
        const queue = readQueue();
        queue.push({ client_id, kind, lat: here.lat, lng: here.lng, accuracy: here.accuracy, at });
        writeQueue(queue);
        setNotice(tr("clock.saved-on-the-machine-will-sync"));
      }
    } finally {
      setBusy(false);
    }
  }

  const todayRow = days?.find((d) => d.work_date === today);

  if (forbidden) {
    return (
      <>
        <div className="page-head">
          <h1>{tr("clock.time-clock")}</h1>
        </div>
        <div className="card">
          <div className="empty">{tr("clock.this-account-does-not-have-time")}</div>
        </div>
      </>
    );
  }

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{tr("clock.time-clock")}</h1>
          <p className="page-sub">{thDate(today)} {tr("clock.text")}</p>
        </div>
      </div>

      {!linked && days !== null ? (
        <div className="card">
          <div className="empty">
            {tr("clock.this-account-has-not-been-linked")}
          </div>
        </div>
      ) : (
        <>
          <div className="card card-pad" style={{ display: "grid", gap: 14, maxWidth: 520 }}>
            {geoError ? (
              <p className="error" role="alert">
                {geoError}
              </p>
            ) : fix ? (
              <p className="hint">
                {tr("clock.text-2")}{Math.round(fix.accuracy)} {tr("clock.text-3")}
              </p>
            ) : (
              <p className="hint">{tr("clock.locating")}</p>
            )}

            {distance && (
              <p className="error" role="alert">
                {tr("clock.text-4")} {distance.away} {tr("clock.text-5")} {distance.allowed} {tr("clock.text-6")}
              </p>
            )}
            {error && (
              <p className="error" role="alert">
                {error}
              </p>
            )}
            {notice && (
              <p className="badge success" data-testid="queue-status">
                {notice}
              </p>
            )}
            {syncMsg && (
              <p className="badge success" data-testid="sync-status">
                {syncMsg}
              </p>
            )}

            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
              <button
                className="btn btn-primary"
                style={{ height: 56, fontSize: 16 }}
                disabled={busy || !!geoError}
                onClick={() => punch("in")}
              >
                {tr("clock.clock-in")}
              </button>
              <button
                className="btn"
                style={{ height: 56, fontSize: 16 }}
                disabled={busy || !!geoError}
                onClick={() => punch("out")}
              >
                {tr("clock.clock-out")}
              </button>
            </div>
            <button className="btn btn-sm" onClick={refreshLocation} disabled={busy}>
              {tr("clock.update-location")}
            </button>
          </div>

          <div className="card" style={{ marginTop: 16 }}>
            <div className="card-head">
              <strong>{tr("clock.today")}</strong>
            </div>
            {todayRow ? (
              <div className="card-pad" data-testid="today-summary">
                {tr("clock.work")} {hm(todayRow.worked_minutes)}
                {todayRow.late_minutes > 0 && tNow("clock.lateNotice", { minutes: todayRow.late_minutes })}
                {todayRow.incomplete && tr("clock.the-entry-exit-time-has-not")}
              </div>
            ) : (
              <div className="empty">{tr("clock.there-are-no-records-today")}</div>
            )}
          </div>

          <div className="card" style={{ marginTop: 16 }}>
            <div className="card-head">
              <strong>{tr("clock.this-month")}</strong>
            </div>
            {days === null ? (
              <div className="spinner">{tr("clock.loading")}</div>
            ) : days.length === 0 ? (
              <div className="empty">{tr("clock.there-are-no-records-for-this")}</div>
            ) : (
              <table className="table">
                <thead>
                  <tr>
                    <th scope="col">{tr("clock.date")}</th>
                    <th scope="col" className="num">{tr("clock.work")}</th>
                    <th scope="col" className="num">{tr("clock.late-minutes")}</th>
                    <th scope="col" className="num">{tr("clock.ot-minute")}</th>
                    <th scope="col">{tr("clock.status")}</th>
                  </tr>
                </thead>
                <tbody>
                  {days.map((d) => {
                    const s = STATUS[d.status] ?? { label: d.status, tone: "" };
                    return (
                      <tr key={d.work_date} data-testid="day-row">
                        <td>{thDate(d.work_date)}</td>
                        <td className="num">{hm(d.worked_minutes)}</td>
                        <td className="num">{d.late_minutes}</td>
                        <td className="num">{d.ot_minutes}</td>
                        <td>
                          <span className={`badge ${s.tone}`}>
                            {d.incomplete ? tr("clock.not-complete") : tr(s.labelKey)}
                          </span>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            )}
          </div>
        </>
      )}
    </>
  );
}

"use client";

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

const STATUS: Record<string, { label: string; tone: string }> = {
  pending: { label: "รออนุมัติ", tone: "warning" },
  approved: { label: "อนุมัติแล้ว", tone: "success" },
};

const bangkokToday = () =>
  new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Bangkok" }).format(new Date());

const hm = (min: number) => `${Math.floor(min / 60)} ชม. ${min % 60} นาที`;

function locate(): Promise<Fix> {
  return new Promise((resolve, reject) => {
    if (!navigator.geolocation) {
      reject(new Error("อุปกรณ์นี้ไม่รองรับการระบุตำแหน่ง"));
      return;
    }
    navigator.geolocation.getCurrentPosition(
      (p) =>
        resolve({ lat: p.coords.latitude, lng: p.coords.longitude, accuracy: p.coords.accuracy }),
      (err) =>
        reject(
          new Error(
            err.code === err.PERMISSION_DENIED
              ? "ไม่ได้รับอนุญาตให้ใช้ตำแหน่ง — เปิดสิทธิ์ตำแหน่งในเบราว์เซอร์แล้วลองอีกครั้ง"
              : "ไม่สามารถระบุตำแหน่งได้ ลองอีกครั้งในที่โล่ง",
          ),
        ),
      { enableHighAccuracy: true, timeout: 15000, maximumAge: 0 },
    );
  });
}

export default function ClockPage() {
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
      if (synced > 0) parts.push(`ซิงก์แล้ว ${synced} รายการ`);
      if (skipped > 0) parts.push(`ข้าม ${skipped} รายการ`);
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
      setNotice(kind === "in" ? "บันทึกเวลาแล้ว: เข้างาน" : "บันทึกเวลาแล้ว: ออกงาน");
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
        setNotice("บันทึกในเครื่องแล้ว จะซิงก์เมื่อออนไลน์");
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
          <h1>ลงเวลา</h1>
        </div>
        <div className="card">
          <div className="empty">บัญชีนี้ไม่มีสิทธิ์ลงเวลา — ติดต่อ HR</div>
        </div>
      </>
    );
  }

  return (
    <>
      <div className="page-head">
        <div>
          <h1>ลงเวลา</h1>
          <p className="page-sub">{thDate(today)} · ระบบบันทึกเวลาจากเซิร์ฟเวอร์</p>
        </div>
      </div>

      {!linked && days !== null ? (
        <div className="card">
          <div className="empty">
            ยังไม่ได้ผูกบัญชีนี้กับข้อมูลพนักงาน — ติดต่อ HR เพื่อเชื่อมบัญชี
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
                ตำแหน่งปัจจุบัน ±{Math.round(fix.accuracy)} ม.
              </p>
            ) : (
              <p className="hint">กำลังระบุตำแหน่ง…</p>
            )}

            {distance && (
              <p className="error" role="alert">
                อยู่นอกพื้นที่ลงเวลา — ห่างจุดลงเวลา {distance.away} ม. (อนุญาต {distance.allowed} ม.)
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
                เข้างาน
              </button>
              <button
                className="btn"
                style={{ height: 56, fontSize: 16 }}
                disabled={busy || !!geoError}
                onClick={() => punch("out")}
              >
                ออกงาน
              </button>
            </div>
            <button className="btn btn-sm" onClick={refreshLocation} disabled={busy}>
              อัปเดตตำแหน่ง
            </button>
          </div>

          <div className="card" style={{ marginTop: 16 }}>
            <div className="card-head">
              <strong>วันนี้</strong>
            </div>
            {todayRow ? (
              <div className="card-pad" data-testid="today-summary">
                ทำงาน {hm(todayRow.worked_minutes)}
                {todayRow.late_minutes > 0 && ` · สาย ${todayRow.late_minutes} นาที`}
                {todayRow.incomplete && " · ยังไม่ครบเวลาเข้า/ออก"}
              </div>
            ) : (
              <div className="empty">ยังไม่มีบันทึกวันนี้</div>
            )}
          </div>

          <div className="card" style={{ marginTop: 16 }}>
            <div className="card-head">
              <strong>เดือนนี้</strong>
            </div>
            {days === null ? (
              <div className="spinner">กำลังโหลด…</div>
            ) : days.length === 0 ? (
              <div className="empty">ยังไม่มีบันทึกเดือนนี้</div>
            ) : (
              <table className="table">
                <thead>
                  <tr>
                    <th>วันที่</th>
                    <th className="num">ทำงาน</th>
                    <th className="num">สาย (นาที)</th>
                    <th className="num">OT (นาที)</th>
                    <th>สถานะ</th>
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
                            {d.incomplete ? "ไม่ครบ" : s.label}
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

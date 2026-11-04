"use client";

import type { TranslationKey } from "@/lib/i18n-catalog";

import { useI18N, tNow } from "@/lib/i18n";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, thDate } from "@/lib/api";
import { ThaiMonthInput } from "@/components/ThaiDateTimeInput";

type Holiday = { name: string };
type Leave = {
  employee_code: string;
  employee_name: string;
  leave_type: string;
  label_th: string;
  start_date: string;
  end_date: string;
  status: string;
};
type Ot = { employee_code: string; employee_name: string; minutes: number; status: string };
type Day = { date: string; holidays: Holiday[]; leave: Leave[]; ot: Ot[] };

const bangkokMonth = () =>
  new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Bangkok" }).format(new Date()).slice(0, 7);

function monthRange(month: string) {
  const [y, m] = month.split("-").map(Number);
  const last = new Date(Date.UTC(y, m, 0)).getUTCDate();
  return { from: `${month}-01`, to: `${month}-${String(last).padStart(2, "0")}` };
}

const hm = (min: number) => `${Math.floor(min / 60)}:${String(min % 60).padStart(2, "0")}`;

const LEAVE_STATUS: Record<string, TranslationKey> = {
  pending: "calendar.pending-approval",
  approved: "calendar.approved",
};

const OT_STATUS: Record<string, TranslationKey> = {
  pending: "calendar.pending-approval",
  approved: "calendar.approved",
};

export default function CalendarPage() {
  const { t: tr } = useI18N();
  const [month, setMonth] = useState(bangkokMonth);
  const [days, setDays] = useState<Day[] | null>(null);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    const range = monthRange(month);
    const res = await gw<{ days: Day[] }>(
      `api/v1/calendar?from=${range.from}&to=${range.to}`,
    );
    setDays(res.days);
  }, [month]);

  useEffect(() => {
    load().catch((e) => handleApiError(e, setError));
  }, [load]);

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{tr("calendar.team-calendar")}</h1>
          <p className="page-sub">{tr("calendar.team-holidays-leave-and-ot-in")}</p>
        </div>
        <div style={{ display: "flex", gap: 12, alignItems: "center" }}>
          <ThaiMonthInput
            testId="calendar-month"
            ariaLabel={tr("calendar.month")}
            value={month}
            onChange={setMonth}
          />
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}

      {days === null ? (
        <div className="spinner">{tr("calendar.loading")}</div>
      ) : days.length === 0 ? (
        <div className="card">
          <div className="empty">{tr("calendar.there-are-no-holidays-leave-or")}</div>
        </div>
      ) : (
        <div style={{ display: "grid", gap: 12 }}>
          {days.map((d) => (
            <div className="card card-pad" key={d.date} data-testid="calendar-day">
              <div className="card-head">
                <h2 style={{ fontSize: 16 }}>{thDate(d.date)}</h2>
              </div>
              {d.holidays.length > 0 && (
                <div style={{ marginTop: 8 }}>
                  {d.holidays.map((h) => (
                    <span key={h.name} className="badge success" style={{ marginRight: 6 }}>
                      🎉 {h.name}
                    </span>
                  ))}
                </div>
              )}
              {d.leave.length > 0 && (
                <ul className="list" style={{ marginTop: 8 }}>
                  {d.leave.map((l, i) => (
                    <li key={i}>
                      <span className="badge warning" style={{ marginRight: 6 }}>
                        {l.label_th}
                      </span>
                      <span>
                        {l.employee_name} ({l.employee_code}) · {l.status in LEAVE_STATUS ? tr(LEAVE_STATUS[l.status]) : l.status}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
              {d.ot.length > 0 && (
                <ul className="list" style={{ marginTop: 8 }}>
                  {d.ot.map((o, i) => (
                    <li key={i}>
                      <span className="badge accent" style={{ marginRight: 6 }}>
                        OT {hm(o.minutes)} {tr("calendar.text")}
                      </span>
                      <span>
                        {o.employee_name} ({o.employee_code}) · {o.status in OT_STATUS ? tr(OT_STATUS[o.status]) : o.status}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          ))}
        </div>
      )}
    </>
  );
}

"use client";

import type { TranslationKey } from "@/lib/i18n-catalog";

import { useI18N, tNow } from "@/lib/i18n";

import ThaiDateInput from "@/components/ThaiDateInput";
import { ThaiDateTimeInput, ThaiMonthInput } from "@/components/ThaiDateTimeInput";

import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError } from "@/lib/api";

type Employee = { id: string; employee_code: string; first_name_th: string; last_name_th: string };
type Holiday = { id: string; holiday_date: string; name_th: string };
type Day = {
  id: string;
  employee_id: string;
  work_date: string;
  worked_minutes: number;
  late_minutes: number;
  early_leave_minutes: number;
  ot_minutes: number;
  derived_ot_minutes: number;
  ot_kind: string;
  incomplete: boolean;
  status: string;
};

const STATUS: Record<string, { labelKey: TranslationKey; tone: string }> = {
  pending: { labelKey: "attendance.pending-approval", tone: "warning" },
  approved: { labelKey: "attendance.approved", tone: "success" },
};

const hm = (min: number) => `${Math.floor(min / 60)}:${String(min % 60).padStart(2, "0")}`;

const OT_KIND: Record<string, TranslationKey> = {
  weekday: "attendance.weekday-1-5x",
  weekend: "attendance.weekend-2x",
  holiday: "attendance.public-holiday-3x",
};

const FILTERS: { value: string; labelKey: TranslationKey }[] = [
  { value: "all", labelKey: "attendance.all" },
  { value: "pending", labelKey: "attendance.pending-approval" },
  { value: "incomplete", labelKey: "attendance.not-complete" },
  { value: "late", labelKey: "attendance.late" },
  { value: "early", labelKey: "attendance.early-leave" },
];

const bangkokMonth = () =>
  new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Bangkok" }).format(new Date()).slice(0, 7);

function monthRange(month: string) {
  const [y, m] = month.split("-").map(Number);
  const last = new Date(Date.UTC(y, m, 0)).getUTCDate();
  return { from: `${month}-01`, to: `${month}-${String(last).padStart(2, "0")}` };
}

export default function AttendancePage() {
  const { t: tr } = useI18N();
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [days, setDays] = useState<Day[] | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [inAt, setInAt] = useState("");
  const [outAt, setOutAt] = useState("");
  const [holidays, setHolidays] = useState<Holiday[]>([]);
  const [holidayDate, setHolidayDate] = useState("");
  const [holidayName, setHolidayName] = useState("");
  const [month, setMonth] = useState(bangkokMonth);
  const [filter, setFilter] = useState("all");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    const range = monthRange(month);
    const [emps, hols, derived] = await Promise.all([
      gw<{ employees: Employee[] }>("api/v1/employees?status=active&limit=500"),
      gw<{ holidays: Holiday[] }>(`api/v1/attendance/holidays?year=${month.slice(0, 4)}`),
      gw<{ days: Day[] }>(`api/v1/attendance/days?from=${range.from}&to=${range.to}`),
    ]);
    setDays(derived.days);
    setEmployees(emps.employees);
    setHolidays(hols.holidays);
  }, [month]);

  useEffect(() => {
    load().catch((e) => handleApiError(e, setError));
  }, [load]);

  const visible = (days ?? []).filter((d) =>
    filter === "pending"
      ? d.status === "pending"
      : filter === "incomplete"
        ? d.incomplete
        : filter === "late"
          ? d.late_minutes > 0
          : filter === "early"
            ? d.early_leave_minutes > 0
            : true,
  );

  function startCorrection(day: Day) {
    setEditing(day.id);
    setInAt(`${day.work_date}T09:00`);
    setOutAt(`${day.work_date}T18:00`);
  }

  async function dayAction(done: string, path: string, body?: object) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw(path, { method: "POST", body: body && JSON.stringify(body) });
      setNotice(done);
      setEditing(null);
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  const correct = (day: Day) =>
    dayAction(tr("attendance.edit-saved"), `api/v1/attendance/days/${day.id}/correct`, {
      in_at: `${inAt}:00+07:00`,
      out_at: `${outAt}:00+07:00`,
      note: "HR correction",
    });

  async function addHoliday(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      await gw("api/v1/attendance/holidays", {
        method: "POST",
        body: JSON.stringify({ holiday_date: holidayDate, name_th: holidayName }),
      });
      setHolidayDate("");
      setHolidayName("");
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function removeHoliday(holiday: Holiday) {
    if (!confirm(tr("attendance.deleteHolidayConfirm", { name: holiday.name_th, date: holiday.holiday_date }))) return;
    setBusy(true);
    try {
      await gw(`api/v1/attendance/holidays/${holiday.id}`, { method: "DELETE" });
      await load();
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
          <h1>{tr("attendance.time-ot")}</h1>
          <p className="page-sub">{tr("attendance.approve-time-calculated-workdays-only-approved")}</p>
        </div>
        <div style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap" }}>
          <ThaiMonthInput
            testId="month-picker"
            ariaLabel={tr("attendance.month")}
            value={month}
            onChange={setMonth}
          />
          <select data-testid="day-filter" aria-label={tr("attendance.filter")} value={filter} onChange={(e) => setFilter(e.target.value)}>
            {FILTERS.map((f) => (
              <option key={f.value} value={f.value}>
                {tr(f.labelKey)}
              </option>
            ))}
          </select>
          <Link className="btn btn-sm" href="/attendance/shifts">
            {tr("attendance.manage-shifts")}
          </Link>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="badge success" role="status" aria-live="polite">{notice}</p>}

      <div className="card">
        <div className="card-head">
          <h2>{tr("attendance.working-days-calculated-from-time-stamp")}</h2>
          <span className="hint">{tr("attendance.thai-time-utc-7")}</span>
        </div>
        {days === null ? (
          <div className="spinner">{tr("attendance.loading")}</div>
        ) : visible.length === 0 ? (
          <div className="empty">{tr("attendance.there-are-no-work-days-according")}</div>
        ) : (
          <div style={{ overflowX: "auto" }}>
            <table className="table">
              <thead>
                <tr>
                  <th scope="col">{tr("attendance.date")}</th>
                  <th scope="col">{tr("attendance.employee")}</th>
                  <th scope="col" className="num">{tr("attendance.work")}</th>
                  <th scope="col" className="num">{tr("attendance.late-minutes")}</th>
                  <th scope="col" className="num">{tr("attendance.leave-early-minutes")}</th>
                  <th scope="col" className="num">OT</th>
                  <th scope="col">{tr("attendance.ot-type")}</th>
                  <th scope="col">{tr("attendance.status")}</th>
                  <th scope="col" />
                </tr>
              </thead>
              <tbody>
                {visible.map((d) => {
                  const st = STATUS[d.status] ?? { label: d.status, tone: "" };
                  return (
                    <tr key={d.id} data-testid="day-row">
                      <td>{d.work_date}</td>
                      <td>{employees.find((e) => e.id === d.employee_id)?.employee_code ?? "—"}</td>
                      <td className="num">{hm(d.worked_minutes)}</td>
                      <td className="num">{d.late_minutes}</td>
                      <td className="num">{d.early_leave_minutes}</td>
                      <td className="num">
                        {hm(d.ot_minutes)}
                        {d.ot_minutes > d.derived_ot_minutes && (
                          <span className="hint"> {tr("attendance.ot-upon-request")}</span>
                        )}
                      </td>
                      <td>{d.ot_minutes > 0 ? (d.ot_kind in OT_KIND ? tr(OT_KIND[d.ot_kind]) : d.ot_kind) : "—"}</td>
                      <td>
                        <span className={`badge ${st?.tone ?? ""}`}>{st ? tr(st.labelKey) : d.status}</span>
                        {d.incomplete && <span className="hint"> {tr("attendance.incomplete")}</span>}
                      </td>
                      <td className="num">
                        {d.status === "approved" ? null : editing === d.id ? (
                          <div style={{ display: "grid", gap: 6, justifyItems: "end" }}>
                            <ThaiDateTimeInput testId="correct-in" value={inAt} onChange={setInAt} />
                            <ThaiDateTimeInput testId="correct-out" value={outAt} onChange={setOutAt} />
                            <span className="hint">{tr("attendance.thai-time-utc-7")}</span>
                            <span style={{ display: "flex", gap: 6 }}>
                              <button className="btn btn-sm" onClick={() => setEditing(null)} disabled={busy}>
                                {tr("attendance.cancel")}
                              </button>
                              <button
                                className="btn btn-sm btn-primary"
                                data-testid="save-correction"
                                onClick={() => correct(d)}
                                disabled={busy || !inAt || !outAt}
                              >
                                {tr("attendance.save")}
                              </button>
                            </span>
                          </div>
                        ) : (
                          <span style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                            <button className="btn btn-sm" data-testid="correct-day" onClick={() => startCorrection(d)} disabled={busy}>
                              {tr("attendance.edit")}
                            </button>
                            <button
                              className="btn btn-sm btn-primary"
                              data-testid="approve-day"
                              onClick={() => dayAction(tr("attendance.working-days-approved"), `api/v1/attendance/days/${d.id}/approve`)}
                              disabled={busy || d.incomplete}
                              title={d.incomplete ? tr("attendance.the-time-must-be-completely-revised") : undefined}
                            >
                              {tr("attendance.approve")}
                            </button>
                          </span>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <div className="grid-2">
        <form className="card card-pad" onSubmit={addHoliday} style={{ display: "grid", gap: 12 }}>
          <div>
            <h2>{tr("attendance.holiday-calendar")}</h2>
            <p className="hint" style={{ marginTop: 4 }}>
              {tr("attendance.used-for-automatic-ot-classification-3x")}
            </p>
          </div>
          <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label>{tr("attendance.date")}</label>
              <ThaiDateInput testId="holiday-date" value={holidayDate} onChange={(v) => setHolidayDate(v)} required />
            </div>
            <div className="field">
              <label>{tr("attendance.holiday-name")}</label>
              <input
                data-testid="holiday-name"
                value={holidayName}
                onChange={(e) => setHolidayName(e.target.value)}
                placeholder={tr("attendance.such-as-loy-krathong-day")}
                required
              />
            </div>
          </div>
          <div>
            <button className="btn btn-primary btn-sm" data-testid="add-holiday" type="submit" disabled={busy}>
              {tr("attendance.add-a-holiday")}
            </button>
          </div>
        </form>

        <div className="card">
          <div className="card-head">
            <h2>{tr("attendance.holidays-this-year")}</h2>
            <span className="hint">{holidays.length} {tr("attendance.day")}</span>
          </div>
          {holidays.length === 0 ? (
            <div className="empty" style={{ padding: "24px 18px" }}>
              {tr("attendance.no-holidays-yet-added-to-allow")}
            </div>
          ) : (
            <table className="table">
              <thead>
                <tr>
                  <th scope="col">{tr("attendance.date")}</th>
                  <th scope="col">{tr("attendance.name")}</th>
                  <th scope="col" />
                </tr>
              </thead>
              <tbody>
                {holidays.map((h) => (
                  <tr key={h.id}>
                    <td>{h.holiday_date}</td>
                    <td>{h.name_th}</td>
                    <td className="num">
                      <button
                        className="btn btn-sm btn-danger"
                        onClick={() => removeHoliday(h)}
                        disabled={busy}
                      >
                        {tr("attendance.delete")}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>
    </>
  );
}

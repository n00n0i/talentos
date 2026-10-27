"use client";

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
  ot_kind: string;
  incomplete: boolean;
  status: string;
};

const STATUS: { [k: string]: { label: string; tone: string } } = {
  pending: { label: "รออนุมัติ", tone: "warning" },
  approved: { label: "อนุมัติแล้ว", tone: "success" },
};

const hm = (min: number) => `${Math.floor(min / 60)}:${String(min % 60).padStart(2, "0")}`;

const OT_KIND: { [k: string]: string } = {
  weekday: "วันทำงาน (1.5x)",
  weekend: "วันหยุด (2x)",
  holiday: "วันหยุดนักขัตฤกษ์ (3x)",
};

const FILTERS = [
  { value: "all", label: "ทั้งหมด" },
  { value: "pending", label: "รออนุมัติ" },
  { value: "incomplete", label: "ไม่ครบ" },
  { value: "late", label: "มาสาย" },
  { value: "early", label: "ออกก่อน" },
];

const bangkokMonth = () =>
  new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Bangkok" }).format(new Date()).slice(0, 7);

function monthRange(month: string) {
  const [y, m] = month.split("-").map(Number);
  const last = new Date(Date.UTC(y, m, 0)).getUTCDate();
  return { from: `${month}-01`, to: `${month}-${String(last).padStart(2, "0")}` };
}

export default function AttendancePage() {
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
    dayAction("บันทึกการแก้ไขแล้ว", `api/v1/attendance/days/${day.id}/correct`, {
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
    if (!confirm(`ลบวันหยุด ${holiday.name_th} (${holiday.holiday_date})?`)) return;
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
          <h1>เวลาและ OT</h1>
          <p className="page-sub">อนุมัติวันทำงานที่คำนวณจากการลงเวลา — เฉพาะวันที่อนุมัติแล้วเท่านั้นที่เข้ารอบเงินเดือน</p>
        </div>
        <div style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap" }}>
          <input
            data-testid="month-picker"
            type="month"
            aria-label="เดือน"
            value={month}
            onChange={(e) => e.target.value && setMonth(e.target.value)}
          />
          <select data-testid="day-filter" aria-label="ตัวกรอง" value={filter} onChange={(e) => setFilter(e.target.value)}>
            {FILTERS.map((f) => (
              <option key={f.value} value={f.value}>
                {f.label}
              </option>
            ))}
          </select>
          <Link className="btn btn-sm" href="/attendance/shifts">
            จัดการกะ
          </Link>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      <div className="card">
        <div className="card-head">
          <h2>วันทำงาน (คำนวณจากการลงเวลา)</h2>
          <span className="hint">เวลาไทย (UTC+7)</span>
        </div>
        {days === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : visible.length === 0 ? (
          <div className="empty">ไม่มีวันทำงานตามตัวกรองในเดือนนี้</div>
        ) : (
          <div style={{ overflowX: "auto" }}>
            <table className="table">
              <thead>
                <tr>
                  <th>วันที่</th>
                  <th>พนักงาน</th>
                  <th className="num">ทำงาน</th>
                  <th className="num">สาย (นาที)</th>
                  <th className="num">ออกก่อน (นาที)</th>
                  <th className="num">OT</th>
                  <th>ประเภท OT</th>
                  <th>สถานะ</th>
                  <th />
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
                      <td className="num">{hm(d.ot_minutes)}</td>
                      <td>{d.ot_minutes > 0 ? (OT_KIND[d.ot_kind] ?? d.ot_kind) : "—"}</td>
                      <td>
                        <span className={`badge ${st.tone}`}>{st.label}</span>
                        {d.incomplete && <span className="hint"> · ไม่ครบ</span>}
                      </td>
                      <td className="num">
                        {d.status === "approved" ? null : editing === d.id ? (
                          <div style={{ display: "grid", gap: 6, justifyItems: "end" }}>
                            <input data-testid="correct-in" type="datetime-local" value={inAt} onChange={(e) => setInAt(e.target.value)} />
                            <input data-testid="correct-out" type="datetime-local" value={outAt} onChange={(e) => setOutAt(e.target.value)} />
                            <span className="hint">เวลาไทย (UTC+7)</span>
                            <span style={{ display: "flex", gap: 6 }}>
                              <button className="btn btn-sm" onClick={() => setEditing(null)} disabled={busy}>
                                ยกเลิก
                              </button>
                              <button
                                className="btn btn-sm btn-primary"
                                data-testid="save-correction"
                                onClick={() => correct(d)}
                                disabled={busy || !inAt || !outAt}
                              >
                                บันทึก
                              </button>
                            </span>
                          </div>
                        ) : (
                          <span style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                            <button className="btn btn-sm" data-testid="correct-day" onClick={() => startCorrection(d)} disabled={busy}>
                              แก้ไข
                            </button>
                            <button
                              className="btn btn-sm btn-primary"
                              data-testid="approve-day"
                              onClick={() => dayAction("อนุมัติวันทำงานแล้ว", `api/v1/attendance/days/${d.id}/approve`)}
                              disabled={busy || d.incomplete}
                              title={d.incomplete ? "ต้องแก้ไขเวลาให้ครบก่อนอนุมัติ" : undefined}
                            >
                              อนุมัติ
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
            <h2>ปฏิทินวันหยุด</h2>
            <p className="hint" style={{ marginTop: 4 }}>
              ใช้จัดประเภท OT อัตโนมัติ (วันหยุดนักขัตฤกษ์ 3x)
            </p>
          </div>
          <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label>วันที่</label>
              <input
                data-testid="holiday-date"
                type="date"
                value={holidayDate}
                onChange={(e) => setHolidayDate(e.target.value)}
                required
              />
            </div>
            <div className="field">
              <label>ชื่อวันหยุด</label>
              <input
                data-testid="holiday-name"
                value={holidayName}
                onChange={(e) => setHolidayName(e.target.value)}
                placeholder="เช่น วันลอยกระทง"
                required
              />
            </div>
          </div>
          <div>
            <button className="btn btn-primary btn-sm" data-testid="add-holiday" type="submit" disabled={busy}>
              เพิ่มวันหยุด
            </button>
          </div>
        </form>

        <div className="card">
          <div className="card-head">
            <h2>วันหยุดในปีนี้</h2>
            <span className="hint">{holidays.length} วัน</span>
          </div>
          {holidays.length === 0 ? (
            <div className="empty" style={{ padding: "24px 18px" }}>
              ยังไม่มีวันหยุด — เพิ่มเพื่อให้ OT จัดประเภทเอง
            </div>
          ) : (
            <table className="table">
              <thead>
                <tr>
                  <th>วันที่</th>
                  <th>ชื่อ</th>
                  <th />
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
                        ลบ
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

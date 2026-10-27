"use client";

import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError } from "@/lib/api";

type Employee = { id: string; employee_code: string; first_name_th: string; last_name_th: string };
type Holiday = { id: string; holiday_date: string; name_th: string };
type Record = {
  id: string;
  employee_id: string;
  employee_code: string;
  work_date: string;
  hours_worked: string;
  ot_hours: string;
  ot_kind: string;
  note: string | null;
};

type Day = {
  id: string;
  employee_id: string;
  work_date: string;
  worked_minutes: number;
  late_minutes: number;
  ot_minutes: number;
  incomplete: boolean;
  status: string;
};

const STATUS: { [k: string]: { label: string; tone: string } } = {
  pending: { label: "รออนุมัติ", tone: "warning" },
  approved: { label: "อนุมัติแล้ว", tone: "success" },
};

const hm = (min: number) => `${Math.floor(min / 60)}:${String(min % 60).padStart(2, "0")}`;

const KINDS = [
  { value: "", label: "อัตโนมัติ (ตามวัน/ปฏิทิน)" },
  { value: "weekday", label: "วันทำงาน (1.5x)" },
  { value: "weekend", label: "วันหยุด (2x)" },
  { value: "holiday", label: "วันหยุดนักขัตฤกษ์ (3x)" },
];

function monthRange() {
  const now = new Date();
  const first = new Date(now.getFullYear(), now.getMonth(), 1);
  const last = new Date(now.getFullYear(), now.getMonth() + 1, 0);
  const iso = (d: Date) => d.toISOString().slice(0, 10);
  return { from: iso(first), to: iso(last) };
}

export default function AttendancePage() {
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [records, setRecords] = useState<Record[] | null>(null);
  const [days, setDays] = useState<Day[] | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [inAt, setInAt] = useState("");
  const [outAt, setOutAt] = useState("");
  const [holidays, setHolidays] = useState<Holiday[]>([]);
  const [holidayDate, setHolidayDate] = useState("");
  const [holidayName, setHolidayName] = useState("");
  const [employeeId, setEmployeeId] = useState("");
  const [workDate, setWorkDate] = useState(new Date().toISOString().slice(0, 10));
  const [otHours, setOtHours] = useState("0");
  const [otKind, setOtKind] = useState("");
  const [note, setNote] = useState("");
  const [range] = useState(monthRange);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    const year = range.from.slice(0, 4);
    const [emps, recs, hols, derived] = await Promise.all([
      gw<{ employees: Employee[] }>("api/v1/employees?status=active&limit=500"),
      gw<{ records: Record[] }>(`api/v1/attendance?from=${range.from}&to=${range.to}`),
      gw<{ holidays: Holiday[] }>(`api/v1/attendance/holidays?year=${year}`),
      gw<{ days: Day[] }>(`api/v1/attendance/days?from=${range.from}&to=${range.to}`),
    ]);
    setDays(derived.days);
    setEmployees(emps.employees);
    setRecords(recs.records);
    setHolidays(hols.holidays);
    if (!employeeId && emps.employees[0]) setEmployeeId(emps.employees[0].id);
  }, [range.from, range.to, employeeId]);

  useEffect(() => {
    load().catch((e) => handleApiError(e, setError));
  }, [load]);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw("api/v1/attendance", {
        method: "POST",
        body: JSON.stringify({
          employee_id: employeeId,
          work_date: workDate,
          hours_worked: 8,
          ot_hours: Number(otHours || "0"),
          ot_kind: otKind || null,
          note: note.trim() || null,
        }),
      });
      setNotice("บันทึกเวลาแล้ว");
      setOtHours("0");
      setNote("");
      await load();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

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

  async function remove(record: Record) {
    if (!confirm(`ลบรายการวันที่ ${record.work_date} ของ ${record.employee_code}?`)) return;
    setBusy(true);
    try {
      await gw(`api/v1/attendance/${record.id}`, { method: "DELETE" });
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
          <p className="page-sub">บันทึก OT เพื่อให้รอบเงินเดือนคำนวณอัตราตามกฎหมาย</p>
        </div>
        <div style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap" }}>
          <span className="hint">
            {range.from} – {range.to}
          </span>
          <Link className="btn btn-sm" href="/attendance/shifts">
            จัดการกะ
          </Link>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      <div className="card">
        <div className="card-head">
          <h2>วันทำงาน (derived)</h2>
          <span className="hint">คำนวณจากการลงเวลาและกะ</span>
        </div>
        {days === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : days.length === 0 ? (
          <div className="empty">ยังไม่มีวันทำงานในเดือนนี้</div>
        ) : (
          <div style={{ overflowX: "auto" }}>
            <table className="table">
              <thead>
                <tr>
                  <th>วันที่</th>
                  <th>พนักงาน</th>
                  <th className="num">ทำงาน</th>
                  <th className="num">สาย (นาที)</th>
                  <th className="num">OT</th>
                  <th>สถานะ</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {days.map((d) => {
                  const st = STATUS[d.status] ?? { label: d.status, tone: "" };
                  return (
                    <tr key={d.id} data-testid="day-row">
                      <td>{d.work_date}</td>
                      <td>{employees.find((e) => e.id === d.employee_id)?.employee_code ?? "—"}</td>
                      <td className="num">{hm(d.worked_minutes)}</td>
                      <td className="num">{d.late_minutes}</td>
                      <td className="num">{hm(d.ot_minutes)}</td>
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
                              disabled={busy}
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

      <form className="card card-pad" onSubmit={submit}>
        <h2 style={{ marginBottom: 14 }}>บันทึกเวลา</h2>
        <div className="form-row">
          <div className="field">
            <label>พนักงาน</label>
            <select data-testid="attendance-employee" value={employeeId} onChange={(e) => setEmployeeId(e.target.value)} required>
              {employees.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.employee_code} · {e.first_name_th} {e.last_name_th}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label>วันที่</label>
            <input data-testid="attendance-date" type="date" value={workDate} onChange={(e) => setWorkDate(e.target.value)} required />
          </div>
          <div className="field">
            <label>ชั่วโมง OT</label>
            <input
              type="number"
              min="0"
              max="12"
              step="0.5"
              data-testid="attendance-ot"
              value={otHours}
              onChange={(e) => setOtHours(e.target.value)}
            />
          </div>
        </div>
        <div className="form-row" style={{ marginTop: 12, gridTemplateColumns: "1fr 1fr auto" }}>
          <div className="field">
            <label>ประเภท OT</label>
            <select
              data-testid="attendance-ot-kind"
              value={otKind}
              onChange={(e) => setOtKind(e.target.value)}
            >
              {KINDS.map((k) => (
                <option key={k.value} value={k.value}>
                  {k.label}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label>หมายเหตุ</label>
            <input value={note} onChange={(e) => setNote(e.target.value)} />
          </div>
          <div style={{ display: "flex", alignItems: "end" }}>
            <button className="btn btn-primary" data-testid="save-attendance" type="submit" disabled={busy || !employeeId}>
              {busy ? "กำลังบันทึก…" : "บันทึก"}
            </button>
          </div>
        </div>
      </form>

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

      <div className="card">
        {records === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : records.length === 0 ? (
          <div className="empty">ยังไม่มีรายการในเดือนนี้</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>วันที่</th>
                <th>พนักงาน</th>
                <th className="num">ชั่วโมงทำงาน</th>
                <th className="num">OT</th>
                <th>ประเภท</th>
                <th>หมายเหตุ</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {records.map((r) => (
                <tr key={r.id}>
                  <td>{r.work_date}</td>
                  <td>{r.employee_code}</td>
                  <td className="num">{r.hours_worked}</td>
                  <td className="num">{r.ot_hours}</td>
                  <td>{KINDS.find((k) => k.value === r.ot_kind)?.label ?? r.ot_kind}</td>
                  <td>{r.note ?? "—"}</td>
                  <td className="num">
                    <button className="btn btn-sm btn-danger" onClick={() => remove(r)} disabled={busy}>
                      ลบ
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}

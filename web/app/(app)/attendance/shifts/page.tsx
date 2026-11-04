"use client";

import { useI18N } from "@/lib/i18n";

import ThaiDateInput from "@/components/ThaiDateInput";

import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError } from "@/lib/api";

type Employee = { id: string; employee_code: string; first_name_th: string; last_name_th: string };
type Shift = {
  id: string;
  name: string;
  start: string;
  end: string;
  break_minutes: number;
  grace_minutes: number;
};

const today = () => new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Bangkok" }).format(new Date());

export default function ShiftsPage() {
  const { t: tr } = useI18N();
  const [shifts, setShifts] = useState<Shift[] | null>(null);
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [name, setName] = useState("");
  const [start, setStart] = useState("09:00");
  const [end, setEnd] = useState("18:00");
  const [breakMin, setBreakMin] = useState("60");
  const [graceMin, setGraceMin] = useState("5");
  const [shiftId, setShiftId] = useState("");
  const [employeeId, setEmployeeId] = useState("");
  const [effectiveFrom, setEffectiveFrom] = useState(today);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const load = useCallback(async () => {
    const [s, e] = await Promise.all([
      gw<{ shifts: Shift[] }>("api/v1/shifts"),
      gw<{ employees: Employee[] }>("api/v1/employees?status=active&limit=500"),
    ]);
    setShifts(s.shifts);
    setEmployees(e.employees);
    setShiftId((cur) => cur || s.shifts[0]?.id || "");
    setEmployeeId((cur) => cur || e.employees[0]?.id || "");
  }, []);

  useEffect(() => {
    load().catch((e) => handleApiError(e, setError));
  }, [load]);

  async function run(done: string, action: () => Promise<void>) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await action();
      setNotice(done);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  const create = (e: React.FormEvent) => {
    e.preventDefault();
    return run(tr("attendanceShifts.shift-created"), async () => {
      const created = await gw<{ id: string }>("api/v1/shifts", {
        method: "POST",
        body: JSON.stringify({
          name: name.trim(),
          start,
          end,
          break_minutes: Number(breakMin || "0"),
          grace_minutes: Number(graceMin || "0"),
        }),
      });
      setName("");
      setShiftId(created.id);
      await load();
    });
  };

  const assign = (e: React.FormEvent) => {
    e.preventDefault();
    return run(tr("attendanceShifts.shift-assigned"), async () => {
      await gw(`api/v1/shifts/${shiftId}/assign`, {
        method: "POST",
        body: JSON.stringify({ employee_id: employeeId, effective_from: effectiveFrom }),
      });
    });
  };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{tr("attendanceShifts.work-shift")}</h1>
          <p className="page-sub">{tr("attendanceShifts.create-shifts-and-assign-them-to")}</p>
        </div>
        <Link className="btn btn-sm" href="/attendance">
          {tr("attendanceShifts.time-and-ot")}
        </Link>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="badge success" role="status" aria-live="polite">{notice}</p>}

      <div className="grid-2">
        <form className="card card-pad" onSubmit={create} style={{ display: "grid", gap: 12 }}>
          <h2>{tr("attendanceShifts.create-a-shift")}</h2>
          <div className="field">
            <label htmlFor="shift-name">{tr("attendanceShifts.shift-name")}</label>
            <input id="shift-name" data-testid="shift-name" value={name} onChange={(e) => setName(e.target.value)} required />
          </div>
          <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label htmlFor="shift-start">{tr("attendanceShifts.start")}</label>
              <input id="shift-start" data-testid="shift-start" type="time" value={start} onChange={(e) => setStart(e.target.value)} required />
            </div>
            <div className="field">
              <label htmlFor="shift-end">{tr("attendanceShifts.finish")}</label>
              <input id="shift-end" data-testid="shift-end" type="time" value={end} onChange={(e) => setEnd(e.target.value)} required />
            </div>
          </div>
          <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label htmlFor="shift-break">{tr("attendanceShifts.break-minutes")}</label>
              <input id="shift-break" type="number" min="0" data-testid="shift-break" value={breakMin} onChange={(e) => setBreakMin(e.target.value)} />
            </div>
            <div className="field">
              <label htmlFor="shift-grace">{tr("attendanceShifts.late-grace-minutes")}</label>
              <input id="shift-grace" type="number" min="0" data-testid="shift-grace" value={graceMin} onChange={(e) => setGraceMin(e.target.value)} />
            </div>
          </div>
          <div>
            <button className="btn btn-primary" data-testid="save-shift" type="submit" disabled={busy}>
              {tr("attendanceShifts.create-a-shift")}
            </button>
          </div>
        </form>

        <form className="card card-pad" onSubmit={assign} style={{ display: "grid", gap: 12 }}>
          <h2>{tr("attendanceShifts.assign-shifts")}</h2>
          <div className="field">
            <label htmlFor="assign-shift-id">{tr("attendanceShifts.shift")}</label>
            <select id="assign-shift-id" data-testid="assign-shift-id" value={shiftId} onChange={(e) => setShiftId(e.target.value)} required>
              {(shifts ?? []).map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor="shift-employee">{tr("attendanceShifts.employee")}</label>
            <select id="shift-employee" data-testid="shift-employee" value={employeeId} onChange={(e) => setEmployeeId(e.target.value)} required>
              {employees.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.employee_code} · {e.first_name_th} {e.last_name_th}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label>{tr("attendanceShifts.effective-from")}</label>
            <ThaiDateInput value={effectiveFrom} onChange={(v) => setEffectiveFrom(v)} required />
          </div>
          <div>
            <button className="btn btn-primary" data-testid="assign-shift" type="submit" disabled={busy || !shiftId || !employeeId}>
              {tr("attendanceShifts.assign")}
            </button>
          </div>
        </form>
      </div>

      <div className="card">
        {shifts === null ? (
          <div className="spinner">{tr("attendanceShifts.loading")}</div>
        ) : shifts.length === 0 ? (
          <div className="empty">{tr("attendanceShifts.no-shifts-yet-create-first-shift")}</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{tr("attendanceShifts.shift-name")}</th>
                <th scope="col">{tr("attendanceShifts.time")}</th>
                <th scope="col" className="num">{tr("attendanceShifts.break-minutes")}</th>
                <th scope="col" className="num">{tr("attendanceShifts.grace-minutes")}</th>
              </tr>
            </thead>
            <tbody>
              {shifts.map((s) => (
                <tr key={s.id}>
                  <td>{s.name}</td>
                  <td>
                    {s.start.slice(0, 5)} – {s.end.slice(0, 5)}
                  </td>
                  <td className="num">{s.break_minutes}</td>
                  <td className="num">{s.grace_minutes}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}

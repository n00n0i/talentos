"use client";

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
    return run("สร้างกะแล้ว", async () => {
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
    return run("มอบหมายกะแล้ว", async () => {
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
          <h1>กะทำงาน</h1>
          <p className="page-sub">สร้างกะและมอบหมายให้พนักงานเพื่อคำนวณวันทำงานอัตโนมัติ</p>
        </div>
        <Link className="btn btn-sm" href="/attendance">
          ← เวลาและ OT
        </Link>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      <div className="grid-2">
        <form className="card card-pad" onSubmit={create} style={{ display: "grid", gap: 12 }}>
          <h2>สร้างกะ</h2>
          <div className="field">
            <label>ชื่อกะ</label>
            <input data-testid="shift-name" value={name} onChange={(e) => setName(e.target.value)} required />
          </div>
          <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label>เริ่ม</label>
              <input data-testid="shift-start" type="time" value={start} onChange={(e) => setStart(e.target.value)} required />
            </div>
            <div className="field">
              <label>เลิก</label>
              <input data-testid="shift-end" type="time" value={end} onChange={(e) => setEnd(e.target.value)} required />
            </div>
          </div>
          <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label>พัก (นาที)</label>
              <input type="number" min="0" data-testid="shift-break" value={breakMin} onChange={(e) => setBreakMin(e.target.value)} />
            </div>
            <div className="field">
              <label>ผ่อนผันสาย (นาที)</label>
              <input type="number" min="0" data-testid="shift-grace" value={graceMin} onChange={(e) => setGraceMin(e.target.value)} />
            </div>
          </div>
          <div>
            <button className="btn btn-primary" data-testid="save-shift" type="submit" disabled={busy}>
              สร้างกะ
            </button>
          </div>
        </form>

        <form className="card card-pad" onSubmit={assign} style={{ display: "grid", gap: 12 }}>
          <h2>มอบหมายกะ</h2>
          <div className="field">
            <label>กะ</label>
            <select data-testid="assign-shift-id" value={shiftId} onChange={(e) => setShiftId(e.target.value)} required>
              {(shifts ?? []).map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label>พนักงาน</label>
            <select data-testid="shift-employee" value={employeeId} onChange={(e) => setEmployeeId(e.target.value)} required>
              {employees.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.employee_code} · {e.first_name_th} {e.last_name_th}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label>มีผลตั้งแต่</label>
            <input type="date" value={effectiveFrom} onChange={(e) => setEffectiveFrom(e.target.value)} required />
          </div>
          <div>
            <button className="btn btn-primary" data-testid="assign-shift" type="submit" disabled={busy || !shiftId || !employeeId}>
              มอบหมาย
            </button>
          </div>
        </form>
      </div>

      <div className="card">
        {shifts === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : shifts.length === 0 ? (
          <div className="empty">ยังไม่มีกะ — สร้างกะแรกด้านบน</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>ชื่อกะ</th>
                <th>เวลา</th>
                <th className="num">พัก (นาที)</th>
                <th className="num">ผ่อนผัน (นาที)</th>
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

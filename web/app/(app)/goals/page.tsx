"use client";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, Me } from "@/lib/api";

type Employee = {
  id: string;
  employee_code: string;
  first_name_th: string;
  last_name_th: string;
};

type Cycle = { id: string; name: string; status: string };

type Goal = {
  id: string;
  employee_id: string;
  cycle_id: string | null;
  cycle_name: string | null;
  title: string;
  description: string | null;
  target: string | null;
  progress: number;
  status: "active" | "achieved" | "cancelled";
};

const emptyForm = { title: "", cycle_id: "", target: "", description: "" };

const STATUSES: Record<string, string> = {
  active: "กำลังทำ",
  achieved: "สำเร็จ",
  cancelled: "ยกเลิก",
};

export default function GoalsPage() {
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [cycles, setCycles] = useState<Cycle[]>([]);
  const [employeeId, setEmployeeId] = useState("");
  const [cycleFilter, setCycleFilter] = useState("");
  const [goals, setGoals] = useState<Goal[]>([]);
  const [form, setForm] = useState(emptyForm);
  const [drafts, setDrafts] = useState<Record<string, { progress: string; status: string }>>({});
  const [canWrite, setCanWrite] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const loadGoals = useCallback(async (id: string, cycleId: string) => {
    if (!id) {
      setGoals([]);
      return;
    }
    const query = cycleId ? `?cycle_id=${cycleId}` : "";
    const res = await gw<{ goals: Goal[] }>(`api/v1/employees/${id}/goals${query}`);
    setGoals(res.goals);
  }, []);

  useEffect(() => {
    gw<{ employees: Employee[] }>("api/v1/employees?status=all&limit=500")
      .then((res) => setEmployees(res.employees))
      .catch((e) => handleApiError(e, setError));
    gw<Me>("api/v1/auth/me")
      .then((me) => setCanWrite(me.permissions.includes("employee.update")))
      .catch((e) => handleApiError(e, setError));
    gw<{ cycles: Cycle[] }>("api/v1/performance/cycles")
      .then((res) => setCycles(res.cycles))
      .catch((e) => handleApiError(e, setError));
  }, []);

  useEffect(() => {
    loadGoals(employeeId, cycleFilter).catch((e) => handleApiError(e, setError));
  }, [employeeId, cycleFilter, loadGoals]);

  useEffect(() => {
    setDrafts(
      Object.fromEntries(
        goals.map((g) => [g.id, { progress: String(g.progress), status: g.status }]),
      ),
    );
  }, [goals]);

  function setDraft(id: string, patch: Partial<{ progress: string; status: string }>) {
    setDrafts((current) => {
      const base = current[id] ?? { progress: "0", status: "active" };
      return { ...current, [id]: { ...base, ...patch } };
    });
  }

  async function save(event: React.FormEvent) {
    event.preventDefault();
    if (!employeeId) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw(`api/v1/employees/${employeeId}/goals`, {
        method: "POST",
        body: JSON.stringify({
          title: form.title,
          cycle_id: form.cycle_id || null,
          target: form.target.trim() || null,
          description: form.description.trim() || null,
        }),
      });
      setForm(emptyForm);
      await loadGoals(employeeId, cycleFilter);
      setNotice("เพิ่มเป้าหมายแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function saveGoal(goal: Goal) {
    const draft = drafts[goal.id];
    if (!draft) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/goals/${goal.id}`, {
        method: "PATCH",
        body: JSON.stringify({
          progress: Number(draft.progress) || 0,
          status: draft.status,
        }),
      });
      await loadGoals(employeeId, cycleFilter);
      setNotice("บันทึกความคืบหน้าแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function remove(goal: Goal) {
    if (!confirm(`ลบเป้าหมาย ${goal.title} หรือไม่?`)) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/goals/${goal.id}`, { method: "DELETE" });
      await loadGoals(employeeId, cycleFilter);
      setNotice("ลบเป้าหมายแล้ว");
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
          <h1>เป้าหมายและ OKR</h1>
          <p className="page-sub">เป้าหมายรายพนักงาน ความคืบหน้า และสถานะ</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="success">{notice}</p>}

      <div className="card card-pad" style={{ marginBottom: 12, display: "grid", gap: 12 }}>
        <div className="grid-2" style={{ gridTemplateColumns: "1fr 1fr", gap: 12 }}>
          <label>
            พนักงาน
            <select
              data-testid="goal-employee"
              value={employeeId}
              onChange={(e) => {
                setGoals([]);
                setEmployeeId(e.target.value);
              }}
            >
              <option value="">— เลือกพนักงาน —</option>
              {employees.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.employee_code} · {e.first_name_th} {e.last_name_th}
                </option>
              ))}
            </select>
          </label>
          <label>
            กรองตามรอบประเมิน
            <select
              data-testid="goal-cycle-filter"
              value={cycleFilter}
              onChange={(e) => setCycleFilter(e.target.value)}
            >
              <option value="">ทุกรอบ</option>
              {cycles.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </label>
        </div>

        {canWrite && (
          <form onSubmit={save} style={{ display: "grid", gap: 10 }}>
            <div className="grid-2" style={{ gridTemplateColumns: "2fr 1fr", gap: 12 }}>
              <label>
                เป้าหมาย
                <input
                  data-testid="goal-title"
                  value={form.title}
                  onChange={(e) => setForm({ ...form, title: e.target.value })}
                  required
                />
              </label>
              <label>
                ค่าเป้าหมาย
                <input
                  data-testid="goal-target"
                  value={form.target}
                  onChange={(e) => setForm({ ...form, target: e.target.value })}
                />
              </label>
            </div>
            <div className="grid-2" style={{ gridTemplateColumns: "1fr 2fr", gap: 12 }}>
              <label>
                รอบประเมิน (ไม่บังคับ)
                <select
                  data-testid="goal-cycle"
                  value={form.cycle_id}
                  onChange={(e) => setForm({ ...form, cycle_id: e.target.value })}
                >
                  <option value="">— ไม่ผูกกับรอบ —</option>
                  {cycles.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                รายละเอียด
                <input
                  data-testid="goal-description"
                  value={form.description}
                  onChange={(e) => setForm({ ...form, description: e.target.value })}
                />
              </label>
            </div>
            <button className="btn btn-primary" data-testid="add-goal" type="submit" disabled={busy || !employeeId}>
              เพิ่มเป้าหมาย
            </button>
          </form>
        )}
      </div>

      <div className="card">
        {!employeeId ? (
          <div className="empty">เลือกพนักงานเพื่อดูเป้าหมาย</div>
        ) : goals.length === 0 ? (
          <div className="empty">ยังไม่มีเป้าหมายของพนักงานคนนี้</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>เป้าหมาย</th>
                <th>รอบ</th>
                <th>ค่าเป้าหมาย</th>
                <th>ความคืบหน้า (%)</th>
                <th>สถานะ</th>
                {canWrite && <th />}
              </tr>
            </thead>
            <tbody>
              {goals.map((goal) => {
                const draft = drafts[goal.id] ?? {
                  progress: String(goal.progress),
                  status: goal.status,
                };
                return (
                  <tr key={goal.id} data-testid="goal-row">
                    <td>
                      {goal.title}
                      {goal.description && (
                        <div style={{ color: "var(--muted)", fontSize: 13 }}>{goal.description}</div>
                      )}
                    </td>
                    <td>{goal.cycle_name ?? "—"}</td>
                    <td>{goal.target ?? "—"}</td>
                    <td>
                      <input
                        data-testid="goal-progress"
                        type="number"
                        min={0}
                        max={100}
                        value={draft.progress}
                        disabled={busy || !canWrite}
                        onChange={(e) => setDraft(goal.id, { progress: e.target.value })}
                        style={{ width: 80 }}
                      />
                    </td>
                    <td>
                      <select
                        data-testid="goal-status"
                        value={draft.status}
                        disabled={busy || !canWrite}
                        onChange={(e) => setDraft(goal.id, { status: e.target.value })}
                      >
                        {Object.entries(STATUSES).map(([value, label]) => (
                          <option key={value} value={value}>
                            {label}
                          </option>
                        ))}
                      </select>
                    </td>
                    {canWrite && (
                      <td>
                        <button
                          className="btn btn-sm btn-primary"
                          data-testid="save-goal"
                          disabled={busy}
                          onClick={() => saveGoal(goal)}
                          style={{ marginRight: 6 }}
                        >
                          บันทึก
                        </button>
                        <button
                          className="btn btn-sm"
                          data-testid="delete-goal"
                          disabled={busy}
                          onClick={() => remove(goal)}
                        >
                          ลบ
                        </button>
                      </td>
                    )}
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}

"use client";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, Me, thDate } from "@/lib/api";

type Employee = {
  id: string;
  employee_code: string;
  first_name_th: string;
  last_name_th: string;
};

type Task = {
  id: string;
  employee_id: string;
  title: string;
  notes: string | null;
  due_date: string | null;
  status: "pending" | "done";
  completed_at: string | null;
  days_left: number | null;
};

const emptyForm = { title: "", notes: "", due_date: "" };

export default function OnboardingPage() {
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [employeeId, setEmployeeId] = useState("");
  const [tasks, setTasks] = useState<Task[]>([]);
  const [form, setForm] = useState(emptyForm);
  const [busy, setBusy] = useState(false);
  const [canWrite, setCanWrite] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const loadTasks = useCallback(async (id: string) => {
    if (!id) {
      setTasks([]);
      return;
    }
    const res = await gw<{ tasks: Task[] }>(`api/v1/employees/${id}/onboarding`);
    setTasks(res.tasks);
  }, []);

  useEffect(() => {
    gw<{ employees: Employee[] }>("api/v1/employees?status=all&limit=500")
      .then((res) => setEmployees(res.employees))
      .catch((e) => handleApiError(e, setError));
    gw<Me>("api/v1/auth/me")
      .then((me) => setCanWrite(me.permissions.includes("employee.update")))
      .catch((e) => handleApiError(e, setError));
  }, []);

  useEffect(() => {
    loadTasks(employeeId).catch((e) => handleApiError(e, setError));
  }, [employeeId, loadTasks]);

  const done = tasks.filter((t) => t.status === "done").length;

  async function startStandard() {
    if (!employeeId) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const res = await gw<{ tasks: Task[] }>(`api/v1/employees/${employeeId}/onboarding/standard`, {
        method: "POST",
      });
      setTasks(res.tasks);
      setNotice("สร้างรายการมาตรฐานแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function addTask(event: React.FormEvent) {
    event.preventDefault();
    if (!employeeId) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw(`api/v1/employees/${employeeId}/onboarding`, {
        method: "POST",
        body: JSON.stringify({
          title: form.title,
          notes: form.notes.trim() || null,
          due_date: form.due_date || null,
        }),
      });
      setForm(emptyForm);
      await loadTasks(employeeId);
      setNotice("เพิ่มงานแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function toggle(task: Task, done: boolean) {
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/onboarding/${task.id}`, {
        method: "PATCH",
        body: JSON.stringify({ done }),
      });
      await loadTasks(employeeId);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function remove(id: string) {
    if (!confirm("ลบงานนี้หรือไม่?")) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/onboarding/${id}`, { method: "DELETE" });
      await loadTasks(employeeId);
      setNotice("ลบงานแล้ว");
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
          <h1>Onboarding พนักงาน</h1>
          <p className="page-sub">รายการงานต้อนรับ เอกสาร และการตั้งค่าสำหรับพนักงานใหม่</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="success">{notice}</p>}

      <div className="card card-pad" style={{ marginBottom: 12, display: "grid", gap: 12 }}>
        <div className="grid-2" style={{ gridTemplateColumns: "2fr 1fr", gap: 12, alignItems: "end" }}>
          <label>
            พนักงาน
            <select
              data-testid="onboarding-employee"
              value={employeeId}
              onChange={(e) => setEmployeeId(e.target.value)}
            >
              <option value="">— เลือกพนักงาน —</option>
              {employees.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.employee_code} · {e.first_name_th} {e.last_name_th}
                </option>
              ))}
            </select>
          </label>
          {canWrite && (
            <button
              className="btn btn-primary"
              data-testid="standard-checklist"
              disabled={busy || !employeeId}
              onClick={startStandard}
            >
              สร้างรายการมาตรฐาน
            </button>
          )}
        </div>

        {employeeId && (
          <p data-testid="onboarding-progress">
            เสร็จ {done}/{tasks.length} รายการ
          </p>
        )}

        {canWrite && (
          <form onSubmit={addTask} style={{ display: "grid", gap: 10 }}>
            <div className="grid-2" style={{ gridTemplateColumns: "2fr 1fr", gap: 12 }}>
              <label>
                งานที่ต้องทำ
                <input
                  data-testid="task-title"
                  value={form.title}
                  onChange={(e) => setForm({ ...form, title: e.target.value })}
                  required
                />
              </label>
              <label>
                กำหนดเสร็จ
                <input
                  data-testid="task-due"
                  type="date"
                  value={form.due_date}
                  onChange={(e) => setForm({ ...form, due_date: e.target.value })}
                />
              </label>
            </div>
            <label>
              หมายเหตุ
              <input
                data-testid="task-notes"
                value={form.notes}
                onChange={(e) => setForm({ ...form, notes: e.target.value })}
              />
            </label>
            <button
              className="btn"
              data-testid="add-task"
              type="submit"
              disabled={busy || !employeeId}
            >
              เพิ่มงาน
            </button>
          </form>
        )}
      </div>

      <div className="card">
        {tasks.length === 0 ? (
          <div className="empty">ยังไม่มีรายการ onboarding สำหรับพนักงานคนนี้</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>เสร็จ</th>
                <th>งาน</th>
                <th>กำหนดเสร็จ</th>
                <th>สถานะ</th>
                {canWrite && <th />}
              </tr>
            </thead>
            <tbody>
              {tasks.map((t) => (
                <tr key={t.id} data-testid="onboarding-task">
                  <td>
                    <input
                      data-testid="task-done"
                      type="checkbox"
                      aria-label={`เสร็จ: ${t.title}`}
                      checked={t.status === "done"}
                      disabled={busy || !canWrite}
                      onChange={(e) => toggle(t, e.target.checked)}
                    />
                  </td>
                  <td>
                    <div style={{ fontWeight: 500 }}>{t.title}</div>
                    {t.notes && <div style={{ color: "var(--muted)", fontSize: 13 }}>{t.notes}</div>}
                  </td>
                  <td>{t.due_date ? thDate(t.due_date) : "—"}</td>
                  <td>
                    {t.status === "done" ? (
                      <span className="badge success">เสร็จแล้ว</span>
                    ) : t.days_left !== null && t.days_left < 0 ? (
                      <span className="badge danger">เลยกำหนด {Math.abs(t.days_left)} วัน</span>
                    ) : t.days_left === 0 ? (
                      <span className="badge warning">ครบกำหนดวันนี้</span>
                    ) : (
                      <span className="badge">รอทำ</span>
                    )}
                  </td>
                  {canWrite && (
                    <td>
                      <button
                        className="btn btn-sm"
                        data-testid="delete-task"
                        disabled={busy}
                        onClick={() => remove(t.id)}
                      >
                        ลบ
                      </button>
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}

"use client";

import type { TranslationKey } from "@/lib/i18n-catalog";

import { useI18N, tNow } from "@/lib/i18n";

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

type CheckIn = {
  id: string;
  goal_id: string;
  employee_id: string;
  note: string;
  created_at: string;
};

type DirectReport = {
  id: string;
  employee_code: string;
  employee_name: string;
};

const emptyForm = { title: "", cycle_id: "", target: "", description: "" };

const STATUSES: Record<string, TranslationKey> = {
  active: "goals.in-progress",
  achieved: "goals.succeed",
  cancelled: "goals.cancel",
};

export default function GoalsPage() {
  const { t: tr } = useI18N();
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [cycles, setCycles] = useState<Cycle[]>([]);
  const [employeeId, setEmployeeId] = useState("");
  const [cycleFilter, setCycleFilter] = useState("");
  const [goals, setGoals] = useState<Goal[]>([]);
  const [form, setForm] = useState(emptyForm);
  const [drafts, setDrafts] = useState<Record<string, { progress: string; status: string }>>({});
  const [canWrite, setCanWrite] = useState(false);
  const [canDirect, setCanDirect] = useState(false);
  const [directReports, setDirectReports] = useState<DirectReport[]>([]);
  const [checkIns, setCheckIns] = useState<CheckIn[]>([]);
  const [checkInGoal, setCheckInGoal] = useState("");
  const [checkInNote, setCheckInNote] = useState("");
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
      .catch(() => undefined);
    gw<Me>("api/v1/auth/me")
      .then((me) => {
        setCanWrite(me.permissions.includes("employee.update"));
        setCanDirect(me.permissions.includes("performance.manage_direct"));
      })
      .catch((e) => handleApiError(e, setError));
    gw<{ cycles: Cycle[] }>("api/v1/performance/cycles")
      .then((res) => setCycles(res.cycles))
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    if (!canDirect) return;
    gw<{ direct_reports: DirectReport[] }>("api/v1/me/direct-reports")
      .then((res) => setDirectReports(res.direct_reports))
      .catch((e) => handleApiError(e, setError));
  }, [canDirect]);

  useEffect(() => {
    if (!canDirect || !checkInGoal) {
      setCheckIns([]);
      return;
    }
    gw<{ check_ins: CheckIn[] }>(`api/v1/goals/${checkInGoal}/check-ins`)
      .then((res) => setCheckIns(res.check_ins))
      .catch((e) => handleApiError(e, setError));
  }, [canDirect, checkInGoal]);

  async function addCheckIn(event: React.FormEvent) {
    event.preventDefault();
    if (!checkInGoal) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await gw(`api/v1/goals/${checkInGoal}/check-ins`, {
        method: "POST",
        body: JSON.stringify({ note: checkInNote }),
      });
      setCheckInNote("");
      const res = await gw<{ check_ins: CheckIn[] }>(`api/v1/goals/${checkInGoal}/check-ins`);
      setCheckIns(res.check_ins);
      setNotice(tr("goals.check-in-has-been-recorded"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    loadGoals(employeeId, cycleFilter).catch((e) => handleApiError(e, setError));
  }, [employeeId, cycleFilter, loadGoals]);

  useEffect(() => {
    if (!canDirect || directReports.length === 0) return;
    gw<{ goals: Goal[] }>("api/v1/me/direct-goals")
      .then((res) => setGoals(res.goals))
      .catch((e) => handleApiError(e, setError));
  }, [canDirect, directReports]);

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
      setNotice(tr("goals.target-added"));
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
      setNotice(tr("goals.progress-saved"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function remove(goal: Goal) {
    if (!confirm(tr("goals.deleteGoalConfirm", { title: goal.title }))) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/goals/${goal.id}`, { method: "DELETE" });
      await loadGoals(employeeId, cycleFilter);
      setNotice(tr("goals.target-deleted"));
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
          <h1>{tr("goals.goals-okrs")}</h1>
          <p className="page-sub">{tr("goals.per-employee-goals-progress-and-status")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="success" role="status" aria-live="polite">{notice}</p>}

      <div className="card card-pad" style={{ marginBottom: 12, display: "grid", gap: 12 }}>
        <div className="grid-2" style={{ gridTemplateColumns: "1fr 1fr", gap: 12 }}>
          <label>
            {tr("goals.employee")}
            <select
              data-testid="goal-employee"
              value={employeeId}
              onChange={(e) => {
                setGoals([]);
                setEmployeeId(e.target.value);
              }}
            >
              <option value="">{tr("goals.select-employee")}</option>
              {employees.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.employee_code} · {e.first_name_th} {e.last_name_th}
                </option>
              ))}
            </select>
          </label>
          <label>
            {tr("goals.filter-by-evaluation-period")}
            <select
              data-testid="goal-cycle-filter"
              value={cycleFilter}
              onChange={(e) => setCycleFilter(e.target.value)}
            >
              <option value="">{tr("goals.all-cycles")}</option>
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
                {tr("goals.goals")}
                <input
                  data-testid="goal-title"
                  value={form.title}
                  onChange={(e) => setForm({ ...form, title: e.target.value })}
                  required
                />
              </label>
              <label>
                {tr("goals.target-value")}
                <input
                  data-testid="goal-target"
                  value={form.target}
                  onChange={(e) => setForm({ ...form, target: e.target.value })}
                />
              </label>
            </div>
            <div className="grid-2" style={{ gridTemplateColumns: "1fr 2fr", gap: 12 }}>
              <label>
                {tr("goals.assessment-round-optional")}
                <select
                  data-testid="goal-cycle"
                  value={form.cycle_id}
                  onChange={(e) => setForm({ ...form, cycle_id: e.target.value })}
                >
                  <option value="">{tr("goals.not-tied-to-cycle")}</option>
                  {cycles.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                {tr("goals.details")}
                <input
                  data-testid="goal-description"
                  value={form.description}
                  onChange={(e) => setForm({ ...form, description: e.target.value })}
                />
              </label>
            </div>
            <button className="btn btn-primary" data-testid="add-goal" type="submit" disabled={busy || !employeeId}>
              {tr("goals.add-goal")}
            </button>
          </form>
        )}
      </div>

      <div className="card">
        {!employeeId ? (
          <div className="empty">{tr("goals.select-an-employee-to-view-their")}</div>
        ) : goals.length === 0 ? (
          <div className="empty">{tr("goals.there-are-no-goals-for-this")}</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{tr("goals.goals")}</th>
                <th scope="col">{tr("goals.cycle")}</th>
                <th scope="col">{tr("goals.target-value")}</th>
                <th scope="col">{tr("goals.progress")}</th>
                <th scope="col">{tr("goals.status")}</th>
                {canWrite && <th scope="col" />}
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
                            {tr(label)}
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
                          {tr("goals.save")}
                        </button>
                        <button
                          className="btn btn-sm"
                          data-testid="delete-goal"
                          disabled={busy}
                          onClick={() => remove(goal)}
                        >
                          {tr("goals.delete")}
                        </button>
                      </td>
                    )}
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
        {canDirect && directReports.length > 0 && (
          <div className="card card-pad" style={{ marginTop: 12, display: "grid", gap: 12 }}>
            <h2 style={{ fontSize: 16 }}>{tr("goals.team-goal-check-in")}</h2>
            <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
              <select
                data-testid="checkin-goal"
                value={checkInGoal}
                onChange={(e) => setCheckInGoal(e.target.value)}
              >
                <option value="">{tr("goals.text")}</option>
                {goals.map((g) => {
                  const report = directReports.find((r) => r.id === g.employee_id);
                  return (
                    <option key={g.id} value={g.id}>
                      {report ? `${report.employee_code} · ` : ""}{g.title}
                    </option>
                  );
                })}
              </select>
            </div>
            {checkInGoal && (
              <div style={{ display: "grid", gap: 8 }}>
                <form onSubmit={addCheckIn} style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                  <input
                    data-testid="checkin-note"
                    value={checkInNote}
                    onChange={(e) => setCheckInNote(e.target.value)}
                    placeholder={tr("goals.progress-notes-optional")}
                    required
                  />
                  <button
                    className="btn btn-primary"
                    data-testid="add-checkin"
                    type="submit"
                    disabled={busy}
                  >
                    {tr("goals.record-check-in")}
                  </button>
                </form>
                {checkIns.length === 0 ? (
                  <div className="empty">{tr("goals.no-goal-check-in-yet")}</div>
                ) : (
                  <ul style={{ margin: 0, paddingLeft: 18 }}>
                    {checkIns.map((c) => (
                      <li key={c.id} data-testid="checkin-row">
                        {c.note}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </div>
        )}
      </div>
    </>
  );
}

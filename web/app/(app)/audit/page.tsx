"use client";

import { useEffect, useState } from "react";
import { gw, handleApiError } from "@/lib/api";

type Entry = {
  id: number;
  actor_type: string;
  actor_id: string;
  action: string;
  resource_type: string;
  resource_id: string;
  after_state: unknown;
  created_at: string;
};

export default function AuditPage() {
  const [rows, setRows] = useState<Entry[] | null>(null);
  const [action, setAction] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    const query = action ? `?action=${encodeURIComponent(action)}` : "";
    gw<{ entries: Entry[] }>(`api/v1/audit${query}`)
      .then((r) => setRows(r.entries))
      .catch((e) => handleApiError(e, setError));
  }, [action]);

  return (
    <>
      <div className="page-head">
        <div>
          <h1>ประวัติการใช้งาน</h1>
          <p className="page-sub">ทุกการเปลี่ยนแปลงสำคัญในองค์กร</p>
        </div>
        <input
          className="compact-input"
          style={{ width: 240 }}
          placeholder="กรองตาม action เช่น employee.create"
          value={action}
          onChange={(e) => setAction(e.target.value)}
        />
      </div>

      {error && <p className="error">{error}</p>}

      <div className="card">
        {rows === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : rows.length === 0 ? (
          <div className="empty">ยังไม่มีรายการ</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>เวลา</th>
                <th>ผู้กระทำ</th>
                <th>การกระทำ</th>
                <th>resource</th>
                <th>รายละเอียด</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((e) => (
                <tr key={e.id}>
                  <td>{new Date(e.created_at).toLocaleString("th-TH")}</td>
                  <td>
                    <div>{e.actor_type}</div>
                    <div className="hint">{e.actor_id.slice(0, 12)}</div>
                  </td>
                  <td>{e.action}</td>
                  <td>
                    {e.resource_type}
                    <div className="hint">{e.resource_id.slice(0, 12)}</div>
                  </td>
                  <td className="hint" style={{ maxWidth: 320, overflow: "hidden", textOverflow: "ellipsis" }}>
                    {e.after_state ? JSON.stringify(e.after_state) : "—"}
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

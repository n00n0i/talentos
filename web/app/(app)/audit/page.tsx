"use client";

import { useI18N } from "@/lib/i18n";

import { useEffect, useState } from "react";
import { gw, handleApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/i18n-format";

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
  const { t: tr } = useI18N();
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
          <h1>{tr("audit.audit-log")}</h1>
          <p className="page-sub">{tr("audit.every-important-change-in-the-organization")}</p>
        </div>
        <input
          className="compact-input"
          style={{ width: 240 }}
          placeholder={tr("audit.filter-by-action-such-as-employee")}
          value={action}
          onChange={(e) => setAction(e.target.value)}
        />
      </div>

      {error && <p className="error" role="alert">{error}</p>}

      <div className="card">
        {rows === null ? (
          <div className="spinner">{tr("audit.loading")}</div>
        ) : rows.length === 0 ? (
          <div className="empty">{tr("audit.there-are-no-items-yet")}</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{tr("audit.time")}</th>
                <th scope="col">{tr("audit.actor")}</th>
                <th scope="col">{tr("audit.action")}</th>
                <th scope="col">{tr("audit.resource")}</th>
                <th scope="col">{tr("audit.details")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((e) => (
                <tr key={e.id}>
                  <td>{formatDateTime(e.created_at)}</td>
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

"use client";

import type { TranslationKey } from "@/lib/i18n-catalog";

import { useI18N, tNow } from "@/lib/i18n";

import { useEffect, useState } from "react";
import { gw, handleApiError } from "@/lib/api";

type TeamMember = {
  id: string;
  employee_code: string;
  employee_name: string;
  position_title: string;
  department: string | null;
  status: string;
};

const STATUS: Record<string, TranslationKey> = {
  active: "team.active",
  probation: "team.probation",
  leave: "team.leave",
  terminated: "team.terminated",
};

export default function TeamPage() {
  const { t: tr } = useI18N();
  const [members, setMembers] = useState<TeamMember[] | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    gw<{ members: TeamMember[] }>("api/v1/me/team")
      .then((res) => setMembers(res.members))
      .catch((e) => handleApiError(e, setError));
  }, []);

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{tr("team.my-team")}</h1>
          <p className="page-sub">{tr("team.direct-reports-read-only")}</p>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}

      <div className="card">
        {members === null ? (
          <div className="spinner">{tr("team.loading")}</div>
        ) : members.length === 0 ? (
          <div className="empty" data-testid="team-empty">
            {tr("team.no-direct-reports-yet")}
          </div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{tr("team.code")}</th>
                <th scope="col">{tr("team.name")}</th>
                <th scope="col">{tr("team.position")}</th>
                <th scope="col">{tr("team.department")}</th>
                <th scope="col">{tr("team.status")}</th>
              </tr>
            </thead>
            <tbody>
              {members.map((member) => (
                <tr key={member.id} data-testid="team-row">
                  <td>{member.employee_code}</td>
                  <td>{member.employee_name}</td>
                  <td>{member.position_title}</td>
                  <td>{member.department ?? "—"}</td>
                  <td>
                    <span className="badge success">{member.status in STATUS ? tr(STATUS[member.status]) : member.status}</span>
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

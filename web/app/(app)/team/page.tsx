"use client";

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

const STATUS: Record<string, string> = {
  active: "ทำงานอยู่",
  probation: "ทดลองงาน",
  leave: "ลา",
  terminated: "พ้นสภาพ",
};

export default function TeamPage() {
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
          <h1>ทีมของฉัน</h1>
          <p className="page-sub">รายชื่อลูกทีมที่รายงานตรงกับคุณ (อ่านอย่างเดียว)</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}

      <div className="card">
        {members === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : members.length === 0 ? (
          <div className="empty" data-testid="team-empty">
            ยังไม่มีลูกทีมที่รายงานตรงกับคุณ
          </div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>รหัส</th>
                <th>ชื่อ</th>
                <th>ตำแหน่ง</th>
                <th>แผนก</th>
                <th>สถานะ</th>
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
                    <span className="badge success">{STATUS[member.status] ?? member.status}</span>
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

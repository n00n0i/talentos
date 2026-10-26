"use client";

import { useEffect, useState } from "react";
import { gql, handleApiError, thb } from "@/lib/api";

type Employee = {
  employeeCode: string;
  firstNameTh: string;
  lastNameTh: string;
  positionTitle: string;
  department: string | null;
  baseSalary: string;
  status: string;
};

export default function EmployeesPage() {
  const [rows, setRows] = useState<Employee[] | null>(null);
  const [query, setQuery] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    gql<{ employees: Employee[] }>(
      "{ employees(limit: 200) { employeeCode firstNameTh lastNameTh positionTitle department baseSalary status } }",
    )
      .then((r) => setRows(r.employees))
      .catch((e) => handleApiError(e, setError));
  }, []);

  const filtered = (rows ?? []).filter((e) => {
    const q = query.trim().toLowerCase();
    if (!q) return true;
    return [e.employeeCode, e.firstNameTh, e.lastNameTh, e.positionTitle, e.department ?? ""]
      .join(" ")
      .toLowerCase()
      .includes(q);
  });

  return (
    <>
      <div className="page-head">
        <div>
          <h1>พนักงาน</h1>
          <p className="page-sub">ข้อมูลพนักงานในองค์กรของคุณ</p>
        </div>
        <input
          style={{ height: 34, width: 260, border: "1px solid var(--border)", borderRadius: 6, padding: "0 10px" }}
          placeholder="ค้นหารหัส ชื่อ หรือตำแหน่ง"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>

      {error && <p className="error">{error}</p>}

      <div className="card">
        {rows === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : filtered.length === 0 ? (
          <div className="empty">ไม่พบพนักงาน</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>รหัส</th>
                <th>ชื่อ</th>
                <th>ตำแหน่ง</th>
                <th>แผนก</th>
                <th className="num">เงินเดือน</th>
                <th>สถานะ</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((e) => (
                <tr key={e.employeeCode}>
                  <td>{e.employeeCode}</td>
                  <td>
                    {e.firstNameTh} {e.lastNameTh}
                  </td>
                  <td>{e.positionTitle}</td>
                  <td>{e.department ?? "—"}</td>
                  <td className="num">฿{thb(e.baseSalary)}</td>
                  <td>
                    <span className={`badge ${e.status === "active" ? "success" : ""}`}>
                      {e.status === "active" ? "ทำงานอยู่" : e.status}
                    </span>
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

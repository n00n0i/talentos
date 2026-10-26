"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import {
  gql,
  handleApiError,
  PayrollRun,
  RUN_STATUS,
  thb,
  thDate,
  thbShort,
} from "@/lib/api";

type Employee = { employeeCode: string };

export default function DashboardPage() {
  const [runs, setRuns] = useState<PayrollRun[] | null>(null);
  const [headcount, setHeadcount] = useState<number | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    Promise.all([
      gql<{ payrollRuns: PayrollRun[] }>(
        "{ payrollRuns(limit: 6) { id periodStart periodEnd paymentDate status totalEmployees totalGross totalNet totalTax totalSso totalPf } }",
      ),
      gql<{ employees: Employee[] }>("{ employees(limit: 200) { employeeCode } }"),
    ])
      .then(([r, e]) => {
        setRuns(r.payrollRuns);
        setHeadcount(e.employees.length);
      })
      .catch((e) => handleApiError(e, setError));
  }, []);

  const latest = runs?.[0];

  return (
    <>
      <div className="page-head">
        <div>
          <h1>ภาพรวม</h1>
          <p className="page-sub">สถานะพนักงานและรอบเงินเดือนล่าสุด</p>
        </div>
        <Link className="btn btn-primary" href="/payroll">
          ไปที่รอบเงินเดือน
        </Link>
      </div>

      {error && <p className="error">{error}</p>}

      <div className="grid-4">
        <div className="card card-pad">
          <div className="stat-label">พนักงานที่ใช้งาน</div>
          <div className="stat-value">{headcount ?? "—"}</div>
          <div className="stat-hint">คน</div>
        </div>
        <div className="card card-pad">
          <div className="stat-label">ยอดจ่ายสุทธิรอบล่าสุด</div>
          <div className="stat-value">฿{latest ? thbShort(latest.totalNet) : "—"}</div>
          <div className="stat-hint">{latest ? `${latest.totalEmployees} คน` : "ยังไม่มีรอบ"}</div>
        </div>
        <div className="card card-pad">
          <div className="stat-label">ภาษีหัก ณ ที่จ่าย</div>
          <div className="stat-value">฿{latest ? thbShort(latest.totalTax) : "—"}</div>
          <div className="stat-hint">PND.1 รอบล่าสุด</div>
        </div>
        <div className="card card-pad">
          <div className="stat-label">ประกันสังคม (ลูกจ้าง)</div>
          <div className="stat-value">฿{latest ? thbShort(latest.totalSso) : "—"}</div>
          <div className="stat-hint">มาตรา 33</div>
        </div>
      </div>

      <div className="card">
        <div className="card-head">
          <h2>รอบเงินเดือนล่าสุด</h2>
          <Link href="/payroll" className="hint">
            ดูทั้งหมด
          </Link>
        </div>
        {runs === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : runs.length === 0 ? (
          <div className="empty">ยังไม่มีรอบเงินเดือน — เริ่มสร้างรอบแรกได้ที่หน้า “รอบเงินเดือน”</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>งวด</th>
                <th>วันจ่าย</th>
                <th>สถานะ</th>
                <th className="num">พนักงาน</th>
                <th className="num">ยอดรวม</th>
                <th className="num">สุทธิ</th>
              </tr>
            </thead>
            <tbody>
              {runs.map((run) => {
                const status = RUN_STATUS[run.status] ?? { label: run.status, tone: "" };
                return (
                  <tr key={run.id} className="clickable">
                    <td>
                      <Link href={`/payroll?run=${run.id}`}>
                        {thDate(run.periodStart)} – {thDate(run.periodEnd)}
                      </Link>
                    </td>
                    <td>{thDate(run.paymentDate)}</td>
                    <td>
                      <span className={`badge ${status.tone}`}>
                        <span className="dot" />
                        {status.label}
                      </span>
                    </td>
                    <td className="num">{run.totalEmployees}</td>
                    <td className="num">฿{thb(run.totalGross)}</td>
                    <td className="num">฿{thb(run.totalNet)}</td>
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

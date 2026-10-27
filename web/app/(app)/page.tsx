"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import Icon from "@/components/Icon";
import {
  gw,
  gql,
  handleApiError,
  Me,
  PayrollRun,
  Payslip,
  RUN_STATUS,
  thb,
  thbShort,
  thDate,
} from "@/lib/api";

type Employee = { employeeCode: string };

function initials(name: string) {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "T";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return `${parts[0][0]}${parts[1][0]}`.toUpperCase();
}

export default function DashboardPage() {
  const [me, setMe] = useState<Me | null>(null);
  const [runs, setRuns] = useState<PayrollRun[] | null>(null);
  const [headcount, setHeadcount] = useState<number | null>(null);
  const [payslips, setPayslips] = useState<Payslip[]>([]);
  const [greeting, setGreeting] = useState("สวัสดี");
  const [error, setError] = useState("");
  const router = useRouter();

  useEffect(() => {
    const hour = new Date().getHours();
    setGreeting(hour < 12 ? "สวัสดีตอนเช้า" : hour < 17 ? "สวัสดีตอนบ่าย" : "สวัสดีตอนเย็น");
  }, []);

  useEffect(() => {
    gw<Me>("api/v1/auth/me")
      .then((profile) => {
        setMe(profile);
        if (!profile.permissions.includes("employee.read_all")) {
          router.replace("/me/payslips");
        }
      })
      .catch((e) => handleApiError(e, setError));
  }, [router]);

  useEffect(() => {
    if (!me?.permissions.includes("employee.read_all")) return;
    (async () => {
      const [runData, employees] = await Promise.all([
        gql<{ payrollRuns: PayrollRun[] }>(
          "{ payrollRuns(limit: 6) { id periodStart periodEnd paymentDate status totalEmployees totalGross totalNet totalTax totalSso totalPf } }",
        ),
        gql<{ employees: Employee[] }>("{ employees(limit: 200) { employeeCode } }"),
      ]);
      setRuns(runData.payrollRuns);
      setHeadcount(employees.employees.length);

      const latest = runData.payrollRuns[0];
      if (latest) {
        try {
          const slips = await gw<{ payslips: Payslip[] }>(
            `api/v1/payroll/runs/${latest.id}/payslips`,
          );
          setPayslips(slips.payslips);
        } catch {
          // a run without payslips is normal right after creation
        }
      }
    })().catch((e) => handleApiError(e, setError));
  }, [me]);

  const latest = runs?.[0];
  const firstName = me?.full_name_th?.split(/\s+/)[0] || "";
  const status = latest ? RUN_STATUS[latest.status] ?? { label: latest.status, tone: "" } : null;

  return (
    <>
      <div className="greeting">
        <div>
          <h1>
            {greeting}
            {firstName ? ` ${firstName}` : ""}
          </h1>
          <p className="page-sub">ภาพรวมทีมและรอบเงินเดือนล่าสุดของคุณ</p>
        </div>
        <Link className="btn btn-primary" href="/payroll">
          <Icon name="wallet" size={15} />
          ไปที่รอบเงินเดือน
        </Link>
      </div>

      {error && <p className="error">{error}</p>}

      <div className="dash-grid">
        <div className="dash-main">
          <div className="stats">
            <div className="card card-pad tone-sage">
              <div className="stat-top">
                <span className="stat-label">พนักงานที่ใช้งาน</span>
                <span className="stat-icon">
                  <Icon name="users" size={16} />
                </span>
              </div>
              <div className="stat-value">{headcount ?? "—"}</div>
              <div className="stat-hint">คนในองค์กร</div>
            </div>

            <div className="card card-pad tone-teal">
              <div className="stat-top">
                <span className="stat-label">ยอดจ่ายสุทธิรอบล่าสุด</span>
                <span className="stat-icon">
                  <Icon name="wallet" size={16} />
                </span>
              </div>
              <div className="stat-value">฿{latest ? thbShort(latest.totalNet) : "—"}</div>
              <div className="stat-hint">{latest ? `${latest.totalEmployees} คน` : "ยังไม่มีรอบ"}</div>
            </div>

            <div className="card card-pad tone-mint">
              <div className="stat-top">
                <span className="stat-label">ภาษีหัก ณ ที่จ่าย</span>
                <span className="stat-icon">
                  <Icon name="file" size={16} />
                </span>
              </div>
              <div className="stat-value">฿{latest ? thbShort(latest.totalTax) : "—"}</div>
              <div className="stat-hint">PND.1 รอบล่าสุด</div>
            </div>

            <div className="card card-pad tone-plain">
              <div className="stat-top">
                <span className="stat-label">ประกันสังคม (ลูกจ้าง)</span>
                <span className="stat-icon">
                  <Icon name="clock" size={16} />
                </span>
              </div>
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
                    <th>สถานะ</th>
                    <th className="num">คน</th>
                    <th className="num">ยอดรวม</th>
                    <th className="num">สุทธิ</th>
                  </tr>
                </thead>
                <tbody>
                  {runs.slice(0, 5).map((run) => {
                    const s = RUN_STATUS[run.status] ?? { label: run.status, tone: "" };
                    return (
                      <tr key={run.id} className="clickable">
                        <td>
                          <Link href={`/payroll?run=${run.id}`}>
                            {thDate(run.periodStart)} – {thDate(run.periodEnd)}
                          </Link>
                        </td>
                        <td>
                          <span className={`badge ${s.tone}`}>
                            <span className="dot" />
                            {s.label}
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
        </div>

        <aside className="rail">
          <div className="card rail-card">
            <div className="card-head">
              <h2>จ่ายเงินเดือนรอบล่าสุด</h2>
              <Link href="/payroll" className="hint">
                ทั้งหมด
              </Link>
            </div>
            {payslips.length === 0 ? (
              <div className="empty" style={{ padding: "28px 18px" }}>
                ยังไม่มีสลิปในรอบนี้
              </div>
            ) : (
              <div className="pay-list">
                {payslips.slice(0, 6).map((p) => (
                  <div className="pay-item" key={p.payslip_id}>
                    <div className="avatar avatar-sm">{initials(p.employee_name)}</div>
                    <div className="pay-meta">
                      <div className="pay-name">{p.employee_name}</div>
                      <div className="pay-sub">
                        {p.employee_code}
                        {status ? ` · ${status.label}` : ""}
                      </div>
                    </div>
                    <div className="pay-amount">฿{thbShort(p.net_pay)}</div>
                  </div>
                ))}
                {payslips.length > 6 && (
                  <div className="pay-more">และอีก {payslips.length - 6} คน</div>
                )}
              </div>
            )}
          </div>

          {latest && (
            <div className="pay-summary">
              <div className="pay-summary-top">
                <span>สรุปการจ่าย</span>
                {status && <span className="pay-chip">{status.label}</span>}
              </div>
              <div className="pay-rows">
                <div className="pay-row">
                  <span>เงินเดือนรวม</span>
                  <b>฿{thbShort(latest.totalGross)}</b>
                </div>
                <div className="pay-row">
                  <span>ภาษี</span>
                  <b>฿{thbShort(latest.totalTax)}</b>
                </div>
                <div className="pay-row">
                  <span>ประกันสังคม</span>
                  <b>฿{thbShort(latest.totalSso)}</b>
                </div>
                <div className="pay-row">
                  <span>กองทุนสำรองฯ</span>
                  <b>฿{thbShort(latest.totalPf)}</b>
                </div>
              </div>
              <div className="pay-total">
                <span>ยอดจ่ายสุทธิ</span>
                <b>฿{thb(latest.totalNet)}</b>
              </div>
            </div>
          )}
        </aside>
      </div>
    </>
  );
}

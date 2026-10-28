//! Read-only tenant-scoped report queries and the GL journal CSV renderer.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::{PgConnection, Row};
use thiserror::Error;
use uuid::Uuid;

const UNKNOWN_DEPARTMENT: &str = "ไม่ระบุ";

#[derive(Debug, Clone)]
pub struct Headcount {
    pub active: i64,
    pub probation: i64,
    pub leave: i64,
    pub terminated_last_12m: i64,
    pub by_department: Vec<DepartmentCount>,
}

#[derive(Debug, Clone)]
pub struct DepartmentCount {
    pub department: String,
    pub count: i64,
}

#[derive(Debug, Clone)]
pub struct PayrollCostRow {
    pub department: String,
    pub employees: i64,
    pub gross: Decimal,
    pub net: Decimal,
    pub tax: Decimal,
    pub sso: Decimal,
    pub pf: Decimal,
}

#[derive(Debug, Clone)]
pub struct PayrollCostReport {
    pub rows: Vec<PayrollCostRow>,
}

#[derive(Debug, Clone)]
pub struct OtTrendRow {
    pub month: String,
    pub ot_minutes: i64,
    pub approved_days: i64,
}

#[derive(Debug, Clone)]
pub struct OtTrendReport {
    pub rows: Vec<OtTrendRow>,
}

#[derive(Debug, Clone)]
pub struct GlLine {
    pub account_code: String,
    pub account_name: String,
    pub department: String,
    pub debit: Decimal,
    pub credit: Decimal,
    pub description: String,
}

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("not found")]
    NotFound,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

/// Headcount snapshot. `active` counts only `active` status; `probation` and
/// `leave` are reported separately. Terminated employees are counted only if
/// their termination_date falls within the last 12 months.
pub async fn headcount(conn: &mut PgConnection) -> Result<Headcount, ReportError> {
    let rows = sqlx::query(
        "SELECT status, COUNT(*)::bigint AS cnt \
         FROM employees \
         WHERE status IN ('active', 'probation', 'leave', 'terminated') \
           AND tenant_id = current_tenant_id() \
         GROUP BY status",
    )
    .fetch_all(&mut *conn)
    .await?;

    let mut active = 0i64;
    let mut probation = 0i64;
    let mut leave = 0i64;
    for row in &rows {
        let status: String = row.get("status");
        let cnt: i64 = row.get("cnt");
        match status.as_str() {
            "active" => active = cnt,
            "probation" => probation = cnt,
            "leave" => leave = cnt,
            _ => {}
        }
    }

    let terminated_last_12m: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint \
         FROM employees \
         WHERE status = 'terminated' \
           AND termination_date >= (CURRENT_DATE - INTERVAL '12 months') \
           AND tenant_id = current_tenant_id()",
    )
    .fetch_one(&mut *conn)
    .await?;

    let by_department_rows = sqlx::query(
        "SELECT COALESCE(department, $1) AS department, COUNT(*)::bigint AS cnt \
         FROM employees \
         WHERE status != 'terminated' \
           AND tenant_id = current_tenant_id() \
         GROUP BY department \
         ORDER BY department",
    )
    .bind(UNKNOWN_DEPARTMENT)
    .fetch_all(&mut *conn)
    .await?;

    let mut by_department = Vec::with_capacity(by_department_rows.len());
    for row in &by_department_rows {
        by_department.push(DepartmentCount {
            department: row.get("department"),
            count: row.get("cnt"),
        });
    }

    Ok(Headcount {
        active,
        probation,
        leave,
        terminated_last_12m,
        by_department,
    })
}

/// Payroll cost grouped by department for approved runs whose period_end falls
/// inside the inclusive [from, to] range. `sso` and `pf` are total contributions
/// (employee + employer) because this is a cost-centre view.
pub async fn payroll_cost(
    conn: &mut PgConnection,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<PayrollCostReport, ReportError> {
    let rows = sqlx::query(
        "SELECT COALESCE(e.department, $1) AS department, \
                COUNT(DISTINCT p.employee_id)::bigint AS employees, \
                COALESCE(SUM(p.gross_earnings), 0) AS gross, \
                COALESCE(SUM(p.net_pay), 0) AS net, \
                COALESCE(SUM(p.tax_withheld), 0) AS tax, \
                COALESCE(SUM(p.sso_employee + p.sso_employer), 0) AS sso, \
                COALESCE(SUM(p.pf_employee + p.pf_employer), 0) AS pf \
         FROM payslips p \
         JOIN payroll_runs r ON r.id = p.payroll_run_id \
         JOIN employees e ON e.id = p.employee_id \
         WHERE r.status = 'approved' \
           AND r.tenant_id = current_tenant_id() \
           AND r.period_end BETWEEN $2 AND $3 \
         GROUP BY e.department \
         ORDER BY department",
    )
    .bind(UNKNOWN_DEPARTMENT)
    .bind(from)
    .bind(to)
    .fetch_all(&mut *conn)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for row in &rows {
        result.push(PayrollCostRow {
            department: row.get("department"),
            employees: row.get("employees"),
            gross: row.get("gross"),
            net: row.get("net"),
            tax: row.get("tax"),
            sso: row.get("sso"),
            pf: row.get("pf"),
        });
    }

    Ok(PayrollCostReport { rows: result })
}

/// Approved attendance days grouped by `YYYY-MM`. `ot_minutes` is the sum of
/// overtime minutes; `approved_days` is the number of approved day rows.
pub async fn ot_trend(
    conn: &mut PgConnection,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<OtTrendReport, ReportError> {
    let rows = sqlx::query(
        "SELECT TO_CHAR(work_date, 'YYYY-MM') AS month, \
                COALESCE(SUM(ot_minutes), 0)::bigint AS ot_minutes, \
                COUNT(*)::bigint AS approved_days \
         FROM attendance_days \
         WHERE status = 'approved' \
           AND tenant_id = current_tenant_id() \
           AND work_date BETWEEN $1 AND $2 \
         GROUP BY month \
         ORDER BY month",
    )
    .bind(from)
    .bind(to)
    .fetch_all(&mut *conn)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for row in &rows {
        result.push(OtTrendRow {
            month: row.get("month"),
            ot_minutes: row.get("ot_minutes"),
            approved_days: row.get("approved_days"),
        });
    }

    Ok(OtTrendReport { rows: result })
}

/// PF remittance CSV for an approved payroll run.
/// Returns `(filename, body)`. Only `approved` runs are exported; anything else
/// or a run not visible to the tenant yields `NotFound`. Rows are included only
/// for payslips with a PF contribution (employee or employer side).
pub async fn export_pf_remittance_csv(
    conn: &mut PgConnection,
    run_id: Uuid,
) -> Result<(String, String), ReportError> {
    let run = sqlx::query(
        "SELECT status, period_start, period_end \
         FROM payroll_runs \
         WHERE id = $1 \
           AND tenant_id = current_tenant_id()",
    )
    .bind(run_id)
    .fetch_optional(&mut *conn)
    .await?;

    let (period_start, period_end) = match run {
        Some(row) => {
            let status: String = row.get("status");
            if status != "approved" {
                return Err(ReportError::NotFound);
            }
            let start: NaiveDate = row.get("period_start");
            let end: NaiveDate = row.get("period_end");
            (start, end)
        }
        None => return Err(ReportError::NotFound),
    };

    let rows = sqlx::query(
        "SELECT e.employee_code, \
                 e.first_name_th || ' ' || e.last_name_th AS name, \
                 p.pf_employee, \
                 p.pf_employer \
          FROM payslips p \
          JOIN employees e ON e.id = p.employee_id \
          WHERE p.payroll_run_id = $1 \
            AND p.tenant_id = current_tenant_id() \
            AND (p.pf_employee > 0 OR p.pf_employer > 0) \
          ORDER BY e.employee_code",
    )
    .bind(run_id)
    .fetch_all(&mut *conn)
    .await?;

    let filename = format!("pf-remittance-{run_id}-{period_start}-{period_end}.csv");

    let mut total_employee = Decimal::ZERO;
    let mut total_employer = Decimal::ZERO;
    let mut body =
        String::from("\u{feff}employee_code,name,employee_amount,employer_amount,total\r\n");
    for row in &rows {
        let code: String = row.get("employee_code");
        let name: String = row.get("name");
        let employee: Decimal = row.get("pf_employee");
        let employer: Decimal = row.get("pf_employer");
        let total = employee + employer;
        total_employee += employee;
        total_employer += employer;
        body.push_str(&format!(
            "{},{},{},{},{}\r\n",
            csv_field(&code),
            csv_field(&name),
            csv_field(&format_decimal(employee)),
            csv_field(&format_decimal(employer)),
            csv_field(&format_decimal(total)),
        ));
    }
    let grand_total = total_employee + total_employer;
    body.push_str(&format!(
        "{},{},{},{},{}\r\n",
        csv_field(""),
        csv_field(""),
        csv_field(&format_decimal(total_employee)),
        csv_field(&format_decimal(total_employer)),
        csv_field(&format_decimal(grand_total)),
    ));

    Ok((filename, body))
}

/// GL journal CSV for an approved payroll run.
/// Returns `(filename, body)`. Only `approved` runs are exported; anything else
/// or a run not visible to the tenant yields `NotFound`.
pub async fn export_gl_csv(
    conn: &mut PgConnection,
    run_id: Uuid,
) -> Result<(String, String), ReportError> {
    let run = sqlx::query(
        "SELECT status, period_start, period_end \
         FROM payroll_runs \
         WHERE id = $1 \
           AND tenant_id = current_tenant_id()",
    )
    .bind(run_id)
    .fetch_optional(&mut *conn)
    .await?;

    let (period_start, period_end) = match run {
        Some(row) => {
            let status: String = row.get("status");
            if status != "approved" {
                return Err(ReportError::NotFound);
            }
            let start: NaiveDate = row.get("period_start");
            let end: NaiveDate = row.get("period_end");
            (start, end)
        }
        None => return Err(ReportError::NotFound),
    };

    let rows = sqlx::query(
        "SELECT COALESCE(e.department, $1) AS department, \
                COALESCE(SUM(p.gross_earnings), 0) AS gross, \
                COALESCE(SUM(p.sso_employer), 0) AS sso_employer, \
                COALESCE(SUM(p.pf_employer), 0) AS pf_employer, \
                COALESCE(SUM(p.net_pay), 0) AS net, \
                COALESCE(SUM(p.tax_withheld), 0) AS tax, \
                COALESCE(SUM(p.sso_employee), 0) AS sso_employee, \
                COALESCE(SUM(p.pf_employee), 0) AS pf_employee, \
                COALESCE(SUM(p.loan_deduction), 0) AS loan_deduction \
         FROM payslips p \
         JOIN employees e ON e.id = p.employee_id \
         WHERE p.payroll_run_id = $2 \
           AND p.tenant_id = current_tenant_id() \
         GROUP BY e.department \
         ORDER BY department",
    )
    .bind(UNKNOWN_DEPARTMENT)
    .bind(run_id)
    .fetch_all(&mut *conn)
    .await?;

    let description = format!("รอบเงินเดือน {period_start} ถึง {period_end}");
    let filename = format!("gl-journal-{run_id}-{period_start}-{period_end}.csv");

    let mut lines: Vec<GlLine> = Vec::with_capacity(rows.len() * 6);
    for row in &rows {
        let department: String = row.get("department");
        let gross: Decimal = row.get("gross");
        let sso_employer: Decimal = row.get("sso_employer");
        let pf_employer: Decimal = row.get("pf_employer");
        let net: Decimal = row.get("net");
        let tax: Decimal = row.get("tax");
        let sso_employee: Decimal = row.get("sso_employee");
        let pf_employee: Decimal = row.get("pf_employee");
        let loan_deduction: Decimal = row.get("loan_deduction");

        let debit = gross + sso_employer + pf_employer;
        lines.push(GlLine {
            account_code: "5100".into(),
            account_name: "ค่าแรง".into(),
            department: department.clone(),
            debit,
            credit: Decimal::ZERO,
            description: description.clone(),
        });
        lines.push(GlLine {
            account_code: "2100".into(),
            account_name: "เงินเดือนค้างจ่าย".into(),
            department: department.clone(),
            debit: Decimal::ZERO,
            credit: net,
            description: description.clone(),
        });
        lines.push(GlLine {
            account_code: "2200".into(),
            account_name: "ภาษีหัก ณ ที่จ่าย".into(),
            department: department.clone(),
            debit: Decimal::ZERO,
            credit: tax,
            description: description.clone(),
        });
        lines.push(GlLine {
            account_code: "2210".into(),
            account_name: "ประกันสังคม".into(),
            department: department.clone(),
            debit: Decimal::ZERO,
            credit: sso_employee + sso_employer,
            description: description.clone(),
        });
        lines.push(GlLine {
            account_code: "2220".into(),
            account_name: "กองทุนสำรองเลี้ยงชีพ".into(),
            department: department.clone(),
            debit: Decimal::ZERO,
            credit: pf_employee + pf_employer,
            description: description.clone(),
        });
        lines.push(GlLine {
            account_code: "2230".into(),
            account_name: "เงินให้กู้พนักงาน".into(),
            department: department.clone(),
            debit: Decimal::ZERO,
            credit: loan_deduction,
            description: description.clone(),
        });
    }

    lines.sort_by(|a, b| {
        a.account_code
            .cmp(&b.account_code)
            .then(a.department.cmp(&b.department))
    });

    let mut body =
        String::from("\u{feff}account_code,account_name,department,debit,credit,description\r\n");
    for line in &lines {
        body.push_str(&format!(
            "{},{},{},{},{},{}\r\n",
            csv_field(&line.account_code),
            csv_field(&line.account_name),
            csv_field(&line.department),
            csv_field(&format_decimal(line.debit)),
            csv_field(&format_decimal(line.credit)),
            csv_field(&line.description),
        ));
    }

    Ok((filename, body))
}

fn csv_field(value: &str) -> String {
    // Spreadsheet formula-injection guard.
    let safe = if value.starts_with(['=', '+', '-', '@']) {
        format!("'{value}")
    } else {
        value.to_string()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}

fn format_decimal(value: Decimal) -> String {
    value.round_dp(2).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_field_escapes_and_prefixes_injection_chars() {
        assert_eq!(csv_field("=cmd"), "\"'=cmd\"");
        assert_eq!(csv_field("+1"), "\"'+1\"");
        assert_eq!(csv_field("-1"), "\"'-1\"");
        assert_eq!(csv_field("@ref"), "\"'@ref\"");
        assert_eq!(csv_field("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_field("normal"), "\"normal\"");
    }

    #[test]
    fn format_decimal_rounds_to_two_places() {
        assert_eq!(format_decimal(Decimal::new(12345, 2)), "123.45");
    }
}

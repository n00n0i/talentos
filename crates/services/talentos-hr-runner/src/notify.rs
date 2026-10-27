//! Notification outbox enqueue hooks.
//!
//! These helpers render Thai subject/body strings and insert rows into the
//! tenant-scoped `notifications` outbox. Delivery is performed later by the
//! gateway worker so HR operations stay fast and transactional.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

const CHANNEL_EMAIL: &str = "email";
const CHANNEL_LINE: &str = "line";

fn status_th(status: &str) -> &str {
    match status {
        "approved" => "อนุมัติ",
        "rejected" => "ปฏิเสธ",
        _ => status,
    }
}

/// Notify HR admins and managers when an employee submits a leave request.
pub async fn enqueue_leave_submitted(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    employee_id: Uuid,
    label_th: &str,
    start: NaiveDate,
    end: NaiveDate,
    days: Decimal,
) -> Result<(), sqlx::Error> {
    let employee = sqlx::query(
        "SELECT first_name_th, last_name_th FROM employees WHERE id = $1 AND tenant_id = $2",
    )
    .bind(employee_id)
    .bind(tenant_id)
    .fetch_one(&mut **tx)
    .await?;

    let first_name: String = employee.try_get("first_name_th")?;
    let last_name: String = employee.try_get("last_name_th")?;
    let employee_name = format!("{first_name} {last_name}");

    let subject = format!("มีคำขอ{label_th}ใหม่จาก {employee_name}");
    let body = format!("{employee_name} ขอ{label_th} วันที่ {start} ถึง {end} ({days} วัน)");

    let recipients = sqlx::query(
        r#"SELECT u.id, u.email, u.line_user_id
           FROM users u
           JOIN user_roles ur ON ur.user_id = u.id
           JOIN roles r ON r.id = ur.role_id
           WHERE u.tenant_id = $1
             AND u.is_active = TRUE
             AND r.name IN ('hr_admin', 'manager')
           GROUP BY u.id"#,
    )
    .bind(tenant_id)
    .fetch_all(&mut **tx)
    .await?;

    for row in recipients {
        let user_id: Uuid = row.try_get("id")?;
        let email: String = row.try_get("email")?;
        insert_notification(
            tx,
            tenant_id,
            user_id,
            CHANNEL_EMAIL,
            email,
            &subject,
            &body,
        )
        .await?;
        if let Ok(Some(line_id)) = row.try_get::<Option<String>, _>("line_user_id") {
            insert_notification(
                tx,
                tenant_id,
                user_id,
                CHANNEL_LINE,
                line_id,
                &subject,
                &body,
            )
            .await?;
        }
    }

    Ok(())
}

/// Notify an employee when their leave request is approved or rejected.
pub async fn enqueue_leave_decided(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    employee_id: Uuid,
    status: &str,
    label_th: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<(), sqlx::Error> {
    let row = sqlx::query(
        r#"SELECT u.id, u.email, u.line_user_id
           FROM users u
           JOIN employees e ON e.user_id = u.id
           WHERE e.id = $1 AND e.tenant_id = $2 AND u.is_active = TRUE"#,
    )
    .bind(employee_id)
    .bind(tenant_id)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(row) = row else { return Ok(()) };

    let user_id: Uuid = row.try_get("id")?;
    let email: String = row.try_get("email")?;
    let status_label = status_th(status);

    let subject = format!("คำขอ{label_th}ของคุณถูก{status_label}");
    let body = format!("คำขอ{label_th} วันที่ {start} ถึง {end} ถูก{status_label}");

    insert_notification(
        tx,
        tenant_id,
        user_id,
        CHANNEL_EMAIL,
        email,
        &subject,
        &body,
    )
    .await?;
    if let Ok(Some(line_id)) = row.try_get::<Option<String>, _>("line_user_id") {
        insert_notification(
            tx,
            tenant_id,
            user_id,
            CHANNEL_LINE,
            line_id,
            &subject,
            &body,
        )
        .await?;
    }

    Ok(())
}

/// Notify employees linked to payslips in an approved payroll run.
pub async fn enqueue_payslip_ready(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    run_id: Uuid,
    period_start: NaiveDate,
    period_end: NaiveDate,
) -> Result<(), sqlx::Error> {
    let subject = "สลิปเงินเดือนพร้อมให้ดาวน์โหลด";
    let app_url = std::env::var("APP_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
    let body = format!(
        "สลิปเงินเดือนสำหรับงวด {period_start} ถึง {period_end} พร้อมให้ดาวน์โหลดแล้ว เปิดดูได้ที่ {app_url}/me/payslips"
    );

    let recipients = sqlx::query(
        r#"SELECT DISTINCT u.id, u.email, u.line_user_id
           FROM users u
           JOIN employees e ON e.user_id = u.id
           JOIN payslips p ON p.employee_id = e.id AND p.tenant_id = e.tenant_id
           WHERE p.payroll_run_id = $1 AND p.tenant_id = $2 AND u.is_active = TRUE"#,
    )
    .bind(run_id)
    .bind(tenant_id)
    .fetch_all(&mut **tx)
    .await?;

    for row in recipients {
        let user_id: Uuid = row.try_get("id")?;
        let email: String = row.try_get("email")?;
        insert_notification(tx, tenant_id, user_id, CHANNEL_EMAIL, email, subject, &body).await?;
        if let Ok(Some(line_id)) = row.try_get::<Option<String>, _>("line_user_id") {
            insert_notification(
                tx,
                tenant_id,
                user_id,
                CHANNEL_LINE,
                line_id,
                subject,
                &body,
            )
            .await?;
        }
    }

    Ok(())
}

async fn insert_notification(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
    channel: &str,
    recipient: String,
    subject: &str,
    body: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO notifications
             (tenant_id, user_id, channel, recipient, subject, body, status, attempts)
           VALUES ($1, $2, $3, $4, $5, $6, 'pending', 0)"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(channel)
    .bind(recipient)
    .bind(subject)
    .bind(body)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Rendered body for a payslip-ready notification (kept pure for tests).
pub fn payslip_ready_body(
    period_start: chrono::NaiveDate,
    period_end: chrono::NaiveDate,
    app_url: &str,
) -> String {
    format!(
        "สลิปเงินเดือนสำหรับงวด {period_start} ถึง {period_end} พร้อมให้ดาวน์โหลดแล้ว เปิดดูได้ที่ {app_url}/me/payslips"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thai_decision_status_renders_approved_and_rejected() {
        assert_eq!(status_th("approved"), "อนุมัติ");
        assert_eq!(status_th("rejected"), "ปฏิเสธ");
        assert_eq!(status_th("unknown"), "unknown");
    }

    #[test]
    fn leave_submitted_rendering_contains_no_pii_or_salary() {
        let name = "สมชาย ทดสอบ";
        let label = "ลาพักร้อน";
        let subject = format!("มีคำขอ{label}ใหม่จาก {name}");
        let body = format!("{name} ขอ{label} วันที่ 2026-10-05 ถึง 2026-10-06 (2 วัน)");
        assert!(subject.contains("ลาพักร้อน"));
        assert!(body.contains("2026-10-05"));
        assert!(!body.contains("เงินเดือน"));
        assert!(!body.contains("บัตรประชาชน"));
    }

    #[test]
    fn payslip_ready_rendering_contains_period_and_link() {
        let body = payslip_ready_body(
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 31).unwrap(),
            "https://hr.example.com",
        );
        assert!(body.contains("2026-10-01"));
        assert!(body.contains("2026-10-31"));
        assert!(body.contains("https://hr.example.com/me/payslips"));
        assert!(!body.contains("net_pay"));
        assert!(!body.contains("30,000"));
    }
}

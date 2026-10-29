//! Approval SLA escalation: notify active HR admins once per overdue step.
//!
//! A `pending` step whose `created_at` is older than the tenant SLA (hours) and
//! that has not been escalated yet gets one outbox notification per active HR
//! admin, then `escalated_at` is stamped so the next sweep skips it. Runs per
//! tenant inside a tenant-scoped transaction (RLS sees the right tenant).

use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

use super::notify::insert_notification;

const CHANNEL_EMAIL: &str = "email";
const CHANNEL_LINE: &str = "line";
const DEFAULT_SLA_HOURS: i32 = 48;

/// Escalate overdue pending approval steps for one tenant. Returns the number
/// of steps escalated. SLA 0 disables the sweep and returns 0 without writes.
pub async fn escalate_overdue(
    pool: &TenantPool,
    tenant_id: TenantId,
    limit: i64,
) -> Result<usize, sqlx::Error> {
    let mut tx = pool.begin_tenant(tenant_id).await?;

    let sla_hours: i32 = sqlx::query_scalar(
        "SELECT COALESCE(
            (SELECT approval_sla_hours FROM tenant_payroll_settings WHERE tenant_id = $1),
            $2)",
    )
    .bind(tenant_id.0)
    .bind(DEFAULT_SLA_HOURS)
    .fetch_one(&mut *tx)
    .await?;

    if sla_hours == 0 {
        tx.rollback().await?;
        return Ok(0);
    }

    let overdue = sqlx::query(
        r#"SELECT s.id AS step_id, s.step_no, s.approver_role,
                  e.first_name_th, e.last_name_th,
                  lr.leave_type, lr.start_date, lr.end_date,
                  COALESCE(
                    (SELECT lt.label_th FROM leave_types lt
                      WHERE lt.name = lr.leave_type AND lt.tenant_id = s.tenant_id
                      ORDER BY lt.id LIMIT 1),
                    (SELECT lt.label_th FROM leave_types lt
                      WHERE lt.name = lr.leave_type AND lt.tenant_id IS NULL
                      ORDER BY lt.id LIMIT 1),
                    lr.leave_type
                  ) AS label_th
             FROM leave_approval_steps s
             JOIN leave_requests lr ON lr.id = s.leave_request_id
             JOIN employees e ON e.id = lr.employee_id
            WHERE s.tenant_id = $1
              AND s.status = 'pending'
              AND s.escalated_at IS NULL
              AND s.created_at < now() - make_interval(hours => $2)
            ORDER BY s.created_at
            LIMIT $3
            FOR UPDATE OF s"#,
    )
    .bind(tenant_id.0)
    .bind(sla_hours)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;

    if overdue.is_empty() {
        tx.rollback().await?;
        return Ok(0);
    }

    // All overdue steps in a tenant share the same HR admin set: fetch once.
    let recipients = sqlx::query(
        r#"SELECT u.id, u.email, u.line_user_id
             FROM users u
             JOIN user_roles ur ON ur.user_id = u.id
             JOIN roles r ON r.id = ur.role_id
            WHERE u.tenant_id = $1
              AND u.is_active = TRUE
              AND r.name = 'hr_admin'
            GROUP BY u.id"#,
    )
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await?;

    let mut escalated = 0usize;

    for row in &overdue {
        let step_id: Uuid = row.try_get("step_id")?;
        let step_no: i32 = row.try_get("step_no")?;
        let approver_role: String = row.try_get("approver_role")?;
        let first_name: String = row.try_get("first_name_th")?;
        let last_name: String = row.try_get("last_name_th")?;
        let label_th: String = row.try_get("label_th")?;
        let start: chrono::NaiveDate = row.try_get("start_date")?;
        let end: chrono::NaiveDate = row.try_get("end_date")?;

        let employee_name = format!("{first_name} {last_name}");
        let role_th = match approver_role.as_str() {
            "hr_admin" => "HR",
            "manager" => "หัวหน้างาน",
            other => other,
        };
        let subject = format!("คำขอ{label_th}ของ {employee_name} รอการอนุมัติเกินกำหนด");
        let body = format!(
            "{employee_name} ขอ{label_th} วันที่ {start} ถึง {end} ยังรอการอนุมัติขั้นตอน {step_no} ({role_th}) เกินเวลาที่กำหนด"
        );

        for recipient in &recipients {
            let user_id: Uuid = recipient.try_get("id")?;
            let email: String = recipient.try_get("email")?;
            insert_notification(
                &mut tx,
                tenant_id.0,
                user_id,
                CHANNEL_EMAIL,
                email,
                &subject,
                &body,
            )
            .await?;
            if let Ok(Some(line_id)) = recipient.try_get::<Option<String>, _>("line_user_id") {
                insert_notification(
                    &mut tx,
                    tenant_id.0,
                    user_id,
                    CHANNEL_LINE,
                    line_id,
                    &subject,
                    &body,
                )
                .await?;
            }
        }

        sqlx::query("UPDATE leave_approval_steps SET escalated_at = now() WHERE id = $1")
            .bind(step_id)
            .execute(&mut *tx)
            .await?;
        escalated += 1;
    }

    tx.commit().await?;
    Ok(escalated)
}

//! PDPA self-service: privacy-notice acknowledgement and a personal-data export
//! (DSAR). Everything is scoped to the authenticated user; tenant isolation is
//! enforced by `begin_tenant` and explicit `tenant_id` predicates.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::Row;
use talentos_db::pii::{is_encrypted, PiiCipher};
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

/// Bump when the privacy notice changes; users must accept the new version.
pub const NOTICE_VERSION: &str = "2026-10-30";

#[derive(Debug, thiserror::Error)]
pub enum PrivacyError {
    #[error("unsupported privacy notice version")]
    InvalidVersion,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug)]
pub struct PrivacyStatus {
    pub notice_version: &'static str,
    pub accepted_at: Option<DateTime<Utc>>,
}

/// The current notice version and when this user accepted it (if ever).
pub async fn privacy_status(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
) -> Result<PrivacyStatus, PrivacyError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let accepted_at: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT accepted_at FROM privacy_consents
          WHERE tenant_id = $1 AND user_id = $2 AND version = $3",
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .bind(NOTICE_VERSION)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(PrivacyStatus {
        notice_version: NOTICE_VERSION,
        accepted_at,
    })
}

/// Record acknowledgement of the current notice. Repeating the call keeps the
/// original timestamp, so the audit trail shows the first acceptance.
pub async fn accept_privacy(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    version: &str,
) -> Result<DateTime<Utc>, PrivacyError> {
    if version != NOTICE_VERSION {
        return Err(PrivacyError::InvalidVersion);
    }
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let accepted_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"INSERT INTO privacy_consents (tenant_id, user_id, version)
           VALUES ($1, $2, $3)
           ON CONFLICT (tenant_id, user_id, version)
           DO UPDATE SET accepted_at = privacy_consents.accepted_at
           RETURNING accepted_at"#,
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .bind(version)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(accepted_at)
}

fn plain(pii: &Option<Arc<PiiCipher>>, stored: Option<String>) -> Option<String> {
    let stored = stored?;
    match pii {
        Some(cipher) => cipher.decrypt(&stored).ok(),
        None if is_encrypted(&stored) => None,
        None => Some(stored),
    }
}

async fn scalar_json(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    sql: &str,
    tenant_id: Uuid,
    employee_id: Uuid,
) -> Result<Value, sqlx::Error> {
    let value: Option<Value> = sqlx::query_scalar(sql)
        .bind(tenant_id)
        .bind(employee_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(value.unwrap_or_else(|| json!([])))
}

/// Complete personal-data copy for the authenticated user. Includes every
/// employee row linked to the user (multi-entity) and is audited in the same
/// transaction.
pub async fn export_personal_data(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    pii: &Option<Arc<PiiCipher>>,
) -> Result<Value, PrivacyError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;

    let account = sqlx::query(
        "SELECT email, full_name_th, full_name_en, line_user_id, created_at
           FROM users WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?
    .map(|row| {
        json!({
            "email": row.get::<String, _>("email"),
            "full_name_th": row.get::<Option<String>, _>("full_name_th"),
            "full_name_en": row.get::<Option<String>, _>("full_name_en"),
            "line_user_id": row.get::<Option<String>, _>("line_user_id"),
            "created_at": row.get::<DateTime<Utc>, _>("created_at"),
        })
    });

    let roles: Value = sqlx::query_scalar(
        r#"SELECT COALESCE(json_agg(r.name ORDER BY r.name), '[]'::json)
             FROM user_roles ur JOIN roles r ON r.id = ur.role_id
            WHERE r.tenant_id = $1 AND ur.user_id = $2"#,
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;

    let consents: Value = sqlx::query_scalar(
        r#"SELECT COALESCE(json_agg(json_build_object(
                 'version', version, 'accepted_at', accepted_at) ORDER BY accepted_at), '[]'::json)
             FROM privacy_consents WHERE tenant_id = $1 AND user_id = $2"#,
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;

    let notifications: Value = sqlx::query_scalar(
        r#"SELECT COALESCE(json_agg(json_build_object(
                 'channel', channel, 'subject', subject, 'body', body,
                 'status', status, 'sent_at', sent_at, 'created_at', created_at)
               ORDER BY created_at), '[]'::json)
             FROM notifications WHERE tenant_id = $1 AND user_id = $2"#,
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;

    // No status filter: a terminated employee still has the right to their data.
    let employee_rows = sqlx::query(
        r#"SELECT id, employee_code, first_name_th, last_name_th, first_name_en, last_name_en,
                  national_id, birth_date, hire_date, termination_date, position_title, department,
                  cost_center, base_salary, bank_name, bank_account_number, sso_number, pf_member,
                  status, created_at, updated_at
             FROM employees
            WHERE tenant_id = $1 AND user_id = $2
            ORDER BY created_at, id"#,
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;

    let mut employees = Vec::with_capacity(employee_rows.len());
    for row in &employee_rows {
        let id: Uuid = row.get("id");
        let attendance_days = scalar_json(
            &mut tx,
            r#"SELECT json_agg(json_build_object(
                   'work_date', work_date, 'worked_minutes', worked_minutes,
                   'late_minutes', late_minutes, 'early_leave_minutes', early_leave_minutes,
                   'ot_minutes', ot_minutes, 'ot_kind', ot_kind,
                   'incomplete', incomplete, 'status', status) ORDER BY work_date)
                 FROM attendance_days WHERE tenant_id = $1 AND employee_id = $2"#,
            tenant_id.0,
            id,
        )
        .await?;
        let attendance_events = scalar_json(
            &mut tx,
            r#"SELECT json_agg(json_build_object(
                   'kind', kind, 'source', source, 'occurred_at', occurred_at,
                   'work_date', work_date, 'distance_m', distance_m, 'accuracy_m', accuracy_m)
                 ORDER BY occurred_at)
                 FROM attendance_events WHERE tenant_id = $1 AND employee_id = $2"#,
            tenant_id.0,
            id,
        )
        .await?;
        let leave = scalar_json(
            &mut tx,
            r#"SELECT json_agg(json_build_object(
                   'leave_type', leave_type, 'start_date', start_date, 'end_date', end_date,
                   'days', days::text, 'reason', reason, 'status', status,
                   'created_at', created_at) ORDER BY start_date)
                 FROM leave_requests WHERE tenant_id = $1 AND employee_id = $2"#,
            tenant_id.0,
            id,
        )
        .await?;
        let ot = scalar_json(
            &mut tx,
            r#"SELECT json_agg(json_build_object(
                   'work_date', work_date, 'minutes', minutes, 'reason', reason,
                   'status', status, 'created_at', created_at) ORDER BY work_date)
                 FROM ot_requests WHERE tenant_id = $1 AND employee_id = $2"#,
            tenant_id.0,
            id,
        )
        .await?;
        let claims = scalar_json(
            &mut tx,
            r#"SELECT json_agg(json_build_object(
                   'category', category, 'amount', amount::text, 'expense_date', expense_date,
                   'description', description, 'status', status, 'created_at', created_at)
                 ORDER BY expense_date)
                 FROM expense_claims WHERE tenant_id = $1 AND employee_id = $2"#,
            tenant_id.0,
            id,
        )
        .await?;
        let loans = scalar_json(
            &mut tx,
            r#"SELECT json_agg(json_build_object(
                   'principal', principal::text, 'balance', balance::text,
                   'installment_amount', installment_amount::text, 'start_date', start_date,
                   'status', status, 'note', note, 'created_at', created_at) ORDER BY created_at)
                 FROM employee_loans WHERE tenant_id = $1 AND employee_id = $2"#,
            tenant_id.0,
            id,
        )
        .await?;
        let onboarding_tasks = scalar_json(
            &mut tx,
            r#"SELECT json_agg(json_build_object(
                   'title', title, 'notes', notes, 'due_date', due_date,
                   'status', status, 'completed_at', completed_at) ORDER BY created_at)
                 FROM employee_onboarding_tasks WHERE tenant_id = $1 AND employee_id = $2"#,
            tenant_id.0,
            id,
        )
        .await?;
        let training = scalar_json(
            &mut tx,
            r#"SELECT json_agg(json_build_object(
                   'title', title, 'provider', provider, 'completed_on', completed_on,
                   'expires_on', expires_on) ORDER BY completed_on DESC NULLS LAST)
                 FROM employee_training_records WHERE tenant_id = $1 AND employee_id = $2"#,
            tenant_id.0,
            id,
        )
        .await?;
        let reviews = scalar_json(
            &mut tx,
            r#"SELECT json_agg(json_build_object(
                   'cycle', c.name, 'period_start', c.period_start, 'period_end', c.period_end,
                   'status', r.status, 'rating', r.rating, 'strengths', r.strengths,
                   'improvements', r.improvements, 'submitted_at', r.submitted_at)
                 ORDER BY c.period_start)
                 FROM performance_reviews r
                 JOIN performance_cycles c ON c.id = r.cycle_id
                WHERE r.tenant_id = $1 AND r.employee_id = $2"#,
            tenant_id.0,
            id,
        )
        .await?;
        let documents = scalar_json(
            &mut tx,
            r#"SELECT json_agg(json_build_object(
                   'doc_type', doc_type, 'title', title, 'doc_number', doc_number,
                   'issued_date', issued_date, 'expiry_date', expiry_date,
                   'has_file', file_path IS NOT NULL) ORDER BY created_at)
                 FROM employee_documents WHERE tenant_id = $1 AND employee_id = $2"#,
            tenant_id.0,
            id,
        )
        .await?;
        let payslips = scalar_json(
            &mut tx,
            r#"SELECT json_agg(json_build_object(
                   'period_start', r.period_start, 'period_end', r.period_end,
                   'base_salary', p.base_salary::text, 'gross', p.gross_earnings::text,
                   'deductions', p.total_deductions::text, 'net', p.net_pay::text,
                   'tax', p.tax_withheld::text, 'sso_employee', p.sso_employee::text,
                   'sso_employer', p.sso_employer::text, 'pf_employee', p.pf_employee::text,
                   'pf_employer', p.pf_employer::text, 'status', p.status) ORDER BY r.period_start)
                 FROM payslips p JOIN payroll_runs r ON r.id = p.payroll_run_id
                WHERE p.tenant_id = $1 AND p.employee_id = $2 AND r.tenant_id = $1"#,
            tenant_id.0,
            id,
        )
        .await?;

        employees.push(json!({
            "employee_code": row.get::<String, _>("employee_code"),
            "first_name_th": row.get::<String, _>("first_name_th"),
            "last_name_th": row.get::<String, _>("last_name_th"),
            "first_name_en": row.get::<Option<String>, _>("first_name_en"),
            "last_name_en": row.get::<Option<String>, _>("last_name_en"),
            "national_id": plain(pii, row.get("national_id")),
            "birth_date": row.get::<Option<chrono::NaiveDate>, _>("birth_date").map(|d| d.to_string()),
            "hire_date": row.get::<chrono::NaiveDate, _>("hire_date").to_string(),
            "termination_date": row.get::<Option<chrono::NaiveDate>, _>("termination_date").map(|d| d.to_string()),
            "position_title": row.get::<String, _>("position_title"),
            "department": row.get::<Option<String>, _>("department"),
            "cost_center": row.get::<Option<String>, _>("cost_center"),
            "base_salary": row.get::<rust_decimal::Decimal, _>("base_salary").to_string(),
            "bank_name": row.get::<Option<String>, _>("bank_name"),
            "bank_account_number": plain(pii, row.get("bank_account_number")),
            "sso_number": plain(pii, row.get("sso_number")),
            "pf_member": row.get::<bool, _>("pf_member"),
            "status": row.get::<String, _>("status"),
            "created_at": row.get::<DateTime<Utc>, _>("created_at"),
            "updated_at": row.get::<DateTime<Utc>, _>("updated_at"),
            "attendance_days": attendance_days,
            "attendance_events": attendance_events,
            "leave_requests": leave,
            "ot_requests": ot,
            "expense_claims": claims,
            "loans": loans,
            "documents": documents,
            "onboarding_tasks": onboarding_tasks,
            "training": training,
            "reviews": reviews,
            "payslips": payslips,
        }));
    }

    sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'privacy.export', 'user', $2, $3)"#,
    )
    .bind(tenant_id.0)
    .bind(user_id.to_string())
    .bind(json!({ "notice_version": NOTICE_VERSION }))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(json!({
        "exported_at": Utc::now(),
        "notice_version": NOTICE_VERSION,
        "account": account,
        "roles": roles,
        "consents": consents,
        "notifications": notifications,
        "employees": employees,
    }))
}

//! PF remittance delivery to a configured provider endpoint.
//!
//! The payload is provider-agnostic (built by `talentos_hr_runner::pf`); the
//! gateway POSTs it to `{PF_API_BASE}/remittances` with a bearer token and
//! records success/failure on the per-run `pf_remittances` row.

use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_ERROR_CHARS: usize = 500;

#[derive(Clone)]
pub struct PfProviderConfig {
    pub base_url: String,
    pub token: String,
}

impl PfProviderConfig {
    /// `None` when either `PF_API_BASE` or `PF_API_TOKEN` is missing/empty.
    pub fn from_env() -> Option<Self> {
        let base = std::env::var("PF_API_BASE").ok()?.trim().to_string();
        let token = std::env::var("PF_API_TOKEN").ok()?.trim().to_string();
        if base.is_empty() || token.is_empty() {
            return None;
        }
        Some(Self {
            base_url: base,
            token,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct SendOutcome {
    pub status: String,
    pub attempts: i32,
    pub sent_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("payroll run is not approved")]
    NotApproved,
    #[error("payroll run not found")]
    RunNotFound,
    #[error("PF remittance already sent")]
    AlreadySent,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("payload serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
}

impl From<talentos_hr_runner::pf::PfError> for SendError {
    fn from(e: talentos_hr_runner::pf::PfError) -> Self {
        use talentos_hr_runner::pf::PfError;
        match e {
            PfError::NotApproved => SendError::NotApproved,
            PfError::RunNotFound => SendError::RunNotFound,
            PfError::Db(db) => SendError::Db(db),
            // Unreachable in the remittance path; only rate-editing returns these.
            PfError::EmployeeNotFound | PfError::RateOutOfRange => {
                SendError::Db(sqlx::Error::Protocol("unexpected pf error".into()))
            }
        }
    }
}

/// POST /api/v1/payroll/runs/:id/pf-remittance/send
pub async fn send(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(run_id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payroll_run", "export_report") {
        return forbidden;
    }
    let Some(config) = PfProviderConfig::from_env() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "pf_provider_not_configured".into(),
                message: "ยังไม่ได้ตั้งค่า PF API (PF_API_BASE / PF_API_TOKEN)".into(),
                details: vec![],
                request_id: Uuid::new_v4().to_string(),
                retry_after_seconds: None,
            }),
        )
            .into_response();
    };

    let http = reqwest::Client::new();
    match send_pf_remittance(&state.pool, ctx.tenant_id, &http, &config, run_id).await {
        Ok(outcome) => (StatusCode::OK, Json(outcome)).into_response(),
        Err(SendError::NotApproved) => conflict("payroll run is not approved"),
        Err(SendError::RunNotFound) => not_found(),
        Err(SendError::AlreadySent) => conflict("PF remittance already sent"),
        Err(SendError::Db(e)) => internal_err(e),
        Err(SendError::Serialize(e)) => {
            internal_err_msg(&format!("payload serialization failed: {e}"))
        }
    }
}

/// Build the payload, record an attempt, POST to the provider, and record the
/// result. A provider failure is recorded (status `failed`) and returned, not
/// surfaced as an error, so a retry after `failed` is allowed.
pub async fn send_pf_remittance(
    pool: &TenantPool,
    tenant_id: TenantId,
    http: &reqwest::Client,
    config: &PfProviderConfig,
    run_id: Uuid,
) -> Result<SendOutcome, SendError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;

    let payload = talentos_hr_runner::pf::build_remittance_payload(&mut tx, run_id).await?;
    let payload_json = serde_json::to_value(&payload)?;

    let existing =
        sqlx::query("SELECT status, attempts FROM pf_remittances WHERE payroll_run_id = $1")
            .bind(run_id)
            .fetch_optional(&mut *tx)
            .await?;

    let attempts = match &existing {
        Some(row) => {
            if row.try_get::<String, _>("status")? == "sent" {
                return Err(SendError::AlreadySent);
            }
            row.try_get::<i32, _>("attempts")? + 1
        }
        None => 1,
    };

    sqlx::query(
        r#"INSERT INTO pf_remittances (tenant_id, payroll_run_id, status, attempts, payload)
           VALUES (current_tenant_id(), $1, 'pending', $2, $3)
           ON CONFLICT (tenant_id, payroll_run_id) DO UPDATE SET
               status = EXCLUDED.status,
               attempts = EXCLUDED.attempts,
               payload = EXCLUDED.payload,
               last_error = NULL,
               sent_at = NULL"#,
    )
    .bind(run_id)
    .bind(attempts)
    .bind(&payload_json)
    .execute(&mut *tx)
    .await?;

    let url = format!("{}/remittances", config.base_url.trim_end_matches('/'));
    let result = http
        .post(&url)
        .timeout(REQUEST_TIMEOUT)
        .bearer_auth(&config.token)
        .json(&payload_json)
        .send()
        .await
        .and_then(|r| r.error_for_status());

    match result {
        Ok(_) => {
            sqlx::query(
                "UPDATE pf_remittances SET status = 'sent', sent_at = now(), last_error = NULL \
                 WHERE payroll_run_id = $1",
            )
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(SendOutcome {
                status: "sent".into(),
                attempts,
                sent_at: Some(chrono::Utc::now().to_rfc3339()),
                last_error: None,
            })
        }
        Err(e) => {
            let truncated: String = format!("{e:#}").chars().take(MAX_ERROR_CHARS).collect();
            sqlx::query(
                "UPDATE pf_remittances SET status = 'failed', last_error = $2 WHERE payroll_run_id = $1",
            )
            .bind(run_id)
            .bind(&truncated)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(SendOutcome {
                status: "failed".into(),
                attempts,
                sent_at: None,
                last_error: Some(truncated),
            })
        }
    }
}

fn conflict(message: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(ErrorResponse {
            error: "conflict".into(),
            message: message.to_string(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "not_found".into(),
            message: "Payroll run not found".into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn internal_err(e: sqlx::Error) -> Response {
    tracing::error!(error = %e, "database error");
    internal_err_msg("An unexpected error occurred")
}

fn internal_err_msg(message: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "internal_error".into(),
            message: message.to_string(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::HeaderMap, routing::post, Router};
    use serde_json::Value;
    use std::sync::{Arc, Mutex};

    static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    static TEST_TENANTS: Mutex<Vec<Uuid>> = Mutex::new(Vec::new());

    fn register_tenant(id: Uuid) {
        TEST_TENANTS.lock().unwrap().push(id);
    }
    fn forget_tenant(id: Uuid) {
        let mut v = TEST_TENANTS.lock().unwrap();
        v.retain(|&x| x != id);
    }

    fn db_url() -> Option<String> {
        std::env::var("DATABASE_URL").ok()
    }
    fn admin_db_url() -> Option<String> {
        std::env::var("DATABASE_ADMIN_URL").ok()
    }

    #[derive(Debug, Clone, Default)]
    struct Received {
        calls: Vec<(HeaderMap, Value)>,
    }

    async fn stub_server(state: Arc<Mutex<Received>>) -> (reqwest::Client, std::net::SocketAddr) {
        let app = Router::new()
            .route("/remittances", post(remittance_handler))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        (reqwest::Client::new(), addr)
    }

    async fn remittance_handler(
        axum::extract::State(state): axum::extract::State<Arc<Mutex<Received>>>,
        headers: HeaderMap,
        Json(payload): Json<Value>,
    ) -> StatusCode {
        state.lock().unwrap().calls.push((headers, payload));
        StatusCode::OK
    }

    async fn failing_server() -> (reqwest::Client, std::net::SocketAddr) {
        let app = Router::new().route(
            "/remittances",
            post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        (reqwest::Client::new(), addr)
    }

    fn config(base: &str) -> PfProviderConfig {
        PfProviderConfig {
            base_url: base.to_string(),
            token: "test-token".into(),
        }
    }

    struct Seeded {
        tenant_id: Uuid,
        run_id: Uuid,
        member_code: String,
    }

    /// Seed a tenant with one approved run whose payslips have one PF member and
    /// one non-member. Caller cleans up via `cleanup_tenant`.
    async fn seed(admin: &sqlx::PgPool, app: &TenantPool, status: &str) -> Seeded {
        let tenant_id = Uuid::new_v4();
        register_tenant(tenant_id);

        sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'PF Test', $2)")
            .bind(tenant_id)
            .bind(format!("pf-{}", &tenant_id.to_string()[..8]))
            .execute(admin)
            .await
            .expect("insert tenant");

        let mut tx = app.begin_tenant(TenantId(tenant_id)).await.unwrap();

        let entity_id = Uuid::new_v4();
        sqlx::query("INSERT INTO entities (id, tenant_id, name) VALUES ($1, $2, 'Entity')")
            .bind(entity_id)
            .bind(tenant_id)
            .execute(&mut *tx)
            .await
            .unwrap();

        let user_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, tenant_id, email, password_hash, is_active) \
             VALUES ($1, $2, 'pf@example.com', 'x', true)",
        )
        .bind(user_id)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .unwrap();

        let run_id = Uuid::new_v4();
        let period_start = chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let period_end = chrono::NaiveDate::from_ymd_opt(2026, 10, 31).unwrap();
        let payment = chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        sqlx::query(
            "INSERT INTO payroll_runs (id, tenant_id, entity_id, period_start, period_end, payment_date, status, created_by) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(run_id)
        .bind(tenant_id)
        .bind(entity_id)
        .bind(period_start)
        .bind(period_end)
        .bind(payment)
        .bind(status)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .unwrap();

        let (member_code, non_member_code) = ("PF001".to_string(), "PF002".to_string());
        for (code, pf_employee, pf_employer) in [
            (member_code.as_str(), 1500.00f64, 1500.00f64),
            (non_member_code.as_str(), 0.0, 0.0),
        ] {
            let employee_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO employees (id, tenant_id, entity_id, user_id, employee_code, first_name_th, last_name_th, hire_date, position_title, base_salary, pf_member, status) \
                 VALUES ($1, $2, $3, $4, $5, 'ทดสอบ', 'พนักงาน', '2024-01-01', 'Dev', 40000.00, true, 'active')",
            )
            .bind(employee_id)
            .bind(tenant_id)
            .bind(entity_id)
            .bind(user_id)
            .bind(code)
            .execute(&mut *tx)
            .await
            .unwrap();

            let payslip_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO payslips (id, payroll_run_id, tenant_id, employee_id, base_salary, gross_earnings, net_pay, pf_employee, pf_employer) \
                 VALUES ($1, $2, $3, $4, 40000.00, 40000.00, 37000.00, $5, $6)",
            )
            .bind(payslip_id)
            .bind(run_id)
            .bind(tenant_id)
            .bind(employee_id)
            .bind(pf_employee)
            .bind(pf_employer)
            .execute(&mut *tx)
            .await
            .unwrap();
        }

        tx.commit().await.unwrap();

        Seeded {
            tenant_id,
            run_id,
            member_code,
        }
    }

    async fn cleanup_tenant(admin: &sqlx::PgPool, tenant_id: Uuid) {
        for stmt in [
            "DELETE FROM pf_remittances WHERE tenant_id = $1",
            "DELETE FROM payslips WHERE tenant_id = $1",
            "DELETE FROM payroll_runs WHERE tenant_id = $1",
            "DELETE FROM employees WHERE tenant_id = $1",
            "DELETE FROM user_roles WHERE user_id IN (SELECT id FROM users WHERE tenant_id = $1)",
            "DELETE FROM users WHERE tenant_id = $1",
            "DELETE FROM entities WHERE tenant_id = $1",
            "DELETE FROM tenants WHERE id = $1",
        ] {
            let _ = sqlx::query(stmt).bind(tenant_id).execute(admin).await;
        }
        forget_tenant(tenant_id);
    }

    #[test]
    fn config_requires_base_and_token() {
        assert!(PfProviderConfig::from_env().is_none());
    }

    #[tokio::test]
    async fn sends_path_header_and_body_and_marks_sent() {
        let _guard = TEST_LOCK.lock().await;
        let (Some(url), Some(admin_url)) = (db_url(), admin_db_url()) else {
            eprintln!("SKIP: DATABASE_URL/DATABASE_ADMIN_URL unset");
            return;
        };
        let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
        let app = TenantPool::connect(&url, 2).await.unwrap();
        let seeded = seed(&admin, &app, "approved").await;

        let received = Arc::new(Mutex::new(Received::default()));
        let (http, addr) = stub_server(received.clone()).await;

        let outcome = send_pf_remittance(
            &app,
            TenantId(seeded.tenant_id),
            &http,
            &config(&format!("http://{addr}")),
            seeded.run_id,
        )
        .await
        .expect("send");
        assert_eq!(outcome.status, "sent");
        assert_eq!(outcome.attempts, 1);
        assert!(outcome.sent_at.is_some());
        assert!(outcome.last_error.is_none());

        let calls = received.lock().unwrap().calls.clone();
        assert_eq!(calls.len(), 1);
        let (headers, payload) = &calls[0];
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(auth.starts_with("Bearer "));
        assert!(auth.contains("test-token"));
        // Only the PF member is included; the non-member payslip is dropped.
        let members = payload["members"].as_array().unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0]["employee_code"], seeded.member_code);
        assert_eq!(payload["total_employee"], "1500.00");
        assert_eq!(payload["total_employer"], "1500.00");

        cleanup_tenant(&admin, seeded.tenant_id).await;
    }

    #[tokio::test]
    async fn failure_records_error_and_allows_retry() {
        let _guard = TEST_LOCK.lock().await;
        let (Some(url), Some(admin_url)) = (db_url(), admin_db_url()) else {
            eprintln!("SKIP: DATABASE_URL/DATABASE_ADMIN_URL unset");
            return;
        };
        let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
        let app = TenantPool::connect(&url, 2).await.unwrap();
        let seeded = seed(&admin, &app, "approved").await;

        let (http, addr) = failing_server().await;
        let cfg = config(&format!("http://{addr}"));

        let first =
            send_pf_remittance(&app, TenantId(seeded.tenant_id), &http, &cfg, seeded.run_id)
                .await
                .expect("send");
        assert_eq!(first.status, "failed");
        assert_eq!(first.attempts, 1);
        assert!(first.last_error.is_some());

        // Retry after failure is allowed and increments attempts.
        let second =
            send_pf_remittance(&app, TenantId(seeded.tenant_id), &http, &cfg, seeded.run_id)
                .await
                .expect("retry");
        assert_eq!(second.status, "failed");
        assert_eq!(second.attempts, 2);

        cleanup_tenant(&admin, seeded.tenant_id).await;
    }

    #[tokio::test]
    async fn already_sent_is_conflict() {
        let _guard = TEST_LOCK.lock().await;
        let (Some(url), Some(admin_url)) = (db_url(), admin_db_url()) else {
            eprintln!("SKIP: DATABASE_URL/DATABASE_ADMIN_URL unset");
            return;
        };
        let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
        let app = TenantPool::connect(&url, 2).await.unwrap();
        let seeded = seed(&admin, &app, "approved").await;

        let received = Arc::new(Mutex::new(Received::default()));
        let (http, addr) = stub_server(received.clone()).await;
        let cfg = config(&format!("http://{addr}"));

        let first =
            send_pf_remittance(&app, TenantId(seeded.tenant_id), &http, &cfg, seeded.run_id)
                .await
                .expect("send");
        assert_eq!(first.status, "sent");

        let second =
            send_pf_remittance(&app, TenantId(seeded.tenant_id), &http, &cfg, seeded.run_id).await;
        assert!(matches!(second, Err(SendError::AlreadySent)));

        cleanup_tenant(&admin, seeded.tenant_id).await;
    }

    #[tokio::test]
    async fn non_approved_run_is_rejected() {
        let _guard = TEST_LOCK.lock().await;
        let (Some(url), Some(admin_url)) = (db_url(), admin_db_url()) else {
            eprintln!("SKIP: DATABASE_URL/DATABASE_ADMIN_URL unset");
            return;
        };
        let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
        let app = TenantPool::connect(&url, 2).await.unwrap();
        let seeded = seed(&admin, &app, "draft").await;

        let received = Arc::new(Mutex::new(Received::default()));
        let (http, addr) = stub_server(received.clone()).await;

        let result = send_pf_remittance(
            &app,
            TenantId(seeded.tenant_id),
            &http,
            &config(&format!("http://{addr}")),
            seeded.run_id,
        )
        .await;
        assert!(matches!(result, Err(SendError::NotApproved)));
        assert_eq!(received.lock().unwrap().calls.len(), 0);

        cleanup_tenant(&admin, seeded.tenant_id).await;
    }

    #[tokio::test]
    async fn run_is_invisible_to_other_tenant() {
        let _guard = TEST_LOCK.lock().await;
        let (Some(url), Some(admin_url)) = (db_url(), admin_db_url()) else {
            eprintln!("SKIP: DATABASE_URL/DATABASE_ADMIN_URL unset");
            return;
        };
        let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
        let app = TenantPool::connect(&url, 2).await.unwrap();
        let seeded = seed(&admin, &app, "approved").await;

        // A second tenant (no payroll run) must not see or send the first's run.
        let other = Uuid::new_v4();
        register_tenant(other);
        sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'Other', $2)")
            .bind(other)
            .bind(format!("other-{}", &other.to_string()[..8]))
            .execute(&admin)
            .await
            .unwrap();

        let received = Arc::new(Mutex::new(Received::default()));
        let (http, addr) = stub_server(received.clone()).await;

        let result = send_pf_remittance(
            &app,
            TenantId(other),
            &http,
            &config(&format!("http://{addr}")),
            seeded.run_id,
        )
        .await;
        assert!(matches!(result, Err(SendError::RunNotFound)));
        assert_eq!(received.lock().unwrap().calls.len(), 0);

        cleanup_tenant(&admin, seeded.tenant_id).await;
        let _ = sqlx::query("DELETE FROM tenants WHERE id = $1")
            .bind(other)
            .execute(&admin)
            .await;
        forget_tenant(other);
    }
}

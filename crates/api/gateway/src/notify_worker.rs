use std::time::Duration;

use serde_json::json;
use sqlx::Row;
use talentos_db::TenantPool;
use uuid::Uuid;

const DEFAULT_EMAIL_BASE: &str = "https://api.resend.com";
const DEFAULT_LINE_BASE: &str = "https://api.line.me";
const BATCH_LIMIT: i64 = 50;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

const MAX_ATTEMPTS: i32 = 3;

#[derive(Clone)]
pub struct NotifyConfig {
    pub email_api_key: Option<String>,
    pub email_from: Option<String>,
    pub email_api_base: String,
    pub line_channel_token: Option<String>,
    pub line_api_base: String,
}

impl std::fmt::Debug for NotifyConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotifyConfig")
            .field(
                "email_api_key",
                &self.email_api_key.as_deref().map(|_| "***"),
            )
            .field("email_from", &self.email_from)
            .field("email_api_base", &self.email_api_base)
            .field(
                "line_channel_token",
                &self.line_channel_token.as_deref().map(|_| "***"),
            )
            .field("line_api_base", &self.line_api_base)
            .finish()
    }
}

impl NotifyConfig {
    pub fn from_env() -> Self {
        Self {
            email_api_key: std::env::var("EMAIL_API_KEY")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            email_from: std::env::var("EMAIL_FROM")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            email_api_base: std::env::var("EMAIL_API_BASE")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| DEFAULT_EMAIL_BASE.to_string()),
            line_channel_token: std::env::var("LINE_CHANNEL_TOKEN")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            line_api_base: std::env::var("LINE_API_BASE")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| DEFAULT_LINE_BASE.to_string()),
        }
    }

    pub fn enabled_channels(&self) -> Vec<&'static str> {
        let mut channels = Vec::new();
        if self.email_api_key.is_some() && self.email_from.is_some() {
            channels.push("email");
        }
        if self.line_channel_token.is_some() {
            channels.push("line");
        }
        channels
    }
}

/// Claim up to 50 pending notifications across all tenants and deliver them.
/// Returns the number of rows processed. When no channel is configured, returns
/// 0 without touching the outbox.
///
/// Failed sends stay `pending` and are retried up to 3 attempts; the third
/// failure moves the row to `failed`.
pub async fn deliver_pending(
    pool: &TenantPool,
    auth_pool: &sqlx::PgPool,
    http: &reqwest::Client,
    config: &NotifyConfig,
) -> Result<usize, sqlx::Error> {
    let channels: Vec<String> = config
        .enabled_channels()
        .into_iter()
        .map(|c| c.to_string())
        .collect();
    if channels.is_empty() {
        return Ok(0);
    }

    let tenant_rows = sqlx::query("SELECT id FROM tenants")
        .fetch_all(auth_pool)
        .await?;

    let mut total = 0usize;
    let mut remaining = BATCH_LIMIT;

    for row in tenant_rows {
        if remaining <= 0 {
            break;
        }
        let tenant_id: Uuid = row.get("id");
        let mut tx = pool
            .begin_tenant(talentos_kernel::TenantId(tenant_id))
            .await?;

        let rows = sqlx::query(
            r#"SELECT id, user_id, channel, recipient, subject, body
               FROM notifications
               WHERE tenant_id = $1
                 AND status = 'pending'
                 AND channel = ANY($2)
               ORDER BY created_at ASC
               LIMIT $3
               FOR UPDATE SKIP LOCKED"#,
        )
        .bind(tenant_id)
        .bind(&channels)
        .bind(remaining)
        .fetch_all(&mut *tx)
        .await?;

        let count = rows.len();
        if count == 0 {
            let _ = tx.rollback().await;
            continue;
        }

        for row in rows {
            let id: Uuid = row.try_get("id")?;
            let user_id: Option<Uuid> = row.try_get("user_id")?;
            let channel: String = row.try_get("channel")?;
            let recipient: String = row.try_get("recipient")?;
            let subject: String = row.try_get("subject")?;
            let body: String = row.try_get("body")?;

            let recipient_ok = match user_id {
                Some(uid) => {
                    validate_recipient(&mut tx, tenant_id, uid, &channel, &recipient).await?
                }
                None => true,
            };

            if !recipient_ok {
                mark_invalid(&mut tx, id).await?;
                continue;
            }

            let result = match channel.as_str() {
                "email" => send_email(http, config, &recipient, &subject, &body).await,
                "line" => send_line(http, config, &recipient, &body).await,
                other => Err(format!("unsupported channel: {other}").into()),
            };

            match result {
                Ok(()) => mark_sent(&mut tx, id).await?,
                Err(e) => record_attempt(&mut tx, id, &format!("{e:#}")).await?,
            }
        }

        tx.commit().await?;
        total += count;
        remaining -= count as i64;
    }

    Ok(total)
}

async fn send_email(
    http: &reqwest::Client,
    config: &NotifyConfig,
    to: &str,
    subject: &str,
    text: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let api_key = config
        .email_api_key
        .as_ref()
        .ok_or("email api key missing")?;
    let from = config.email_from.as_ref().ok_or("email from missing")?;
    let url = format!("{}/emails", config.email_api_base.trim_end_matches('/'));

    http.post(url)
        .timeout(REQUEST_TIMEOUT)
        .bearer_auth(api_key)
        .json(&json!({
            "from": from,
            "to": to,
            "subject": subject,
            "text": text,
        }))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn send_line(
    http: &reqwest::Client,
    config: &NotifyConfig,
    to: &str,
    text: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let token = config
        .line_channel_token
        .as_ref()
        .ok_or("line channel token missing")?;
    let url = format!(
        "{}/v2/bot/message/push",
        config.line_api_base.trim_end_matches('/')
    );

    http.post(url)
        .timeout(REQUEST_TIMEOUT)
        .bearer_auth(token)
        .json(&json!({
            "to": to,
            "messages": [{"type": "text", "text": text}],
        }))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn validate_recipient(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
    channel: &str,
    recipient: &str,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "SELECT email, line_user_id, is_active FROM users WHERE id = $1 AND tenant_id = $2",
    )
    .bind(user_id)
    .bind(tenant_id)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(row) = row else { return Ok(false) };
    let is_active: bool = row.try_get("is_active")?;
    if !is_active {
        return Ok(false);
    }

    match channel {
        "email" => {
            let email: String = row.try_get("email")?;
            Ok(email == recipient)
        }
        "line" => {
            let line_id: Option<String> = row.try_get("line_user_id")?;
            Ok(line_id.as_deref() == Some(recipient))
        }
        _ => Ok(false),
    }
}

async fn mark_invalid(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE notifications
           SET status = 'failed', attempts = attempts + 1, last_error = 'recipient no longer valid'
           WHERE id = $1"#,
    )
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn mark_sent(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE notifications
           SET status = 'sent', attempts = attempts + 1, sent_at = now(), last_error = NULL
           WHERE id = $1"#,
    )
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn record_attempt(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    error: &str,
) -> Result<(), sqlx::Error> {
    let truncated: String = error.chars().take(500).collect();
    sqlx::query(
        r#"UPDATE notifications
           SET attempts = attempts + 1,
               last_error = $2,
               status = CASE WHEN attempts + 1 >= $3 THEN 'failed' ELSE 'pending' END
           WHERE id = $1"#,
    )
    .bind(id)
    .bind(truncated)
    .bind(MAX_ATTEMPTS)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::{Json, State},
        http::{HeaderMap, StatusCode},
        routing::post,
        Router,
    };
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use std::sync::{Arc, Mutex};

    static WORKER_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    static TEST_TENANTS: std::sync::Mutex<Vec<Uuid>> = std::sync::Mutex::new(Vec::new());

    fn register_test_tenant(id: Uuid) {
        TEST_TENANTS.lock().unwrap().push(id);
    }

    fn forget_test_tenant(id: Uuid) {
        let mut v = TEST_TENANTS.lock().unwrap();
        v.retain(|&x| x != id);
    }

    async fn cleanup_registered_notifications(admin: &sqlx::PgPool) {
        let ids: Vec<Uuid> = TEST_TENANTS.lock().unwrap().clone();
        for id in ids {
            let _ = sqlx::query("DELETE FROM notifications WHERE tenant_id = $1")
                .bind(id)
                .execute(admin)
                .await;
        }
    }

    #[derive(Debug, Clone, Default)]
    struct Received {
        email: Vec<(HeaderMap, EmailPayload)>,
        line: Vec<(HeaderMap, LinePayload)>,
    }

    #[derive(Debug, Clone, Deserialize, Serialize)]
    struct EmailPayload {
        from: String,
        to: String,
        subject: String,
        text: String,
    }

    #[derive(Debug, Clone, Deserialize, Serialize)]
    struct LinePayload {
        to: String,
        messages: Vec<Value>,
    }

    fn empty_config() -> NotifyConfig {
        NotifyConfig {
            email_api_key: None,
            email_from: None,
            email_api_base: DEFAULT_EMAIL_BASE.to_string(),
            line_channel_token: None,
            line_api_base: DEFAULT_LINE_BASE.to_string(),
        }
    }

    async fn stub_server(state: Arc<Mutex<Received>>) -> (reqwest::Client, std::net::SocketAddr) {
        let app = Router::new()
            .route("/emails", post(email_handler))
            .route("/v2/bot/message/push", post(line_handler))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Wait for the listener to be ready.
        tokio::time::sleep(Duration::from_millis(50)).await;
        (reqwest::Client::new(), addr)
    }

    async fn email_handler(
        State(state): State<Arc<Mutex<Received>>>,
        headers: HeaderMap,
        Json(payload): Json<EmailPayload>,
    ) -> StatusCode {
        state.lock().unwrap().email.push((headers, payload));
        StatusCode::OK
    }

    async fn line_handler(
        State(state): State<Arc<Mutex<Received>>>,
        headers: HeaderMap,
        Json(payload): Json<LinePayload>,
    ) -> StatusCode {
        state.lock().unwrap().line.push((headers, payload));
        StatusCode::OK
    }

    fn db_url() -> Option<String> {
        std::env::var("DATABASE_URL").ok()
    }

    fn admin_db_url() -> Option<String> {
        std::env::var("DATABASE_ADMIN_URL").ok()
    }

    async fn seed_tenant(admin: &sqlx::PgPool, app: &TenantPool) -> (Uuid, Uuid) {
        let tenant_id = Uuid::new_v4();
        register_test_tenant(tenant_id);

        // Insert tenant through the admin connection.
        sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'Notify Test', $2)")
            .bind(tenant_id)
            .bind(format!("notify-{}", &tenant_id.to_string()[..8]))
            .execute(admin)
            .await
            .expect("insert tenant");

        // Insert a user and employee using the tenant-scoped app pool.
        let mut tx = app
            .begin_tenant(talentos_kernel::TenantId(tenant_id))
            .await
            .unwrap();
        let user_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, tenant_id, email, password_hash, is_active, line_user_id)
             VALUES ($1, $2, 'test@example.com', 'x', true, 'U_LINE')",
        )
        .bind(user_id)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .unwrap();

        let entity_id = Uuid::new_v4();
        sqlx::query("INSERT INTO entities (id, tenant_id, name) VALUES ($1, $2, 'Entity')")
            .bind(entity_id)
            .bind(tenant_id)
            .execute(&mut *tx)
            .await
            .unwrap();

        let employee_id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO employees
                 (id, tenant_id, entity_id, user_id, employee_code, first_name_th, last_name_th,
                  hire_date, position_title, base_salary, tax_filing_status, pf_member, status)
               VALUES ($1,$2,$3,$4,'E001','ทดสอบ','พนักงาน','2024-01-01','Dev',30000.00,1,false,'active')"#,
        )
        .bind(employee_id)
        .bind(tenant_id)
        .bind(entity_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .unwrap();

        tx.commit().await.unwrap();
        (tenant_id, user_id)
    }

    async fn cleanup_tenant(admin: &sqlx::PgPool, tenant_id: Uuid) {
        for stmt in [
            "DELETE FROM notifications WHERE tenant_id = $1",
            "DELETE FROM employees WHERE tenant_id = $1",
            "DELETE FROM user_roles WHERE user_id IN (SELECT id FROM users WHERE tenant_id = $1)",
            "DELETE FROM users WHERE tenant_id = $1",
            "DELETE FROM entities WHERE tenant_id = $1",
            "DELETE FROM tenants WHERE id = $1",
        ] {
            let _ = sqlx::query(stmt).bind(tenant_id).execute(admin).await;
        }
        forget_test_tenant(tenant_id);
    }

    async fn insert_notification(
        app: &TenantPool,
        tenant_id: Uuid,
        user_id: Option<Uuid>,
        channel: &str,
        recipient: &str,
        subject: &str,
        body: &str,
    ) -> Uuid {
        let mut tx = app
            .begin_tenant(talentos_kernel::TenantId(tenant_id))
            .await
            .unwrap();
        let id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO notifications
                 (id, tenant_id, user_id, channel, recipient, subject, body, status)
               VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending')"#,
        )
        .bind(id)
        .bind(tenant_id)
        .bind(user_id)
        .bind(channel)
        .bind(recipient)
        .bind(subject)
        .bind(body)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        id
    }

    async fn notification_status(app: &TenantPool, tenant_id: Uuid, id: Uuid) -> Option<String> {
        let mut tx = app
            .begin_tenant(talentos_kernel::TenantId(tenant_id))
            .await
            .unwrap();
        let row = sqlx::query("SELECT status FROM notifications WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();
        let _ = tx.rollback().await;
        row.map(|r| r.get("status"))
    }

    async fn notification_status_and_attempts(
        app: &TenantPool,
        tenant_id: Uuid,
        id: Uuid,
    ) -> Option<(String, i32)> {
        let mut tx = app
            .begin_tenant(talentos_kernel::TenantId(tenant_id))
            .await
            .unwrap();
        let row = sqlx::query("SELECT status, attempts FROM notifications WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();
        let _ = tx.rollback().await;
        row.map(|r| (r.get("status"), r.get("attempts")))
    }

    async fn failing_email_server() -> (reqwest::Client, std::net::SocketAddr) {
        let app = Router::new().route(
            "/emails",
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

    #[test]
    fn no_config_enables_no_channels() {
        let cfg = empty_config();
        assert!(cfg.enabled_channels().is_empty());
    }

    #[test]
    fn email_requires_key_and_from() {
        let mut cfg = empty_config();
        cfg.email_api_key = Some("key".into());
        assert!(cfg.enabled_channels().is_empty());
        cfg.email_from = Some("no-reply@example.com".into());
        assert_eq!(cfg.enabled_channels(), vec!["email"]);
    }

    #[tokio::test]
    async fn empty_config_claims_nothing_and_leaves_rows_pending() {
        let _guard = WORKER_TEST_LOCK.lock().await;
        let Some(url) = db_url() else {
            eprintln!("SKIP: DATABASE_URL not set");
            return;
        };
        let Some(admin_url) = admin_db_url() else {
            eprintln!("SKIP: DATABASE_ADMIN_URL not set");
            return;
        };

        let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
        cleanup_registered_notifications(&admin).await;
        let app = TenantPool::connect(&url, 2).await.unwrap();
        let (tenant_id, _user_id) = seed_tenant(&admin, &app).await;

        let id = insert_notification(
            &app,
            tenant_id,
            None,
            "email",
            "to@example.com",
            "sub",
            "body",
        )
        .await;

        let http = reqwest::Client::new();
        let count = deliver_pending(&app, &admin, &http, &empty_config())
            .await
            .expect("deliver");
        assert_eq!(count, 0);
        assert_eq!(
            notification_status(&app, tenant_id, id).await.as_deref(),
            Some("pending")
        );

        cleanup_tenant(&admin, tenant_id).await;
    }

    #[tokio::test]
    async fn email_path_header_and_body_are_correct() {
        let _guard = WORKER_TEST_LOCK.lock().await;
        let Some(url) = db_url() else {
            eprintln!("SKIP: DATABASE_URL not set");
            return;
        };
        let Some(admin_url) = admin_db_url() else {
            eprintln!("SKIP: DATABASE_ADMIN_URL not set");
            return;
        };

        let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
        cleanup_registered_notifications(&admin).await;
        let app = TenantPool::connect(&url, 2).await.unwrap();
        let (tenant_id, _user_id) = seed_tenant(&admin, &app).await;

        let id = insert_notification(
            &app,
            tenant_id,
            None,
            "email",
            "employee@example.com",
            "หัวข้อทดสอบ",
            "เนื้อหาทดสอบ",
        )
        .await;

        let state = Arc::new(Mutex::new(Received::default()));
        let (http, addr) = stub_server(state.clone()).await;
        let config = NotifyConfig {
            email_api_key: Some("test-key".into()),
            email_from: Some("noreply@example.com".into()),
            email_api_base: format!("http://{addr}"),
            line_channel_token: None,
            line_api_base: DEFAULT_LINE_BASE.to_string(),
        };

        let count = deliver_pending(&app, &admin, &http, &config)
            .await
            .expect("deliver");
        assert_eq!(count, 1);
        assert_eq!(
            notification_status(&app, tenant_id, id).await.as_deref(),
            Some("sent")
        );

        {
            let received = state.lock().unwrap();
            assert_eq!(received.email.len(), 1);
            let (headers, payload) = &received.email[0];
            let auth = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert!(auth.starts_with("Bearer "));
            assert!(auth.contains("test-key"));
            assert_eq!(payload.from, "noreply@example.com");
            assert_eq!(payload.to, "employee@example.com");
            assert_eq!(payload.subject, "หัวข้อทดสอบ");
            assert_eq!(payload.text, "เนื้อหาทดสอบ");
        }

        cleanup_tenant(&admin, tenant_id).await;
    }

    #[tokio::test]
    async fn line_path_header_and_body_are_correct() {
        let _guard = WORKER_TEST_LOCK.lock().await;
        let Some(url) = db_url() else {
            eprintln!("SKIP: DATABASE_URL not set");
            return;
        };
        let Some(admin_url) = admin_db_url() else {
            eprintln!("SKIP: DATABASE_ADMIN_URL not set");
            return;
        };

        let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
        cleanup_registered_notifications(&admin).await;
        let app = TenantPool::connect(&url, 2).await.unwrap();
        let (tenant_id, _user_id) = seed_tenant(&admin, &app).await;

        let id = insert_notification(
            &app,
            tenant_id,
            None,
            "line",
            "U_TEST_EMP",
            "สลิปพร้อม",
            "เนื้อหา LINE",
        )
        .await;

        let state = Arc::new(Mutex::new(Received::default()));
        let (http, addr) = stub_server(state.clone()).await;
        let config = NotifyConfig {
            email_api_key: None,
            email_from: None,
            email_api_base: DEFAULT_EMAIL_BASE.to_string(),
            line_channel_token: Some("line-token".into()),
            line_api_base: format!("http://{addr}"),
        };

        let count = deliver_pending(&app, &admin, &http, &config)
            .await
            .expect("deliver");
        assert_eq!(count, 1);
        assert_eq!(
            notification_status(&app, tenant_id, id).await.as_deref(),
            Some("sent")
        );

        {
            let received = state.lock().unwrap();
            assert_eq!(received.line.len(), 1);
            let (headers, payload) = &received.line[0];
            let auth = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert!(auth.starts_with("Bearer "));
            assert!(auth.contains("line-token"));
            assert_eq!(payload.to, "U_TEST_EMP");
            assert_eq!(
                payload.messages,
                vec![json!({"type": "text", "text": "เนื้อหา LINE"})]
            );
        }

        cleanup_tenant(&admin, tenant_id).await;
    }

    #[tokio::test]
    async fn transient_failure_retries_then_fails_on_third_attempt() {
        let _guard = WORKER_TEST_LOCK.lock().await;
        let Some(url) = db_url() else {
            eprintln!("SKIP: DATABASE_URL not set");
            return;
        };
        let Some(admin_url) = admin_db_url() else {
            eprintln!("SKIP: DATABASE_ADMIN_URL not set");
            return;
        };

        let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
        cleanup_registered_notifications(&admin).await;
        let app = TenantPool::connect(&url, 2).await.unwrap();
        let (tenant_id, _user_id) = seed_tenant(&admin, &app).await;

        let id = insert_notification(
            &app,
            tenant_id,
            None,
            "email",
            "employee@example.com",
            "sub",
            "body",
        )
        .await;

        let (http, addr) = failing_email_server().await;
        let config = NotifyConfig {
            email_api_key: Some("test-key".into()),
            email_from: Some("noreply@example.com".into()),
            email_api_base: format!("http://{addr}"),
            line_channel_token: None,
            line_api_base: DEFAULT_LINE_BASE.to_string(),
        };

        for expected in [("pending", 1), ("pending", 2), ("failed", 3)] {
            let count = deliver_pending(&app, &admin, &http, &config)
                .await
                .expect("deliver");
            assert_eq!(count, 1);
            let (status, attempts) = notification_status_and_attempts(&app, tenant_id, id)
                .await
                .expect("row exists");
            assert_eq!(
                status, expected.0,
                "unexpected status after attempt {}",
                attempts
            );
            assert_eq!(attempts, expected.1);
        }

        cleanup_tenant(&admin, tenant_id).await;
    }

    #[tokio::test]
    async fn stale_email_recipient_is_marked_failed_without_sending() {
        let _guard = WORKER_TEST_LOCK.lock().await;
        let Some(url) = db_url() else {
            eprintln!("SKIP: DATABASE_URL not set");
            return;
        };
        let Some(admin_url) = admin_db_url() else {
            eprintln!("SKIP: DATABASE_ADMIN_URL not set");
            return;
        };

        let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
        cleanup_registered_notifications(&admin).await;
        let app = TenantPool::connect(&url, 2).await.unwrap();
        let (tenant_id, user_id) = seed_tenant(&admin, &app).await;

        let id = insert_notification(
            &app,
            tenant_id,
            Some(user_id),
            "email",
            "test@example.com",
            "sub",
            "body",
        )
        .await;

        // Deactivate the user after the notification was enqueued.
        let mut tx = app
            .begin_tenant(talentos_kernel::TenantId(tenant_id))
            .await
            .unwrap();
        sqlx::query("UPDATE users SET is_active = false WHERE id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let state = Arc::new(Mutex::new(Received::default()));
        let (http, addr) = stub_server(state.clone()).await;
        let config = NotifyConfig {
            email_api_key: Some("test-key".into()),
            email_from: Some("noreply@example.com".into()),
            email_api_base: format!("http://{addr}"),
            line_channel_token: None,
            line_api_base: DEFAULT_LINE_BASE.to_string(),
        };

        let count = deliver_pending(&app, &admin, &http, &config)
            .await
            .expect("deliver");
        assert_eq!(count, 1);
        assert_eq!(
            notification_status(&app, tenant_id, id).await.as_deref(),
            Some("failed")
        );
        assert_eq!(state.lock().unwrap().email.len(), 0);

        cleanup_tenant(&admin, tenant_id).await;
    }
}

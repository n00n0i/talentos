//! Minimal Prometheus metrics: HTTP request counts/latency and uptime.
//!
//! Hand-rolled atomics keep the dependency surface small. Status classes are
//! aggregated (2xx/4xx/5xx) so labels cannot explode with routes or IDs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

pub static METRICS: LazyLock<Metrics> = LazyLock::new(Metrics::new);

/// Start the uptime clock at process start rather than at the first scrape.
pub fn init() {
    LazyLock::force(&METRICS);
}

pub struct Metrics {
    started: Instant,
    total: AtomicU64,
    success: AtomicU64,
    client_error: AtomicU64,
    server_error: AtomicU64,
    other: AtomicU64,
    duration_micros: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            total: AtomicU64::new(0),
            success: AtomicU64::new(0),
            client_error: AtomicU64::new(0),
            server_error: AtomicU64::new(0),
            other: AtomicU64::new(0),
            duration_micros: AtomicU64::new(0),
        }
    }

    pub fn observe(&self, status: StatusCode, elapsed: Duration) {
        self.total.fetch_add(1, Ordering::Relaxed);
        match status.as_u16() {
            200..=299 => self.success.fetch_add(1, Ordering::Relaxed),
            400..=499 => self.client_error.fetch_add(1, Ordering::Relaxed),
            500..=599 => self.server_error.fetch_add(1, Ordering::Relaxed),
            _ => self.other.fetch_add(1, Ordering::Relaxed),
        };
        self.duration_micros
            .fetch_add(elapsed.as_micros() as u64, Ordering::Relaxed);
    }

    pub fn render(&self) -> String {
        let total = self.total.load(Ordering::Relaxed);
        let sum = self.duration_micros.load(Ordering::Relaxed) as f64 / 1_000_000.0;
        let uptime = self.started.elapsed().as_secs();
        format!(
            "# HELP talentos_http_requests_total Total HTTP responses by status class.\n\
             # TYPE talentos_http_requests_total counter\n\
             talentos_http_requests_total{{status=\"2xx\"}} {}\n\
             talentos_http_requests_total{{status=\"4xx\"}} {}\n\
             talentos_http_requests_total{{status=\"5xx\"}} {}\n\
             talentos_http_requests_total{{status=\"other\"}} {}\n\
             # HELP talentos_http_request_duration_seconds HTTP request duration.\n\
             # TYPE talentos_http_request_duration_seconds summary\n\
             talentos_http_request_duration_seconds_sum {sum:.6}\n\
             talentos_http_request_duration_seconds_count {total}\n\
             # HELP talentos_uptime_seconds Gateway process uptime.\n\
             # TYPE talentos_uptime_seconds gauge\n\
             talentos_uptime_seconds {uptime}\n",
            self.success.load(Ordering::Relaxed),
            self.client_error.load(Ordering::Relaxed),
            self.server_error.load(Ordering::Relaxed),
            self.other.load(Ordering::Relaxed),
        )
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Middleware: count every response and its duration.
pub async fn track(req: Request, next: Next) -> Response {
    let started = Instant::now();
    let response = next.run(req).await;
    METRICS.observe(response.status(), started.elapsed());
    response
}

/// GET /metrics — Prometheus scrape endpoint.
///
/// Public when `METRICS_TOKEN` is unset (bind behind the private network or
/// proxy). When set, require `Authorization: Bearer <token>`.
pub async fn endpoint(headers: HeaderMap) -> Response {
    if let Some(response) = unauthorized(&headers) {
        return response;
    }
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
        )],
        METRICS.render(),
    )
        .into_response()
}

/// Check the `METRICS_TOKEN` env; when set and the bearer token does not
/// match, return a 401 response, else `None` (authorized / token unset).
fn unauthorized(headers: &HeaderMap) -> Option<Response> {
    let expected = std::env::var("METRICS_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())?;
    let authorized = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .is_some_and(|(scheme, token)| scheme.eq_ignore_ascii_case("bearer") && token == expected);
    if !authorized {
        Some(
            (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"))],
                "unauthorized\n",
            )
                .into_response(),
        )
    } else {
        None
    }
}

/// GET /business-metrics — tenant-scoped gauges (notification backlog,
/// payroll run status counts, oldest stuck run age).
///
/// Auth: same bearer token as `/metrics` when `METRICS_TOKEN` is set;
/// otherwise public (bind behind the private network or proxy).
pub async fn business_endpoint(
    State(state): State<crate::state::AppState>,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = unauthorized(&headers) {
        return response;
    }
    let body = match business::fetch(&state.auth_pool).await {
        Ok(gauges) => METRICS.render() + &business::render(&gauges),
        Err(e) => {
            tracing::warn!(error = %e, "business metrics query failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error\n").into_response();
        }
    };
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
        )],
        body,
    )
        .into_response()
}

/// Collect business gauge values from the database.
pub mod business {
    use sqlx::Row;

    /// One notification-outbox count keyed by (channel, status).
    #[derive(Debug, Clone)]
    pub struct OutboxGauge {
        pub channel: &'static str,
        pub status: &'static str,
        pub total: i64,
    }

    /// One payroll-runs count keyed by status.
    #[derive(Debug, Clone)]
    pub struct RunStatusGauge {
        pub status: &'static str,
        pub total: i64,
    }

    /// All business gauges collected in a single pass.
    #[derive(Debug, Default, Clone)]
    pub struct Gauges {
        pub outbox: Vec<OutboxGauge>,
        pub runs_by_status: Vec<RunStatusGauge>,
        /// Age in seconds of the oldest stuck (in-progress) run, or 0 if none.
        pub oldest_stuck_age_secs: i64,
    }

    /// Fetch all business gauges from the database.
    ///
    /// Uses the BYPASSRLS `auth_pool` for cross-tenant counts; this is the
    /// same pattern as the notify worker and escalation sweep.
    pub async fn fetch(auth_pool: &sqlx::PgPool) -> Result<Gauges, sqlx::Error> {
        let outbox_rows = sqlx::query(
            "SELECT channel, status, count(*) AS total
               FROM notifications
              GROUP BY channel, status
              ORDER BY channel, status",
        )
        .fetch_all(auth_pool)
        .await?;

        let outbox: Vec<OutboxGauge> = outbox_rows
            .iter()
            .map(|row| {
                let channel: String = row.try_get("channel")?;
                let status: String = row.try_get("status")?;
                let total: i64 = row.try_get("total")?;
                // Channel is a closed set (CHECK constraint on notifications.channel).
                let channel_static: &'static str = if channel == "email" {
                    "email"
                } else if channel == "line" {
                    "line"
                } else {
                    "other"
                };
                // Status is a closed set (CHECK constraint on notifications.status).
                let status_static: &'static str = if status == "pending" {
                    "pending"
                } else if status == "sent" {
                    "sent"
                } else if status == "failed" {
                    "failed"
                } else {
                    "unknown"
                };
                Ok(OutboxGauge {
                    channel: channel_static,
                    status: status_static,
                    total,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;

        let run_rows = sqlx::query(
            "SELECT status, count(*) AS total
               FROM payroll_runs
              GROUP BY status
              ORDER BY status",
        )
        .fetch_all(auth_pool)
        .await?;

        let runs_by_status: Vec<RunStatusGauge> = run_rows
            .iter()
            .map(|row| {
                let status: String = row.try_get("status")?;
                let total: i64 = row.try_get("total")?;
                // Status is a closed set (CHECK constraint on payroll_runs.status);
                // unknown values are labelled "unknown" to keep label cardinality bounded.
                let status_static: &'static str = if status == "draft" {
                    "draft"
                } else if status == "calculating" {
                    "calculating"
                } else if status == "pending_approval" {
                    "pending_approval"
                } else if status == "approved" {
                    "approved"
                } else if status == "rejected" {
                    "rejected"
                } else if status == "disbursing" {
                    "disbursing"
                } else if status == "completed" {
                    "completed"
                } else if status == "failed" {
                    "failed"
                } else {
                    "unknown"
                };
                Ok(RunStatusGauge {
                    status: status_static,
                    total,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;

        // Oldest in-progress run (not in terminal states completed/failed/rejected).
        // `max(...)` returns NULL when no rows match — map to 0 in that case.
        let oldest_stuck_age_secs: i64 = sqlx::query_scalar(
            r#"SELECT coalesce(max(extract(epoch FROM (now() - updated_at))::bigint), 0)
                  FROM payroll_runs
                 WHERE status IN ('calculating', 'pending_approval', 'approved', 'disbursing')"#,
        )
        .fetch_one(auth_pool)
        .await?;

        Ok(Gauges {
            outbox,
            runs_by_status,
            oldest_stuck_age_secs,
        })
    }

    /// Render the business gauges as a Prometheus text block.
    pub fn render(g: &Gauges) -> String {
        let mut out = String::new();

        for gauge in &g.outbox {
            out.push_str(&format!(
                "talentos_notification_outbox{{channel=\"{}\",status=\"{}\"}} {}\n",
                gauge.channel, gauge.status, gauge.total
            ));
        }

        for gauge in &g.runs_by_status {
            out.push_str(&format!(
                "talentos_payroll_runs_by_status{{status=\"{}\"}} {}\n",
                gauge.status, gauge.total
            ));
        }

        out.push_str(&format!(
            "# HELP talentos_payroll_oldest_stuck_run_seconds Age of the oldest in-progress payroll run.\n\
             # TYPE talentos_payroll_oldest_stuck_run_seconds gauge\n\
             talentos_payroll_oldest_stuck_run_seconds {}\n",
            g.oldest_stuck_age_secs
        ));

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_status_classes_and_duration() {
        let metrics = Metrics::new();
        metrics.observe(StatusCode::OK, Duration::from_millis(10));
        metrics.observe(StatusCode::CREATED, Duration::from_millis(20));
        metrics.observe(StatusCode::UNAUTHORIZED, Duration::from_millis(30));
        metrics.observe(StatusCode::INTERNAL_SERVER_ERROR, Duration::from_millis(40));
        metrics.observe(StatusCode::MOVED_PERMANENTLY, Duration::from_millis(0));

        let text = metrics.render();
        assert!(text.contains("talentos_http_requests_total{status=\"2xx\"} 2"));
        assert!(text.contains("talentos_http_requests_total{status=\"4xx\"} 1"));
        assert!(text.contains("talentos_http_requests_total{status=\"5xx\"} 1"));
        assert!(text.contains("talentos_http_requests_total{status=\"other\"} 1"));
        assert!(text.contains("talentos_http_request_duration_seconds_sum 0.100000"));
        assert!(text.contains("talentos_http_request_duration_seconds_count 5"));
        assert!(text.contains("talentos_uptime_seconds"));
    }

    // --- business gauge tests ------------------------------------------------

    /// Cross-tenant gauge queries run through the BYPASSRLS admin role.
    /// Seeds one tenant's worth of rows, fetches gauges, and verifies the
    /// counts are at least the seeded amounts (other test tenants may exist).
    #[tokio::test]
    async fn gauge_queries_return_expected_counts() {
        let url = std::env::var("DATABASE_ADMIN_URL").ok();
        let Some(url) = url else {
            eprintln!("SKIP: DATABASE_ADMIN_URL not set");
            return;
        };
        let admin = sqlx::PgPool::connect(&url).await.expect("admin pool");

        // Seed one tenant with known notification and payroll-run rows.
        let tenant_id = uuid::Uuid::new_v4();
        let user_id = uuid::Uuid::new_v4();
        let entity_id = uuid::Uuid::new_v4();
        let run_id = uuid::Uuid::new_v4();

        sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'Gauge Test', $2)")
            .bind(tenant_id)
            .bind(format!("gauge-{}", &tenant_id.to_string()[..8]))
            .execute(&admin)
            .await
            .expect("insert tenant");
        sqlx::query(
            "INSERT INTO users (id, tenant_id, email, password_hash, is_active)
               VALUES ($1, $2, 'g@example.com', 'x', true)",
        )
        .bind(user_id)
        .bind(tenant_id)
        .execute(&admin)
        .await
        .expect("insert user");
        sqlx::query("INSERT INTO entities (id, tenant_id, name) VALUES ($1, $2, 'E')")
            .bind(entity_id)
            .bind(tenant_id)
            .execute(&admin)
            .await
            .expect("insert entity");
        // One stuck run (calculating, 30 h old) and one completed run.
        sqlx::query(
            r#"INSERT INTO payroll_runs
                 (id, tenant_id, entity_id, period_start, period_end, payment_date,
                  status, created_by, updated_at)
               VALUES ($1, $2, $3, '2026-01-01', '2026-01-31', '2026-02-01',
                       'calculating', $4, now() - interval '30 hours')"#,
        )
        .bind(run_id)
        .bind(tenant_id)
        .bind(entity_id)
        .bind(user_id)
        .execute(&admin)
        .await
        .expect("insert stuck run");
        let run2 = uuid::Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO payroll_runs
                 (id, tenant_id, entity_id, period_start, period_end, payment_date,
                  status, created_by)
               VALUES ($1, $2, $3, '2026-02-01', '2026-02-28', '2026-03-01',
                       'completed', $4)"#,
        )
        .bind(run2)
        .bind(tenant_id)
        .bind(entity_id)
        .bind(user_id)
        .execute(&admin)
        .await
        .expect("insert completed run");
        // 2 pending email + 1 sent line notifications.
        for _ in 0..2 {
            sqlx::query(
                r#"INSERT INTO notifications
                     (id, tenant_id, user_id, channel, recipient, subject, body, status)
                   VALUES (gen_random_uuid(), $1, $2, 'email', 'g@example.com', 's', 'b', 'pending')"#,
            )
            .bind(tenant_id)
            .bind(user_id)
            .execute(&admin)
            .await
            .expect("insert pending notification");
        }
        sqlx::query(
            r#"INSERT INTO notifications
                 (id, tenant_id, user_id, channel, recipient, subject, body, status)
               VALUES (gen_random_uuid(), $1, $2, 'line', 'U_X', 's', 'b', 'sent')"#,
        )
        .bind(tenant_id)
        .bind(user_id)
        .execute(&admin)
        .await
        .expect("insert sent notification");

        // Fetch and verify (absolute minimums: other tenants may contribute).
        let gauges = business::fetch(&admin).await.expect("fetch gauges");

        let pending_email: i64 = gauges
            .outbox
            .iter()
            .filter(|o| o.channel == "email" && o.status == "pending")
            .map(|o| o.total)
            .sum();
        assert!(
            pending_email >= 2,
            "expected at least 2 pending email, got {pending_email}"
        );

        let sent_line: i64 = gauges
            .outbox
            .iter()
            .filter(|o| o.channel == "line" && o.status == "sent")
            .map(|o| o.total)
            .sum();
        assert!(
            sent_line >= 1,
            "expected at least 1 sent line, got {sent_line}"
        );

        let calculating: i64 = gauges
            .runs_by_status
            .iter()
            .filter(|r| r.status == "calculating")
            .map(|r| r.total)
            .sum();
        assert!(
            calculating >= 1,
            "expected at least 1 calculating run, got {calculating}"
        );

        // The stuck run is 30 h old, so the gauge must reflect >= 30 h.
        assert!(
            gauges.oldest_stuck_age_secs >= 30 * 3600 - 60,
            "oldest stuck run age should be ~30h, got {}s",
            gauges.oldest_stuck_age_secs
        );

        let rendered = business::render(&gauges);
        assert!(rendered.contains("talentos_notification_outbox{"));
        assert!(rendered.contains("talentos_payroll_runs_by_status{status=\"calculating\"}"));
        assert!(rendered.contains("talentos_payroll_oldest_stuck_run_seconds"));

        cleanup_gauges(&admin, tenant_id).await;
    }

    async fn cleanup_gauges(admin: &sqlx::PgPool, tenant_id: uuid::Uuid) {
        for stmt in [
            "DELETE FROM notifications WHERE tenant_id = $1",
            "DELETE FROM payslips WHERE tenant_id = $1",
            "DELETE FROM payroll_runs WHERE tenant_id = $1",
            "DELETE FROM user_roles WHERE user_id IN (SELECT id FROM users WHERE tenant_id = $1)",
            "DELETE FROM users WHERE tenant_id = $1",
            "DELETE FROM entities WHERE tenant_id = $1",
            "DELETE FROM tenants WHERE id = $1",
        ] {
            let _ = sqlx::query(stmt).bind(tenant_id).execute(admin).await;
        }
    }

    /// With an empty (or freshly-seeded-tenant-only) DB the oldest stuck run
    /// age is 0 or absent — the query must not error.
    #[tokio::test]
    async fn gauge_queries_handle_no_stuck_runs() {
        let url = std::env::var("DATABASE_ADMIN_URL").ok();
        let Some(url) = url else {
            eprintln!("SKIP: DATABASE_ADMIN_URL not set");
            return;
        };
        let admin = sqlx::PgPool::connect(&url).await.expect("admin pool");

        // Seed a tenant with only terminal-state runs (completed/rejected/failed).
        let tenant_id = uuid::Uuid::new_v4();
        let user_id = uuid::Uuid::new_v4();
        let entity_id = uuid::Uuid::new_v4();
        let run_id = uuid::Uuid::new_v4();

        sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'T', $2)")
            .bind(tenant_id)
            .bind(format!("ga-{}", &tenant_id.to_string()[..8]))
            .execute(&admin)
            .await
            .expect("insert tenant");
        sqlx::query(
            "INSERT INTO users (id, tenant_id, email, password_hash, is_active)
               VALUES ($1, $2, 't@example.com', 'x', true)",
        )
        .bind(user_id)
        .bind(tenant_id)
        .execute(&admin)
        .await
        .expect("insert user");
        sqlx::query("INSERT INTO entities (id, tenant_id, name) VALUES ($1, $2, 'E')")
            .bind(entity_id)
            .bind(tenant_id)
            .execute(&admin)
            .await
            .expect("insert entity");
        sqlx::query(
            r#"INSERT INTO payroll_runs
                 (id, tenant_id, entity_id, period_start, period_end, payment_date,
                  status, created_by)
               VALUES ($1, $2, $3, '2026-01-01', '2026-01-31', '2026-02-01',
                       'completed', $4)"#,
        )
        .bind(run_id)
        .bind(tenant_id)
        .bind(entity_id)
        .bind(user_id)
        .execute(&admin)
        .await
        .expect("insert completed run");

        let gauges = business::fetch(&admin).await.expect("fetch gauges");
        // completed is a terminal status; oldest_stuck may be 0 or reflect
        // other tenants — just verify the field is non-negative.
        assert!(gauges.oldest_stuck_age_secs >= 0);

        cleanup_gauges(&admin, tenant_id).await;
    }

    #[test]
    fn webhook_payload_shape_matches_spec() {
        use crate::metrics_alert::{build_alerts, AlertConfig};
        use business::{OutboxGauge, RunStatusGauge};

        let cfg = AlertConfig {
            webhook_url: Some("http://example.com/webhook".into()),
            pending_notifications_threshold: 100,
            stuck_run_hours_threshold: 24,
        };
        let gauges = business::Gauges {
            outbox: vec![OutboxGauge {
                channel: "email",
                status: "pending",
                total: 250,
            }],
            runs_by_status: vec![RunStatusGauge {
                status: "calculating",
                total: 3,
            }],
            oldest_stuck_age_secs: 30 * 3600,
        };

        let alerts = build_alerts(&gauges, &cfg);
        assert_eq!(alerts.len(), 2, "both alerts should fire");

        let backlog = &alerts[0];
        assert_eq!(backlog["alert"], "talentos_notification_backlog_high");
        assert!(backlog.get("pending").is_some());
        assert!(backlog.get("threshold").is_some());

        let stuck = &alerts[1];
        assert_eq!(stuck["alert"], "talentos_stuck_payroll_run");
        assert!(stuck.get("age_hours").is_some());
        assert!(stuck.get("threshold_hours").is_some());
    }

    #[test]
    fn webhook_payload_shape_single_alert() {
        use crate::metrics_alert::{build_alerts, AlertConfig};
        use business::OutboxGauge;

        let cfg = AlertConfig {
            webhook_url: None,
            pending_notifications_threshold: 100,
            stuck_run_hours_threshold: 24,
        };
        let gauges = business::Gauges {
            outbox: vec![OutboxGauge {
                channel: "line",
                status: "pending",
                total: 500,
            }],
            runs_by_status: vec![],
            oldest_stuck_age_secs: 0,
        };

        let alerts = build_alerts(&gauges, &cfg);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0]["alert"], "talentos_notification_backlog_high");
        assert_eq!(alerts[0]["pending"], serde_json::json!(500));
    }

    #[test]
    fn no_alerts_when_all_clear() {
        use crate::metrics_alert::{build_alerts, AlertConfig};
        use business::OutboxGauge;

        let cfg = AlertConfig {
            webhook_url: None,
            pending_notifications_threshold: 100,
            stuck_run_hours_threshold: 24,
        };
        let gauges = business::Gauges {
            outbox: vec![
                OutboxGauge {
                    channel: "email",
                    status: "pending",
                    total: 10,
                },
                OutboxGauge {
                    channel: "line",
                    status: "sent",
                    total: 1000,
                },
            ],
            runs_by_status: vec![],
            oldest_stuck_age_secs: 5 * 3600,
        };

        let alerts = build_alerts(&gauges, &cfg);
        assert!(alerts.is_empty(), "expected no alerts, got {alerts:?}");
    }
}

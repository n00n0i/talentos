//! Business-metric alert webhooks.
//!
//! Polls the gauge queries every 60 s; when a threshold is crossed, POSTs a
//! JSON payload to `METRICS_ALERT_WEBHOOK` (if set). Failures are logged and
//! the loop continues — the alert path must not kill the gateway.

use std::time::Duration;

use crate::metrics::business::{self, Gauges};
use serde_json::json;

/// Thresholds and webhook URL read once at startup.
#[derive(Debug, Clone)]
pub struct AlertConfig {
    pub webhook_url: Option<String>,
    /// Fire when total pending notifications exceed this value.
    pub pending_notifications_threshold: u64,
    /// Fire when the oldest stuck run exceeds this many hours.
    pub stuck_run_hours_threshold: u64,
}

impl AlertConfig {
    pub fn from_env() -> Self {
        let webhook = std::env::var("METRICS_ALERT_WEBHOOK")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Self {
            webhook_url: webhook,
            pending_notifications_threshold: env_u64("METRICS_ALERT_PENDING_THRESHOLD", 100),
            stuck_run_hours_threshold: env_u64("METRICS_ALERT_STUCK_RUN_HOURS", 24),
        }
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Check gauges against thresholds; returns alert entries that should fire.
pub fn build_alerts(g: &Gauges, cfg: &AlertConfig) -> Vec<serde_json::Value> {
    let mut alerts = Vec::new();

    let pending: i64 = g
        .outbox
        .iter()
        .filter(|o| o.status == "pending")
        .map(|o| o.total)
        .sum();
    if (pending as u64) > cfg.pending_notifications_threshold {
        alerts.push(json!({
            "alert": "talentos_notification_backlog_high",
            "pending": pending,
            "threshold": cfg.pending_notifications_threshold
        }));
    }

    let stuck_secs: i64 = g.oldest_stuck_age_secs;
    let stuck_hours = (stuck_secs / 3600) as u64;
    if stuck_hours > cfg.stuck_run_hours_threshold {
        alerts.push(json!({
            "alert": "talentos_stuck_payroll_run",
            "age_hours": stuck_hours,
            "threshold_hours": cfg.stuck_run_hours_threshold
        }));
    }

    alerts
}

/// POST a single alert entry to the configured webhook. Returns true on 2xx.
pub async fn post_alert(
    http: &reqwest::Client,
    cfg: &AlertConfig,
    payload: &serde_json::Value,
) -> bool {
    // No webhook URL means the sweep is a no-op: report success, post nothing.
    let Some(url) = cfg.webhook_url.as_ref() else {
        return true;
    };
    match http
        .post(url)
        .timeout(Duration::from_secs(10))
        .json(payload)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            if status.is_client_error() || status.is_server_error() {
                tracing::warn!(%status, url, "alert webhook returned error");
                return false;
            }
            tracing::info!(%status, url, "alert webhook ok");
            true
        }
        Err(e) => {
            tracing::warn!(error = %e, url, "alert webhook failed");
            false
        }
    }
}

/// Run one alert sweep: fetch gauges, build alerts, POST each.
/// Returns the number of alerts POSTed.
pub async fn run_sweep(
    auth_pool: &sqlx::PgPool,
    http: &reqwest::Client,
    cfg: &AlertConfig,
) -> Result<usize, sqlx::Error> {
    let gauges = business::fetch(auth_pool).await?;
    let alerts = build_alerts(&gauges, cfg);
    if alerts.is_empty() {
        return Ok(0);
    }
    let mut posted = 0usize;
    for alert in &alerts {
        if post_alert(http, cfg, alert).await {
            posted += 1;
        }
    }
    Ok(posted)
}

/// Spawn the periodic alert worker. Call once from `main()`; the task
/// exits when the gateway shuts down (it is a child task).
pub fn spawn_worker(auth_pool: sqlx::PgPool, http: reqwest::Client, cfg: AlertConfig) {
    tokio::spawn(async move {
        // First sweep 10 s after startup (after the DB has warmed up).
        tokio::time::sleep(Duration::from_secs(10)).await;
        let mut ticker = tokio::time::interval(Duration::from_secs(60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            match run_sweep(&auth_pool, &http, &cfg).await {
                Ok(0) => {}
                Ok(n) => tracing::info!(count = n, "alert sweep posted"),
                Err(e) => tracing::warn!(error = %e, "alert sweep query failed"),
            }
        }
    });
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::business::{OutboxGauge, RunStatusGauge};

    fn gauges(outbox: Vec<OutboxGauge>, runs: Vec<RunStatusGauge>, stuck_secs: i64) -> Gauges {
        Gauges {
            outbox,
            runs_by_status: runs,
            oldest_stuck_age_secs: stuck_secs,
        }
    }

    fn cfg() -> AlertConfig {
        AlertConfig {
            webhook_url: Some("http://localhost:1".into()),
            pending_notifications_threshold: 50,
            stuck_run_hours_threshold: 24,
        }
    }

    #[test]
    fn pending_backlog_fires_above_threshold() {
        let g = gauges(
            vec![OutboxGauge {
                channel: "email",
                status: "pending",
                total: 200,
            }],
            vec![],
            0,
        );
        let alerts = build_alerts(&g, &cfg());
        assert_eq!(alerts.len(), 1);
        let alert = &alerts[0];
        assert_eq!(alert["alert"], "talentos_notification_backlog_high");
        assert_eq!(alert["pending"], json!(200));
        assert_eq!(alert["threshold"], json!(50));
    }

    #[test]
    fn pending_backlog_does_not_fire_at_or_below_threshold() {
        let g = gauges(
            vec![OutboxGauge {
                channel: "email",
                status: "pending",
                total: 50,
            }],
            vec![],
            0,
        );
        let alerts = build_alerts(&g, &cfg());
        assert!(
            alerts.is_empty(),
            "expected no alerts at exactly the threshold"
        );
    }

    #[test]
    fn stuck_run_fires_when_older_than_threshold() {
        // 25 hours of stuck run → 25h > 24h threshold.
        let g = gauges(vec![], vec![], 25 * 3600);
        let alerts = build_alerts(&g, &cfg());
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0]["alert"], "talentos_stuck_payroll_run");
        assert_eq!(alerts[0]["age_hours"], json!(25));
    }

    #[test]
    fn stuck_run_does_not_fire_below_threshold() {
        let g = gauges(vec![], vec![], 20 * 3600);
        let alerts = build_alerts(&g, &cfg());
        assert!(alerts.is_empty());
    }

    #[test]
    fn both_alerts_fire_simultaneously() {
        let g = gauges(
            vec![OutboxGauge {
                channel: "line",
                status: "pending",
                total: 999,
            }],
            vec![],
            100 * 3600,
        );
        let alerts = build_alerts(&g, &cfg());
        assert_eq!(alerts.len(), 2);
        let names: Vec<&str> = alerts
            .iter()
            .map(|a| a["alert"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"talentos_notification_backlog_high"));
        assert!(names.contains(&"talentos_stuck_payroll_run"));
    }

    #[test]
    fn no_alerts_when_all_clear() {
        let g = gauges(
            vec![
                OutboxGauge {
                    channel: "email",
                    status: "pending",
                    total: 10,
                },
                OutboxGauge {
                    channel: "line",
                    status: "sent",
                    total: 100,
                },
            ],
            vec![RunStatusGauge {
                status: "completed",
                total: 5,
            }],
            0,
        );
        assert!(build_alerts(&g, &cfg()).is_empty());
    }

    #[test]
    fn no_webhook_means_post_alert_returns_true() {
        let cfg = AlertConfig {
            webhook_url: None,
            pending_notifications_threshold: 10,
            stuck_run_hours_threshold: 10,
        };
        let http = reqwest::Client::new();
        let payload = json!({"alert": "test"});
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(post_alert(&http, &cfg, &payload));
        assert!(result, "no webhook URL → treat as success (no-op)");
    }
}

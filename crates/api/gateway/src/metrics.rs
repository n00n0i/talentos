//! Minimal Prometheus metrics: HTTP request counts/latency and uptime.
//!
//! Hand-rolled atomics keep the dependency surface small. Status classes are
//! aggregated (2xx/4xx/5xx) so labels cannot explode with routes or IDs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use axum::{
    extract::Request,
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
    let expected = std::env::var("METRICS_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty());
    if let Some(expected) = expected {
        let authorized = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|value| value.split_once(' '))
            .is_some_and(|(scheme, token)| {
                scheme.eq_ignore_ascii_case("bearer") && token == expected
            });
        if !authorized {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"))],
                "unauthorized\n",
            )
                .into_response();
        }
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
}

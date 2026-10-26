use std::time::Instant;

use axum::{extract::Request, middleware::Next, response::Response};
use tower_http::request_id::MakeRequestUuid;

pub use MakeRequestUuid as RequestIdLayer;

/// Structured access log: method, path, status, latency, request_id.
/// Never logs Authorization headers or bodies (spec §4.5).
pub async fn log_request(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string();
    let start = Instant::now();

    let res = next.run(req).await;

    tracing::info!(
        method = %method,
        path = %path,
        status = res.status().as_u16(),
        latency_ms = start.elapsed().as_millis(),
        request_id = %request_id,
        "request"
    );
    res
}
use std::time::Duration;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use fred::prelude::*;

use crate::dto::responses::{HealthChecks, HealthResponse};
use crate::state::AppState;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// GET /health — probes DB and Redis with a hard timeout (spec §5).
pub async fn health(State(state): State<AppState>) -> Response {
    let db_check = tokio::time::timeout(Duration::from_secs(3), state.pool.ping()).await;
    let redis_check =
        tokio::time::timeout(Duration::from_secs(3), state.redis.ping::<String>(None)).await;

    let db_ok = matches!(db_check, Ok(Ok(())));
    // A PING reply (even "PONG") means the socket is live; only timeouts/errors fail.
    let redis_ok = matches!(redis_check, Ok(Ok(_)));

    let healthy = db_ok && redis_ok;
    (
        if healthy {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(HealthResponse {
            status: if healthy { "ok" } else { "degraded" },
            version: VERSION,
            checks: HealthChecks {
                database: if db_ok { "ok" } else { "error" },
                redis: if redis_ok { "ok" } else { "error" },
            },
            uptime_seconds: state.started_at.elapsed().as_secs(),
        }),
    )
        .into_response()
}

//! Team calendar endpoint: holidays, leave and OT grouped by day.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CalendarQuery {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

/// GET /api/v1/calendar?from=YYYY-MM-DD&to=YYYY-MM-DD
pub async fn list(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<CalendarQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "read") {
        return forbidden;
    }

    match talentos_hr_runner::calendar(&state.pool, ctx.tenant_id, query.from, query.to).await {
        Ok(days) => (StatusCode::OK, Json(json!({ "days": days }))).into_response(),
        Err(talentos_hr_runner::CalendarError::BadRange) => {
            bad_request("from must be on or before to")
        }
        Err(talentos_hr_runner::CalendarError::TooWide) => {
            bad_request("date range must be at most 62 days")
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

fn bad_request(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, message)
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "calendar endpoint failed");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "An unexpected error occurred",
    )
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: "calendar_error".into(),
            message: message.into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

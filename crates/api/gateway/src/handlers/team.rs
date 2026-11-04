//! Line-manager team directory (read-only, no payroll/PII fields).

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

/// GET /api/v1/me/team
pub async fn my_team(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_own_team") {
        return forbidden;
    }
    match talentos_hr_runner::team::list_my_team(&state.pool, ctx.tenant_id, ctx.user_id.0).await {
        Ok(members) => Json(json!({ "members": members })).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "team endpoint failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "internal_error".into(),
                    message: "An unexpected error occurred".into(),
                    details: vec![],
                    request_id: Uuid::new_v4().to_string(),
                    retry_after_seconds: None,
                }),
            )
                .into_response()
        }
    }
}

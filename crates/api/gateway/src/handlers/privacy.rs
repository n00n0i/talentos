//! PDPA self-service endpoints: privacy status, notice acknowledgement, and a
//! personal-data export (DSAR). Every route only acts on the authenticated user.

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::Authed;
use crate::state::AppState;
use talentos_hr_runner::privacy::{self, PrivacyError};

/// GET /api/v1/me/privacy
pub async fn status(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    match privacy::privacy_status(&state.pool, ctx.tenant_id, ctx.user_id.0).await {
        Ok(status) => Json(json!({
            "notice_version": status.notice_version,
            "accepted_at": status.accepted_at.map(|t| t.to_rfc3339()),
        }))
        .into_response(),
        Err(e) => map_error(e),
    }
}

#[derive(Debug, Deserialize)]
pub struct ConsentBody {
    pub version: String,
}

/// POST /api/v1/me/privacy/consent
pub async fn accept(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<ConsentBody>,
) -> Response {
    match privacy::accept_privacy(&state.pool, ctx.tenant_id, ctx.user_id.0, &body.version).await {
        Ok(accepted_at) => Json(json!({
            "notice_version": privacy::NOTICE_VERSION,
            "accepted_at": accepted_at.to_rfc3339(),
        }))
        .into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/me/data-export — JSON attachment with the caller's personal data.
pub async fn export(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    let data =
        match privacy::export_personal_data(&state.pool, ctx.tenant_id, ctx.user_id.0, &state.pii)
            .await
        {
            Ok(data) => data,
            Err(e) => return map_error(e),
        };

    let filename = format!(
        "talentos-personal-data-{}.json",
        chrono::Utc::now().format("%Y-%m-%d")
    );
    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                "application/json; charset=utf-8".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        Json(data),
    )
        .into_response()
}

fn map_error(e: PrivacyError) -> Response {
    match e {
        PrivacyError::InvalidVersion => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorResponse {
                error: "invalid_version".into(),
                message: e.to_string(),
                details: vec![],
                request_id: Uuid::new_v4().to_string(),
                retry_after_seconds: None,
            }),
        )
            .into_response(),
        PrivacyError::Db(_) => {
            tracing::error!(error = %e, "privacy endpoint failed");
            internal_err()
        }
    }
}

fn internal_err() -> Response {
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

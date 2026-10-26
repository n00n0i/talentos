//! Leave approval endpoints. Leave request creation stays in the MCP tool for now.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::Authed;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ApproveLeaveRequest {
    pub approve: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LeaveDecisionResponse {
    pub request_id: String,
    pub status: String,
    pub approver_id: String,
}

/// POST /api/v1/leave/requests/:id/decision
pub async fn decide(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<ApproveLeaveRequest>,
) -> Response {
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(e),
    };

    // Only pending requests may be decided; the guard is in SQL so two approvers
    // cannot both "win" a race.
    let updated = sqlx::query_scalar::<_, String>(
        r#"UPDATE leave_requests
           SET status = $3, approver_id = $4, approved_at = now(), updated_at = now()
           WHERE id = $1 AND tenant_id = $2 AND status = 'pending'
           RETURNING status"#,
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .bind(if body.approve { "approved" } else { "rejected" })
    .bind(ctx.user_id.0)
    .fetch_optional(&mut *tx)
    .await;

    let status = match updated {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = tx.rollback().await;
            return not_found();
        }
        Err(e) => return internal_err(e),
    };

    let _ = body.reason;
    if let Err(e) = tx.commit().await {
        return internal_err(e);
    }

    (
        StatusCode::OK,
        Json(LeaveDecisionResponse {
            request_id: id.to_string(),
            status,
            approver_id: ctx.user_id.to_string(),
        }),
    )
        .into_response()
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "not_found".into(),
            message: "Leave request not found or not pending".into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn internal_err(e: sqlx::Error) -> Response {
    tracing::error!(error = %e, "database error");
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

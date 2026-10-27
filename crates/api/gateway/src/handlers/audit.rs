//! Tenant audit trail (read-only). Entries are written by the other handlers.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    pub action: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// GET /api/v1/audit?action=&limit=&offset=
pub async fn list(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<AuditQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "audit_log", "read") {
        return forbidden;
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);
    let action = query.action.unwrap_or_default();

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let rows = sqlx::query(
        r#"SELECT id, actor_type, actor_id, action, resource_type, resource_id,
                  before_state, after_state, created_at
             FROM audit_log
            WHERE tenant_id = $1 AND ($2 = '' OR action = $2)
            ORDER BY created_at DESC
            LIMIT $3 OFFSET $4"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(&action)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    match rows {
        Ok(rows) => {
            use sqlx::Row;
            let entries: Vec<serde_json::Value> = rows
                .iter()
                .map(|row| {
                    json!({
                        "id": row.get::<i64, _>("id"),
                        "actor_type": row.get::<String, _>("actor_type"),
                        "actor_id": row.get::<String, _>("actor_id"),
                        "action": row.get::<String, _>("action"),
                        "resource_type": row.get::<String, _>("resource_type"),
                        "resource_id": row.get::<String, _>("resource_id"),
                        "after_state": row.get::<Option<serde_json::Value>, _>("after_state"),
                        "created_at": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at").to_rfc3339(),
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({ "entries": entries }))).into_response()
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "audit endpoint failed");
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

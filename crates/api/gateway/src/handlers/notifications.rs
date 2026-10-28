//! Notification outbox admin: summary, rows, and retry failed deliveries.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 100;

#[derive(Debug, Deserialize)]
pub struct NotificationQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

/// GET /api/v1/notifications?status=&limit=
pub async fn list(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<NotificationQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "audit_log", "read") {
        return forbidden;
    }

    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let status_filter = query.status.unwrap_or_default();
    if !status_filter.is_empty() && !is_valid_status(&status_filter) {
        return bad_request("status must be one of pending, sent, failed");
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };

    let summary = sqlx::query(
        r#"SELECT status, count(*)::int8 AS cnt
             FROM notifications
            WHERE tenant_id = $1
            GROUP BY status"#,
    )
    .bind(ctx.tenant_id.0)
    .fetch_all(&mut *tx)
    .await;

    let summary = match summary {
        Ok(rows) => rows,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };

    let rows = sqlx::query(
        r#"SELECT id, channel, recipient, subject, status, attempts, last_error,
                  created_at, sent_at
             FROM notifications
            WHERE tenant_id = $1
              AND ($2 = '' OR status = $2)
            ORDER BY created_at DESC
            LIMIT $3"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(&status_filter)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await;

    let rows = match rows {
        Ok(rows) => rows,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };
    let _ = tx.rollback().await;

    let summary = summary_to_json(&summary);
    let rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            json!({
                "id": row.get::<Uuid, _>("id"),
                "channel": row.get::<String, _>("channel"),
                "recipient_masked": mask_recipient(row.get::<String, _>("channel").as_str(), row.get::<String, _>("recipient").as_str()),
                "subject": row.get::<String, _>("subject"),
                "status": row.get::<String, _>("status"),
                "attempts": row.get::<i32, _>("attempts"),
                "last_error": row.get::<Option<String>, _>("last_error"),
                "created_at": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at").to_rfc3339(),
                "sent_at": row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("sent_at").map(|d| d.to_rfc3339()),
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(json!({ "summary": summary, "rows": rows })),
    )
        .into_response()
}

/// POST /api/v1/notifications/:id/retry
pub async fn retry(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "audit_log", "read") {
        return forbidden;
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };

    let current = sqlx::query("SELECT status FROM notifications WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await;

    let current = match current {
        Ok(Some(row)) => row.get::<String, _>("status"),
        Ok(None) => {
            let _ = tx.rollback().await;
            return not_found("notification");
        }
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };

    if current != "failed" {
        let _ = tx.rollback().await;
        return conflict("notification is not failed");
    }

    let done = sqlx::query(
        r#"UPDATE notifications
              SET status = 'pending', attempts = 0, last_error = NULL
            WHERE id = $1 AND tenant_id = $2"#,
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .execute(&mut *tx)
    .await;

    match done {
        Ok(done) if done.rows_affected() == 0 => {
            let _ = tx.rollback().await;
            not_found("notification")
        }
        Ok(_) => {
            if let Err(e) = tx.commit().await {
                return internal_err(&e.to_string());
            }
            (StatusCode::NO_CONTENT, ()).into_response()
        }
        Err(e) => {
            let _ = tx.rollback().await;
            internal_err(&e.to_string())
        }
    }
}

fn summary_to_json(rows: &[sqlx::postgres::PgRow]) -> serde_json::Value {
    let mut pending = 0i64;
    let mut sent = 0i64;
    let mut failed = 0i64;
    for row in rows {
        let status: String = row.get("status");
        let count: i64 = row.get("cnt");
        match status.as_str() {
            "pending" => pending = count,
            "sent" => sent = count,
            "failed" => failed = count,
            _ => {}
        }
    }
    json!({ "pending": pending, "sent": sent, "failed": failed })
}

fn is_valid_status(status: &str) -> bool {
    matches!(status, "pending" | "sent" | "failed")
}

/// Mask a recipient so the UI never shows full addresses or secrets.
/// - email: `a***@domain.com`
/// - line: `****1234` (last four digits/characters)
pub fn mask_recipient(channel: &str, recipient: &str) -> String {
    match channel {
        "email" => mask_email(recipient),
        "line" => mask_line(recipient),
        _ => mask_line(recipient),
    }
}

fn mask_email(email: &str) -> String {
    let Some((local, domain)) = email.split_once('@') else {
        return mask_line(email);
    };
    let first = local.chars().next().unwrap_or('a');
    format!("{}***@{}", first, domain)
}

fn mask_line(value: &str) -> String {
    if value.len() <= 4 {
        return "****".to_string();
    }
    format!("****{}", &value[value.len() - 4..])
}

fn bad_request(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, message)
}

fn not_found(what: &str) -> Response {
    error_response(StatusCode::NOT_FOUND, &format!("{what} not found"))
}

fn conflict(message: &str) -> Response {
    error_response(StatusCode::CONFLICT, message)
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "notifications endpoint failed");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "An unexpected error occurred",
    )
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: "notification_error".into(),
            message: message.into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_email() {
        assert_eq!(
            mask_recipient("email", "admin@example.com"),
            "a***@example.com"
        );
        assert_eq!(mask_recipient("email", "a@b.co"), "a***@b.co");
    }

    #[test]
    fn masks_line() {
        assert_eq!(mask_recipient("line", "U0123456789abcdef"), "****cdef");
        assert_eq!(mask_recipient("line", "U123"), "****");
    }

    #[test]
    fn falls_back_for_unknown_channel() {
        assert_eq!(mask_recipient("push", "U0123456789abcdef"), "****cdef");
    }

    #[test]
    fn validates_status() {
        assert!(is_valid_status("pending"));
        assert!(is_valid_status("sent"));
        assert!(is_valid_status("failed"));
        assert!(!is_valid_status("deleted"));
        assert!(!is_valid_status(""));
    }
}

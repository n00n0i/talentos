//! Employee documents: HR metadata/file management, expiry alerts, and
//! employee self-service reads.

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
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
use talentos_hr_runner::documents::{self, DocumentError, DocumentFile, DocumentInput};

#[derive(Debug, Deserialize)]
pub struct CreateDocumentBody {
    pub doc_type: String,
    pub title: String,
    pub doc_number: Option<String>,
    pub issued_date: Option<NaiveDate>,
    pub expiry_date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
pub struct ExpiringQuery {
    pub days: Option<i64>,
}

/// GET /api/v1/employees/:id/documents
pub async fn list(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match documents::list_documents(&state.pool, ctx.tenant_id, id).await {
        Ok(items) => Json(json!({ "documents": items })).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/employees/:id/documents
pub async fn create(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateDocumentBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    let input = DocumentInput {
        doc_type: body.doc_type,
        title: body.title,
        doc_number: body.doc_number,
        issued_date: body.issued_date,
        expiry_date: body.expiry_date,
    };
    match documents::create_document(&state.pool, ctx.tenant_id, ctx.user_id.0, id, &input).await {
        Ok(doc) => (StatusCode::CREATED, Json(json!({ "document": doc }))).into_response(),
        Err(e) => map_error(e),
    }
}

/// DELETE /api/v1/documents/:id
pub async fn remove(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match documents::delete_document(&state.pool, ctx.tenant_id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/documents/:id/file
pub async fn upload(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    match documents::set_file(&state.pool, ctx.tenant_id, id, content_type, &body).await {
        Ok(()) => Json(json!({ "document_id": id.to_string(), "has_file": true })).into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/documents/:id/file
pub async fn download(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match documents::get_file(&state.pool, ctx.tenant_id, id).await {
        Ok(file) => file_response(file),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/documents/expiring?days=60
pub async fn expiring(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<ExpiringQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match documents::expiring_documents(&state.pool, ctx.tenant_id, query.days.unwrap_or(60)).await
    {
        Ok(items) => Json(json!({ "documents": items })).into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/me/documents
pub async fn me_list(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    match documents::list_my_documents(&state.pool, ctx.tenant_id, ctx.user_id.0).await {
        Ok(items) => Json(json!({ "documents": items })).into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/me/documents/:id/file
pub async fn me_download(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    match documents::get_my_file(&state.pool, ctx.tenant_id, ctx.user_id.0, id).await {
        Ok(file) => file_response(file),
        Err(e) => map_error(e),
    }
}

fn file_response(file: DocumentFile) -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&file.content_type)
                .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
        )],
        file.bytes,
    )
        .into_response()
}

fn map_error(e: DocumentError) -> Response {
    use DocumentError::*;
    match e {
        EmployeeNotFound | NotFound | NoFile => (
            StatusCode::NOT_FOUND,
            Json(envelope("not_found", &e.to_string())),
        )
            .into_response(),
        InvalidType | InvalidTitle | InvalidDates | FileTooLarge | InvalidFileType => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(envelope("invalid_request", &e.to_string())),
        )
            .into_response(),
        Io(_) | Db(_) => {
            tracing::error!(error = %e, "document endpoint failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(envelope("internal_error", "An unexpected error occurred")),
            )
                .into_response()
        }
    }
}

fn envelope(code: &str, message: &str) -> ErrorResponse {
    ErrorResponse {
        error: code.into(),
        message: message.into(),
        details: vec![],
        request_id: Uuid::new_v4().to_string(),
        retry_after_seconds: None,
    }
}

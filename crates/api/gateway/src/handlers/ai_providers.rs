//! Multi-provider AI configuration: provider CRUD, presets, model discovery,
//! and the per-role model assignments the runtime resolves (design 2026-11-03).

use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::middleware::auth::{require, Authed};
use crate::secrets::provider_secret_path;
use crate::state::AppState;

use super::ai::{error_response, internal_err, unavailable};

pub const MODEL_ROLES: [&str; 5] = ["chat", "embedding", "ocr", "vision", "graph_extract"];

/// Design §3. `key_required` is false for providers where the key is optional.
#[derive(Debug, Serialize)]
pub struct ProviderPreset {
    pub kind: &'static str,
    pub label: &'static str,
    pub base_url: Option<&'static str>,
    pub key_required: bool,
}

pub const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        kind: "openai",
        label: "OpenAI",
        base_url: Some("https://api.openai.com/v1"),
        key_required: true,
    },
    ProviderPreset {
        kind: "anthropic-proxy",
        label: "Anthropic (via OpenAI-compatible gateway)",
        base_url: None,
        key_required: true,
    },
    ProviderPreset {
        kind: "google-proxy",
        label: "Google (via OpenAI-compatible gateway)",
        base_url: None,
        key_required: true,
    },
    ProviderPreset {
        kind: "openrouter",
        label: "OpenRouter",
        base_url: Some("https://openrouter.ai/api/v1"),
        key_required: true,
    },
    ProviderPreset {
        kind: "groq",
        label: "Groq",
        base_url: Some("https://api.groq.com/openai/v1"),
        key_required: true,
    },
    ProviderPreset {
        kind: "together",
        label: "Together",
        base_url: Some("https://api.together.xyz/v1"),
        key_required: true,
    },
    ProviderPreset {
        kind: "mistral",
        label: "Mistral",
        base_url: Some("https://api.mistral.ai/v1"),
        key_required: true,
    },
    ProviderPreset {
        kind: "deepseek",
        label: "DeepSeek",
        base_url: Some("https://api.deepseek.com/v1"),
        key_required: true,
    },
    ProviderPreset {
        kind: "xai",
        label: "xAI",
        base_url: Some("https://api.x.ai/v1"),
        key_required: true,
    },
    ProviderPreset {
        kind: "ollama",
        label: "Ollama (local)",
        base_url: Some("http://127.0.0.1:11434/v1"),
        key_required: false,
    },
    ProviderPreset {
        kind: "vllm",
        label: "vLLM",
        base_url: Some("http://127.0.0.1:8000/v1"),
        key_required: false,
    },
    ProviderPreset {
        kind: "custom",
        label: "Custom OpenAI-compatible",
        base_url: None,
        key_required: false,
    },
];

/// GET /api/v1/ai/provider-presets
pub async fn list_presets(Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({ "presets": PRESETS })),
    )
        .into_response()
}

#[derive(Debug, Serialize)]
pub struct Provider {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub has_key: bool,
    pub models_cache: serde_json::Value,
    pub models_fetched_at: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

const PROVIDER_COLUMNS: &str = "id, name, kind, base_url, api_key_ref, models_cache, \
                                models_fetched_at, enabled, created_at";

fn provider_from_row(row: &sqlx::postgres::PgRow) -> Provider {
    Provider {
        id: row.get("id"),
        name: row.get("name"),
        kind: row.get("kind"),
        base_url: row.get("base_url"),
        has_key: row.get::<Option<String>, _>("api_key_ref").is_some(),
        models_cache: row.get("models_cache"),
        models_fetched_at: row.get("models_fetched_at"),
        enabled: row.get("enabled"),
        created_at: row.get("created_at"),
    }
}

/// GET /api/v1/ai/providers — secrets never leave OpenBao; only `has_key`.
pub async fn list_providers(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let rows = sqlx::query(&format!(
        "SELECT {PROVIDER_COLUMNS} FROM ai_providers WHERE tenant_id = $1 ORDER BY name"
    ))
    .bind(ctx.tenant_id.0)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    match rows {
        Ok(rows) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "providers": rows.iter().map(provider_from_row).collect::<Vec<_>>(),
            })),
        )
            .into_response(),
        Err(e) => internal_err(&e.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct ProviderCreate {
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub api_key: Option<String>,
}

fn valid_key(api_key: Option<&str>) -> Option<&str> {
    api_key.map(str::trim).filter(|key| !key.is_empty())
}

/// POST /api/v1/ai/providers — writes the key to OpenBao before the row exists.
pub async fn create_provider(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<ProviderCreate>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    let name = body.name.trim();
    let kind = body.kind.trim();
    let base_url = body.base_url.trim().trim_end_matches('/');
    if name.is_empty() || kind.is_empty() || base_url.is_empty() {
        return bad_request("name, kind and base_url are required");
    }

    let id = Uuid::new_v4();
    let key = valid_key(body.api_key.as_deref());
    let mut written_key = false;
    if let Some(key) = key {
        let Some(client) = &state.secrets else {
            return unavailable("OpenBao is not configured");
        };
        if let Err(e) = client.write_provider_key(ctx.tenant_id.0, id, key).await {
            tracing::warn!(error = %e, "provider key write failed");
            return unavailable("secret backend write failed");
        }
        written_key = true;
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let inserted = sqlx::query(&format!(
        r#"INSERT INTO ai_providers (id, tenant_id, name, kind, base_url, api_key_ref)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING {PROVIDER_COLUMNS}"#
    ))
    .bind(id)
    .bind(ctx.tenant_id.0)
    .bind(name)
    .bind(kind)
    .bind(base_url)
    .bind(key.map(|_| provider_secret_path(ctx.tenant_id.0, id)))
    .fetch_one(&mut *tx)
    .await;
    let provider = match inserted {
        Ok(row) => provider_from_row(&row),
        Err(e) => {
            let _ = tx.rollback().await;
            if written_key {
                if let Some(client) = &state.secrets {
                    let _ = client
                        .delete_provider_key(&provider_secret_path(ctx.tenant_id.0, id))
                        .await;
                }
            }
            if e.as_database_error()
                .is_some_and(|db| db.is_unique_violation())
            {
                return conflict("a provider with this name already exists");
            }
            return internal_err(&e.to_string());
        }
    };
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }
    (StatusCode::CREATED, Json(provider)).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ProviderUpdate {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub base_url: Option<String>,
    pub enabled: Option<bool>,
    /// Write-only; absent or empty leaves the stored key untouched.
    pub api_key: Option<String>,
}

/// PUT /api/v1/ai/providers/:id
pub async fn update_provider(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<ProviderUpdate>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let kind = body
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let base_url = body
        .base_url
        .as_deref()
        .map(str::trim)
        .map(|s| s.trim_end_matches('/'))
        .filter(|s| !s.is_empty());
    let key = valid_key(body.api_key.as_deref());

    if let Some(key) = key {
        let Some(client) = &state.secrets else {
            return unavailable("OpenBao is not configured");
        };
        if let Err(e) = client.write_provider_key(ctx.tenant_id.0, id, key).await {
            tracing::warn!(error = %e, "provider key write failed");
            return unavailable("secret backend write failed");
        }
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let updated = sqlx::query(&format!(
        r#"UPDATE ai_providers SET
               name = COALESCE($3, name),
               kind = COALESCE($4, kind),
               base_url = COALESCE($5, base_url),
               enabled = COALESCE($6, enabled),
               api_key_ref = COALESCE($7, api_key_ref)
             WHERE tenant_id = $1 AND id = $2
             RETURNING {PROVIDER_COLUMNS}"#
    ))
    .bind(ctx.tenant_id.0)
    .bind(id)
    .bind(name)
    .bind(kind)
    .bind(base_url)
    .bind(body.enabled)
    .bind(key.map(|_| provider_secret_path(ctx.tenant_id.0, id)))
    .fetch_optional(&mut *tx)
    .await;
    let updated = match updated {
        Ok(row) => row,
        Err(e) => {
            let _ = tx.rollback().await;
            if e.as_database_error()
                .is_some_and(|db| db.is_unique_violation())
            {
                return conflict("a provider with this name already exists");
            }
            return internal_err(&e.to_string());
        }
    };
    let _ = tx.commit().await;

    match updated {
        Some(row) => (StatusCode::OK, Json(provider_from_row(&row))).into_response(),
        None => not_found("provider not found"),
    }
}

/// DELETE /api/v1/ai/providers/:id — the OpenBao entry goes first so a backend
/// outage never leaves a key without its row (or vice versa).
pub async fn delete_provider(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let existing =
        sqlx::query("SELECT api_key_ref FROM ai_providers WHERE tenant_id = $1 AND id = $2")
            .bind(ctx.tenant_id.0)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await;
    let api_key_ref = match existing {
        Ok(Some(row)) => row.get::<Option<String>, _>("api_key_ref"),
        Ok(None) => {
            let _ = tx.rollback().await;
            return not_found("provider not found");
        }
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };

    if let Some(path) = &api_key_ref {
        let Some(client) = &state.secrets else {
            let _ = tx.rollback().await;
            return unavailable("OpenBao is not configured");
        };
        if let Err(e) = client.delete_provider_key(path).await {
            let _ = tx.rollback().await;
            tracing::warn!(error = %e, "provider key delete failed");
            return unavailable("secret backend delete failed");
        }
    }

    if let Err(e) = sqlx::query("DELETE FROM ai_providers WHERE tenant_id = $1 AND id = $2")
        .bind(ctx.tenant_id.0)
        .bind(id)
        .execute(&mut *tx)
        .await
    {
        let _ = tx.rollback().await;
        return internal_err(&e.to_string());
    }
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub owned_by: String,
    pub created: i64,
}

/// Accepts the OpenAI `{data: [...]}` envelope, a `{models: [...]}` wrapper, or
/// a bare array. Fields other than `id` are optional upstream.
pub fn normalise_models(payload: &serde_json::Value) -> Vec<ModelInfo> {
    let items = payload
        .get("data")
        .and_then(serde_json::Value::as_array)
        .or_else(|| payload.get("models").and_then(serde_json::Value::as_array))
        .or_else(|| payload.as_array());

    let Some(items) = items else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let object = item.as_object()?;
            let id = object.get("id")?.as_str()?.trim();
            if id.is_empty() {
                return None;
            }
            Some(ModelInfo {
                id: id.to_string(),
                owned_by: object
                    .get("owned_by")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                created: object
                    .get("created")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0),
            })
        })
        .collect()
}

/// POST /api/v1/ai/providers/:id/models — fetch and cache the model list.
pub async fn fetch_models(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let provider = sqlx::query(
        "SELECT base_url, api_key_ref FROM ai_providers WHERE tenant_id = $1 AND id = $2",
    )
    .bind(ctx.tenant_id.0)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await;
    let (base_url, api_key_ref) = match provider {
        Ok(Some(row)) => (
            row.get::<String, _>("base_url"),
            row.get::<Option<String>, _>("api_key_ref"),
        ),
        Ok(None) => {
            let _ = tx.rollback().await;
            return not_found("provider not found");
        }
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };
    let _ = tx.rollback().await;

    // A local provider may have no key; in that case send no Authorization header.
    let api_key = if let Some(path) = &api_key_ref {
        let Some(client) = &state.secrets else {
            return unavailable("OpenBao is not configured");
        };
        match client.read_provider_key(path).await {
            Ok(Some(key)) => Some(key),
            Ok(None) => return provider_unreachable("provider key not found"),
            Err(e) => {
                tracing::warn!(error = %e, "provider key read failed");
                return unavailable("secret backend read failed");
            }
        }
    } else {
        None
    };

    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let mut request = state.http.get(&url).timeout(Duration::from_secs(15));
    if let Some(key) = &api_key {
        request = request.bearer_auth(key);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => {
            tracing::warn!(error = %e, "provider model fetch failed");
            return provider_unreachable("could not reach provider");
        }
    };
    let status = response.status();
    if !status.is_success() {
        return provider_unreachable(&format!("provider returned HTTP {}", status.as_u16()));
    }
    let payload: serde_json::Value = match response.json().await {
        Ok(payload) => payload,
        Err(_) => return provider_unreachable("provider returned an invalid response"),
    };
    let models = normalise_models(&payload);
    let cache = serde_json::to_value(&models).unwrap_or_else(|_| serde_json::json!([]));

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    if let Err(e) = sqlx::query(
        r#"UPDATE ai_providers
              SET models_cache = $3, models_fetched_at = now()
            WHERE tenant_id = $1 AND id = $2"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(id)
    .bind(&cache)
    .execute(&mut *tx)
    .await
    {
        let _ = tx.rollback().await;
        return internal_err(&e.to_string());
    }
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({ "models": models })),
    )
        .into_response()
}

#[derive(Debug, Serialize)]
pub struct RoleEntry {
    pub provider_id: Uuid,
    pub model: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Default, Serialize)]
pub struct ModelRolesResponse {
    pub chat: Option<RoleEntry>,
    pub embedding: Option<RoleEntry>,
    pub ocr: Option<RoleEntry>,
    pub vision: Option<RoleEntry>,
    pub graph_extract: Option<RoleEntry>,
}

/// GET /api/v1/ai/model-roles — every role key is present, unset roles are null.
pub async fn get_model_roles(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let rows = sqlx::query(
        "SELECT role, provider_id, model, params FROM ai_model_roles WHERE tenant_id = $1",
    )
    .bind(ctx.tenant_id.0)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    let rows = match rows {
        Ok(rows) => rows,
        Err(e) => return internal_err(&e.to_string()),
    };
    let mut roles = ModelRolesResponse::default();
    for row in &rows {
        let entry = RoleEntry {
            provider_id: row.get("provider_id"),
            model: row.get("model"),
            params: row.get("params"),
        };
        match row.get::<String, _>("role").as_str() {
            "chat" => roles.chat = Some(entry),
            "embedding" => roles.embedding = Some(entry),
            "ocr" => roles.ocr = Some(entry),
            "vision" => roles.vision = Some(entry),
            "graph_extract" => roles.graph_extract = Some(entry),
            _ => {}
        }
    }
    (StatusCode::OK, Json(roles)).into_response()
}

#[derive(Debug, Deserialize)]
pub struct RoleUpdate {
    pub provider_id: Uuid,
    pub model: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

/// PUT /api/v1/ai/model-roles/:role
pub async fn put_model_role(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(role): Path<String>,
    Json(body): Json<RoleUpdate>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    if !MODEL_ROLES.contains(&role.as_str()) {
        return bad_request("unknown model role");
    }
    let model = body.model.trim();
    if model.is_empty() {
        return bad_request("model is required");
    }
    let params = body
        .params
        .filter(|value| !value.is_null())
        .unwrap_or_else(|| serde_json::json!({}));

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let provider = sqlx::query("SELECT 1 FROM ai_providers WHERE tenant_id = $1 AND id = $2")
        .bind(ctx.tenant_id.0)
        .bind(body.provider_id)
        .fetch_optional(&mut *tx)
        .await;
    match provider {
        Ok(Some(_)) => {}
        Ok(None) => {
            let _ = tx.rollback().await;
            return bad_request("provider not found");
        }
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    }

    let saved = sqlx::query(
        r#"INSERT INTO ai_model_roles (tenant_id, role, provider_id, model, params)
           VALUES ($1, $2, $3, $4, $5)
           ON CONFLICT (tenant_id, role) DO UPDATE SET
               provider_id = EXCLUDED.provider_id,
               model = EXCLUDED.model,
               params = EXCLUDED.params,
               updated_at = now()
           RETURNING provider_id, model, params"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(&role)
    .bind(body.provider_id)
    .bind(model)
    .bind(&params)
    .fetch_one(&mut *tx)
    .await;
    let entry = match saved {
        Ok(row) => RoleEntry {
            provider_id: row.get("provider_id"),
            model: row.get("model"),
            params: row.get("params"),
        },
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }
    (
        StatusCode::OK,
        Json(
            serde_json::json!({ "role": role, "provider_id": entry.provider_id,
                                 "model": entry.model, "params": entry.params }),
        ),
    )
        .into_response()
}

fn bad_request(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_request", message)
}

fn not_found(message: &str) -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", message)
}

fn conflict(message: &str) -> Response {
    error_response(StatusCode::CONFLICT, "conflict", message)
}

fn provider_unreachable(message: &str) -> Response {
    error_response(StatusCode::BAD_GATEWAY, "provider_unreachable", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_include_ollama_without_key_requirement() {
        let ollama = PRESETS
            .iter()
            .find(|preset| preset.kind == "ollama")
            .expect("ollama preset");
        assert!(!ollama.key_required);
        assert_eq!(ollama.base_url, Some("http://127.0.0.1:11434/v1"));

        assert!(PRESETS
            .iter()
            .any(|preset| preset.kind == "openai" && preset.key_required));
    }

    #[test]
    fn normalises_openai_model_list() {
        let payload = serde_json::json!({
            "object": "list",
            "data": [
                {"id": "gpt-4o", "object": "model", "created": 1686935002, "owned_by": "openai"},
                {"id": "  ", "owned_by": "ignored"},
                {"id": "bge-m3"},
                {"owned_by": "missing-id"},
            ]
        });
        assert_eq!(
            normalise_models(&payload),
            vec![
                ModelInfo {
                    id: "gpt-4o".into(),
                    owned_by: "openai".into(),
                    created: 1686935002,
                },
                ModelInfo {
                    id: "bge-m3".into(),
                    owned_by: String::new(),
                    created: 0,
                },
            ]
        );
    }

    #[test]
    fn normalises_bare_and_models_arrays() {
        let bare = serde_json::json!([{"id": "llama3"}]);
        assert_eq!(normalise_models(&bare)[0].id, "llama3");

        let wrapped = serde_json::json!({"models": [{"id": "qwen"}]});
        assert_eq!(normalise_models(&wrapped)[0].id, "qwen");

        assert!(normalise_models(&serde_json::json!({"unexpected": true})).is_empty());
    }
}

//! Agent memory on pgvector.
//!
//! Four behavioural layers share one table and one retrieval path, so ranking,
//! expiry and RLS cannot drift between them:
//!
//! | layer | lifetime | holds |
//! |---|---|---|
//! | `working` | 1 h default | current task / conversation context |
//! | `episodic` | durable | what happened: runs, decisions, outcomes |
//! | `semantic` | durable | durable tenant facts, policies, constants |
//! | `procedural` | durable | runbooks and learned procedures |
//!
//! Deliberately not built: a separate graph store and an ontology engine. Graph
//! edges, decisions and ontology concepts are stored as `metadata` on an
//! episodic/semantic row; add a real graph when traversals are the bottleneck.

pub mod tools;

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use talentos_db::TenantPool;
use talentos_kernel::{TenantId, UserId};
use talentos_model_gw::HuakatiClient;
use uuid::Uuid;

const EMBEDDING_DIM: usize = 1536;
const MAX_CONTENT_CHARS: usize = 8000;
const WORKING_TTL_SECS: i64 = 3600;

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("invalid memory: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Working,
    Episodic,
    Semantic,
    Procedural,
}

impl Layer {
    pub const ALL: [Layer; 4] = [
        Layer::Working,
        Layer::Episodic,
        Layer::Semantic,
        Layer::Procedural,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Layer::Working => "working",
            Layer::Episodic => "episodic",
            Layer::Semantic => "semantic",
            Layer::Procedural => "procedural",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.as_str() == value)
    }
}

/// One memory to write.
pub struct NewMemory<'a> {
    pub layer: Layer,
    pub content: &'a str,
    pub importance: f32,
    /// Conversation/task id for `working`; empty for durable layers.
    pub scope_key: &'a str,
    pub metadata: Value,
    /// `None` uses the layer default (1 h only for `working`).
    pub ttl_seconds: Option<i64>,
}

impl<'a> NewMemory<'a> {
    pub fn new(layer: Layer, content: &'a str) -> Self {
        Self {
            layer,
            content,
            importance: 0.5,
            scope_key: "",
            metadata: Value::Object(serde_json::Map::new()),
            ttl_seconds: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHit {
    pub id: Uuid,
    pub layer: Layer,
    pub content: String,
    pub score: f64,
    pub importance: f64,
    pub scope_key: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerCount {
    pub layer: String,
    pub count: i64,
}

pub struct MemoryStore {
    pool: Arc<TenantPool>,
    model_gw: Option<Arc<HuakatiClient>>,
}

impl MemoryStore {
    pub fn new(pool: Arc<TenantPool>, model_gw: Option<Arc<HuakatiClient>>) -> Self {
        Self { pool, model_gw }
    }

    /// Write one memory. Embedding is best-effort: a gateway outage degrades
    /// recall to keyword + recency, it never loses the memory.
    pub async fn remember(
        &self,
        tenant_id: TenantId,
        user_id: Option<UserId>,
        memory: NewMemory<'_>,
    ) -> Result<Uuid, MemoryError> {
        let content = memory.content.trim();
        let chars = content.chars().count();
        if chars == 0 || chars > MAX_CONTENT_CHARS {
            return Err(MemoryError::Invalid(format!(
                "content must be 1..={MAX_CONTENT_CHARS} characters, got {chars}"
            )));
        }

        let importance = if memory.importance.is_finite() {
            memory.importance.clamp(0.0, 1.0)
        } else {
            0.5
        };

        let embedding = match &self.model_gw {
            Some(gw) if !content.is_empty() => gw
                .embed(&tenant_id.to_string(), content)
                .await
                .ok()
                .filter(|v| v.len() == EMBEDDING_DIM),
            _ => None,
        };
        let vector = embedding.as_ref().map(|v| to_pgvector(v));

        let expires_at = effective_ttl(memory.layer, memory.ttl_seconds)
            .map(|s| Utc::now() + Duration::seconds(s));

        let mut tx = self.pool.begin_tenant(tenant_id).await?;
        let row = sqlx::query(
            r#"INSERT INTO ai_memory
                 (tenant_id, user_id, layer, scope_key, content, metadata, importance,
                  embedding, expires_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7::numeric, $8::vector, $9)
               RETURNING id"#,
        )
        .bind(tenant_id.0)
        .bind(user_id.map(|u| u.0))
        .bind(memory.layer.as_str())
        .bind(memory.scope_key)
        .bind(content)
        .bind(memory.metadata)
        .bind(Decimal::from_f32_retain(importance).unwrap_or(Decimal::new(50, 2)))
        .bind(vector)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;

        use sqlx::Row;
        Ok(row.get("id"))
    }

    /// Hybrid recall: vector similarity (when an embedding exists for both sides),
    /// keyword match, importance and recency. Expired rows never surface.
    pub async fn recall(
        &self,
        tenant_id: TenantId,
        query: &str,
        layers: &[Layer],
        limit: u32,
    ) -> Result<Vec<MemoryHit>, MemoryError> {
        let query = query.trim();
        let limit = limit.clamp(1, 100) as i64;
        let layers: Vec<String> = layers.iter().map(|l| l.as_str().to_string()).collect();
        if layers.is_empty() {
            return Ok(Vec::new());
        }

        let embedding = match &self.model_gw {
            Some(gw) if !query.is_empty() => gw
                .embed(&tenant_id.to_string(), query)
                .await
                .ok()
                .filter(|v| v.len() == EMBEDDING_DIM),
            _ => None,
        };
        let vector = embedding.as_ref().map(|v| to_pgvector(v));

        let mut tx = self.pool.begin_tenant(tenant_id).await?;
        let rows = sqlx::query(
            r#"SELECT id, layer, content, scope_key, metadata, importance,
                      created_at,
                      (
                        0.50 * CASE
                            WHEN $3::text IS NOT NULL AND embedding IS NOT NULL
                            THEN 1 - (embedding <=> $3::vector)
                            ELSE 0
                        END
                      + 0.25 * ts_rank(tsv, plainto_tsquery('simple', $2))
                      + 0.15 * importance::float8
                      + 0.10 * exp(-EXTRACT(EPOCH FROM (now() - created_at)) / 2592000.0)
                      ) AS score
               FROM ai_memory
               WHERE tenant_id = $1
                 AND layer = ANY($4)
                 AND (expires_at IS NULL OR expires_at > now())
               ORDER BY score DESC
               LIMIT $5"#,
        )
        .bind(tenant_id.0)
        .bind(query)
        .bind(vector)
        .bind(&layers)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;

        use sqlx::Row;
        let hits: Vec<MemoryHit> = rows
            .into_iter()
            .map(|row| {
                let importance: Decimal = row.get("importance");
                MemoryHit {
                    id: row.get("id"),
                    layer: Layer::parse(row.get::<String, _>("layer").as_str())
                        .unwrap_or(Layer::Episodic),
                    content: row.get("content"),
                    score: row.get::<f64, _>("score"),
                    importance: importance.to_f64().unwrap_or(0.5),
                    scope_key: row.get("scope_key"),
                    metadata: row.get("metadata"),
                    created_at: row.get("created_at"),
                }
            })
            .collect();

        if !hits.is_empty() {
            let ids: Vec<Uuid> = hits.iter().map(|h| h.id).collect();
            sqlx::query(
                "UPDATE ai_memory
                    SET access_count = access_count + 1, last_accessed = now()
                  WHERE tenant_id = $1 AND id = ANY($2)",
            )
            .bind(tenant_id.0)
            .bind(&ids)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(hits)
    }

    /// Delete expired rows for one tenant. Safe to call on a schedule.
    pub async fn forget_expired(&self, tenant_id: TenantId) -> Result<u64, MemoryError> {
        let mut tx = self.pool.begin_tenant(tenant_id).await?;
        let result = sqlx::query(
            "DELETE FROM ai_memory
              WHERE tenant_id = $1 AND expires_at IS NOT NULL AND expires_at <= now()",
        )
        .bind(tenant_id.0)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result.rows_affected())
    }

    /// Test-only handle for seeding expiry timestamps.
    #[doc(hidden)]
    pub fn pool_for_test(&self) -> &sqlx::PgPool {
        self.pool.pool()
    }

    /// Live row counts per layer, for the dashboard/health view.
    pub async fn stats(&self, tenant_id: TenantId) -> Result<Vec<LayerCount>, MemoryError> {
        let mut tx = self.pool.begin_tenant(tenant_id).await?;
        let rows = sqlx::query(
            "SELECT layer, count(*) AS count
               FROM ai_memory
              WHERE tenant_id = $1 AND (expires_at IS NULL OR expires_at > now())
              GROUP BY layer
              ORDER BY layer",
        )
        .bind(tenant_id.0)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;

        use sqlx::Row;
        Ok(rows
            .into_iter()
            .map(|row| LayerCount {
                layer: row.get("layer"),
                count: row.get("count"),
            })
            .collect())
    }
}

/// `working` memory expires by default; durable layers only when asked.
fn effective_ttl(layer: Layer, ttl_seconds: Option<i64>) -> Option<i64> {
    ttl_seconds
        .or_else(|| (layer == Layer::Working).then_some(WORKING_TTL_SECS))
        .map(|s| s.clamp(60, 30 * 24 * 3600))
}

fn to_pgvector(vector: &[f32]) -> String {
    let body = vector
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("[{body}]")
}

/// Shape returned by the HTTP/tool surfaces so clients do not invent their own.
pub fn hits_json(hits: &[MemoryHit]) -> Value {
    json!({ "memories": hits })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_round_trip_and_reject_unknown_names() {
        for layer in Layer::ALL {
            assert_eq!(Layer::parse(layer.as_str()), Some(layer));
        }
        assert_eq!(Layer::parse("graph"), None);
    }

    #[test]
    fn working_memory_expires_by_default_but_durable_layers_do_not() {
        assert_eq!(effective_ttl(Layer::Working, None), Some(3600));
        assert_eq!(effective_ttl(Layer::Working, Some(120)), Some(120));
        assert_eq!(effective_ttl(Layer::Semantic, None), None);
        // Clamped so a caller cannot pin a row for a decade by accident.
        assert_eq!(
            effective_ttl(Layer::Episodic, Some(999_999_999)),
            Some(30 * 24 * 3600)
        );
    }

    #[test]
    fn pgvector_literal_is_a_bracket_list() {
        assert_eq!(to_pgvector(&[1.0, 0.5]), "[1,0.5]");
    }
}

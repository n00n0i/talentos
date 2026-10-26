use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use talentos_kernel::shared::{Citation, DocReference};

/// Document in the RAG store (`ai_documents`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub entity_id: Option<Uuid>,
    pub content_hash: String,
    pub doc_type: String,
    pub source_path: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A text chunk ready for embedding (pre-persistence).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub document_id: Uuid,
    pub chunk_index: i32,
    pub content: String,
    pub word_count: i32,
}

/// Persisted chunk with optional embedding (`ai_document_chunks`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentChunk {
    pub id: i64,
    pub document_id: Uuid,
    pub chunk_index: i32,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub word_count: i32,
    pub created_at: DateTime<Utc>,
    pub source_path: Option<String>,
    pub vector_score: Option<f32>,
    pub keyword_score: Option<f32>,
}

/// Chunk with fused ranking scores.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedChunk {
    pub chunk: DocumentChunk,
    pub vector_score: f32,
    pub keyword_score: f32,
    pub rrf_score: f32,
}

/// Filters applied to both retrieval arms.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchFilters {
    pub doc_type: Option<String>,
    pub entity_id: Option<Uuid>,
    pub date_from: Option<chrono::NaiveDate>,
    pub date_to: Option<chrono::NaiveDate>,
}

/// Result of a hybrid search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchResult {
    pub chunks: Vec<RankedChunk>,
    pub query: String,
    pub filters: SearchFilters,
    pub total_hits: usize,
}

/// Recursive-split chunking configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingConfig {
    pub chunk_size: usize,
    pub overlap: usize,
    pub separator: String,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            chunk_size: 1000,
            overlap: 200,
            separator: "\n\n".to_string(),
        }
    }
}

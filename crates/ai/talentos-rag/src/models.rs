use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Document in the RAG store
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub entity_id: Option<Uuid>,
    pub content_hash: String,
    pub doc_type: DocType,
    pub source_path: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Document type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DocType {
    Policy,
    Regulation,
    Payslip,
    ApiSpec,
    Handbook,
}

/// Document chunk with embedding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentChunk {
    pub id: i64,
    pub document_id: Uuid,
    pub chunk_index: i32,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub word_count: i32,
    pub created_at: DateTime<Utc>,
}

/// Chunk with search scores
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedChunk {
    pub chunk: DocumentChunk,
    pub vector_score: f32,
    pub keyword_score: f32,
    pub rrf_score: f32,
}

/// Search filters
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchFilters {
    pub doc_type: Option<DocType>,
    pub entity_id: Option<Uuid>,
    pub date_from: Option<chrono::NaiveDate>,
    pub date_to: Option<chrono::NaiveDate>,
}

/// Hybrid search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchResult {
    pub chunks: Vec<RankedChunk>,
    pub query: String,
    pub filters: SearchFilters,
    pub total_hits: usize,
}

/// Citation for LLM context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub chunk_text: String,
    pub source_doc: DocReference,
    pub similarity: f32,
    pub chunk_index: usize,
}

/// Document reference for citations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocReference {
    pub tenant_id: Uuid,
    pub doc_id: Uuid,
    pub source_path: String,
}

/// Chunking configuration
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
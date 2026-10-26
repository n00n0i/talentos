use thiserror::Error;

/// Errors that can occur in the RAG pipeline.
#[derive(Error, Debug)]
pub enum RAGError {
    #[error("database error: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("model gateway error: {0}")]
    ModelGatewayError(#[from] talentos_model_gw::ModelError),

    #[error("document not found: {0}")]
    DocumentNotFound(String),

    #[error("invalid document format: {0}")]
    InvalidFormat(String),

    #[error("embedding generation failed: {0}")]
    EmbeddingFailed(String),

    #[error("search failed: {0}")]
    SearchFailed(String),

    #[error("chunking failed: {0}")]
    ChunkingFailed(String),

    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("UUID parse error: {0}")]
    UuidError(#[from] uuid::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

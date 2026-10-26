use thiserror::Error;

/// Errors that can occur in the RAG pipeline
#[derive(Error, Debug)]
pub enum RAGError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("Model gateway error: {0}")]
    ModelGatewayError(#[from] crate::error::ModelError),

    #[error("Document not found: {0}")]
    DocumentNotFound(String),

    #[error("Invalid document format: {0}")]
    InvalidFormat(String),

    #[error("Embedding generation failed: {0}")]
    EmbeddingFailed(String),

    #[error("Search failed: {0}")]
    SearchFailed(String),

    #[error("Chunking failed: {0}")]
    ChunkingFailed(String),

    #[error("PDF extraction failed: {0}")]
    PdfExtractionFailed(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("UUID parse error: {0}")]
    UuidError(#[from] uuid::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}
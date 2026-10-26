//! RAG Pipeline - document ingestion, chunking, embedding, and hybrid search with pgvector.

pub mod chunker;
pub mod embedder;
pub mod error;
pub mod models;
pub mod search;

pub use chunker::Chunker;
pub use embedder::Embedder;
pub use error::RAGError;
pub use models::*;
pub use search::HybridSearch;

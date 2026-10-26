//! RAG Pipeline - document ingestion, chunking, embedding, and hybrid search with pgvector.

pub mod chunker;
pub mod embedder;
pub mod search;
pub mod error;
pub mod models;

pub use chunker::{Chunker, ChunkingConfig, Chunk};
pub use embedder::Embedder;
pub use search::{HybridSearch, SearchFilters, RankedChunk, HybridSearchResult};
pub use error::RAGError;
pub use models::*;
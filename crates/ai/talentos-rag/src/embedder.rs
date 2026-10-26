//! Embeds document chunks and persists them to pgvector.

use sqlx::{PgPool, Row};
use talentos_model_gw::HuakatiClient;
use tracing::{debug, warn};

use crate::error::RAGError;
use crate::models::{Chunk, DocumentChunk};

/// Generates embeddings via the model gateway, then stores chunks.
pub struct Embedder {
    model_gw: HuakatiClient,
    pool: PgPool,
    batch_size: usize,
}

impl Embedder {
    pub fn new(model_gw: HuakatiClient, pool: PgPool) -> Self {
        Self {
            model_gw,
            pool,
            batch_size: 32,
        }
    }

    /// Embed chunks in batches. Failures are logged and skipped so one bad
    /// chunk does not abort a whole document ingest.
    pub async fn embed_chunks(
        &self,
        tenant_id: &str,
        chunks: &mut [DocumentChunk],
    ) -> Result<usize, RAGError> {
        let mut embedded = 0;
        for batch in chunks.chunks_mut(self.batch_size) {
            for chunk in batch.iter_mut() {
                match self.model_gw.embed(tenant_id, &chunk.content).await {
                    Ok(vector) => {
                        chunk.embedding = Some(vector);
                        embedded += 1;
                    }
                    Err(e) => warn!(chunk_id = chunk.id, error = %e, "embedding failed"),
                }
            }
        }
        Ok(embedded)
    }

    /// Persist chunks (with embeddings) as rows in `ai_document_chunks`.
    pub async fn store_chunks(&self, chunks: &[DocumentChunk]) -> Result<(), RAGError> {
        let mut tx = self.pool.begin().await?;
        for chunk in chunks {
            let Some(embedding) = &chunk.embedding else {
                continue;
            };
            // pgvector accepts the textual `[a,b,c]` literal.
            let vector = format!(
                "[{}]",
                embedding
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            sqlx::query(
                r#"
                INSERT INTO ai_document_chunks (document_id, chunk_index, content, embedding, word_count)
                VALUES ($1, $2, $3, $4::vector, $5)
                ON CONFLICT (document_id, chunk_index) DO UPDATE SET
                    content = EXCLUDED.content,
                    embedding = EXCLUDED.embedding,
                    word_count = EXCLUDED.word_count
                "#,
            )
            .bind(chunk.document_id)
            .bind(chunk.chunk_index)
            .bind(&chunk.content)
            .bind(vector)
            .bind(chunk.word_count)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        debug!(count = chunks.len(), "stored chunks");
        Ok(())
    }

    /// Insert a document row if its content hash is new; returns its id.
    pub async fn upsert_document(
        &self,
        tenant_id: uuid::Uuid,
        entity_id: Option<uuid::Uuid>,
        content_hash: &str,
        doc_type: &str,
        source_path: &str,
        metadata: &serde_json::Value,
    ) -> Result<uuid::Uuid, RAGError> {
        let row = sqlx::query(
            r#"
            INSERT INTO ai_documents (tenant_id, entity_id, content_hash, doc_type, source_path, metadata)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (tenant_id, content_hash) DO UPDATE SET updated_at = now()
            RETURNING id
            "#,
        )
        .bind(tenant_id)
        .bind(entity_id)
        .bind(content_hash)
        .bind(doc_type)
        .bind(source_path)
        .bind(metadata)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.try_get("id")?)
    }

    /// Chunk → embed → store, returning how many chunks were embedded.
    pub async fn ingest(
        &self,
        tenant_id: &str,
        document_id: uuid::Uuid,
        chunks: &[Chunk],
    ) -> Result<usize, RAGError> {
        let mut rows: Vec<DocumentChunk> = chunks
            .iter()
            .map(|c| DocumentChunk {
                id: 0,
                document_id,
                chunk_index: c.chunk_index,
                content: c.content.clone(),
                embedding: None,
                word_count: c.word_count,
                created_at: chrono::Utc::now(),
                source_path: None,
                vector_score: None,
                keyword_score: None,
            })
            .collect();
        let count = self.embed_chunks(tenant_id, &mut rows).await?;
        self.store_chunks(&rows).await?;
        Ok(count)
    }
}

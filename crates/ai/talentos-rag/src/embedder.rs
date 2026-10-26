use crate::models::DocumentChunk;
use sqlx::PgPool;
use talents_model_gw::HuakatiClient;
use tracing::{debug, warn};

/// Embedder - generates embeddings for document chunks using ModelGateway
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
            batch_size: 32, // huakati limit ~50 concurrent
        }
    }

    /// Embed a batch of chunks and store in database
    pub async fn embed_chunks(&self, tenant_id: &str, chunks: &mut [DocumentChunk]) -> Result<usize, crate::error::RAGError> {
        let mut embedded_count = 0;

        for chunk_batch in chunks.chunks_mut(self.batch_size) {
            // Prepare texts for embedding
            let texts: Vec<&str> = chunk_batch.iter().map(|c| c.content.as_str()).collect();

            // Call embedding API
            // Note: huakati API expects single text per call for embeddings
            // We batch by making concurrent calls
            let mut handles = Vec::new();
            for text in texts {
                let client = self.model_gw.clone();
                let tenant = tenant_id.to_string();
                let text = text.to_string();
                handles.push(tokio::spawn(async move {
                    client.embed(&tenant, &text).await
                }));
            }

            // Collect results
            for (i, handle) in handles.into_iter().enumerate() {
                match handle.await {
                    Ok(Ok(embedding)) => {
                        if i < chunk_batch.len() {
                            chunk_batch[i].embedding = Some(embedding);
                            embedded_count += 1;
                        }
                    }
                    Ok(Err(e)) => {
                        warn!("Embedding failed for chunk {}: {}", i, e);
                        // Continue with other chunks
                    }
                    Err(e) => {
                        warn!("Task join error: {}", e);
                    }
                }
            }
        }

        Ok(embedded_count)
    }

    /// Store chunks with embeddings in database
    pub async fn store_chunks(&self, chunks: &[DocumentChunk]) -> Result<(), crate::error::RAGError> {
        if chunks.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        for chunk in chunks {
            if let Some(embedding) = &chunk.embedding {
                // Convert to pgvector format
                let vector_str = format!("[{}]", embedding.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(","));

                sqlx::query!(
                    r#"
                    INSERT INTO ai_document_chunks (document_id, chunk_index, content, embedding, word_count)
                    VALUES ($1, $2, $3, $4::vector, $5)
                    ON CONFLICT (document_id, chunk_index) DO UPDATE SET
                        content = EXCLUDED.content,
                        embedding = EXCLUDED.embedding,
                        word_count = EXCLUDED.word_count
                    "#,
                    chunk.document_id,
                    chunk.chunk_index,
                    chunk.content,
                    vector_str,
                    chunk.word_count
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        debug!("Stored {} chunks with embeddings", chunks.len());
        Ok(())
    }

    /// Embed and store in one call
    pub async fn embed_and_store(&self, tenant_id: &str, chunks: &mut [DocumentChunk]) -> Result<usize, crate::error::RAGError> {
        let count = self.embed_chunks(tenant_id, chunks).await?;
        self.store_chunks(chunks).await?;
        Ok(count)
    }
}
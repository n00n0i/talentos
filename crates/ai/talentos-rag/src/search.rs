//! Hybrid retrieval: pgvector cosine similarity + Postgres full-text, fused with RRF.

use sqlx::{PgPool, Row};
use talentos_model_gw::HuakatiClient;
use uuid::Uuid;

use crate::error::RAGError;
use crate::models::{DocumentChunk, HybridSearchResult, RankedChunk, SearchFilters};

/// Combines a dense (embedding) arm and a sparse (tsvector) arm.
pub struct HybridSearch {
    model_gw: HuakatiClient,
    pool: PgPool,
    rrf_k: f32,
}

impl HybridSearch {
    pub fn new(model_gw: HuakatiClient, pool: PgPool) -> Self {
        Self {
            model_gw,
            pool,
            rrf_k: 60.0,
        }
    }

    /// Run both arms and fuse the rankings.
    pub async fn search(
        &self,
        tenant_id: &str,
        query: &str,
        filters: &SearchFilters,
        top_k: usize,
    ) -> Result<HybridSearchResult, RAGError> {
        let embedding = self.model_gw.embed(tenant_id, query).await?;
        let vector_hits = self
            .vector_search(tenant_id, &embedding, filters, top_k * 2)
            .await?;
        let keyword_hits = self
            .keyword_search(tenant_id, query, filters, top_k * 2)
            .await?;
        let fused = self.reciprocal_rank_fusion(&vector_hits, &keyword_hits, top_k);

        let total_hits = fused.len();
        Ok(HybridSearchResult {
            chunks: fused,
            query: query.to_string(),
            filters: filters.clone(),
            total_hits,
        })
    }

    async fn vector_search(
        &self,
        tenant_id: &str,
        embedding: &[f32],
        filters: &SearchFilters,
        limit: usize,
    ) -> Result<Vec<DocumentChunk>, RAGError> {
        let tenant = Uuid::parse_str(tenant_id)?;
        let vector = format!(
            "[{}]",
            embedding
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );

        let rows = sqlx::query(
            r#"
            SELECT dc.id, dc.document_id, dc.chunk_index, dc.content, dc.word_count, dc.created_at,
                   ad.source_path,
                   1 - (dc.embedding <=> $2::vector) AS similarity
            FROM ai_document_chunks dc
            JOIN ai_documents ad ON ad.id = dc.document_id
            WHERE ad.tenant_id = $1
              AND dc.embedding IS NOT NULL
              AND ($3::text IS NULL OR ad.doc_type = $3)
              AND ($4::uuid IS NULL OR ad.entity_id = $4)
            ORDER BY dc.embedding <=> $2::vector
            LIMIT $5
            "#,
        )
        .bind(tenant)
        .bind(vector)
        .bind(filters.doc_type.as_deref())
        .bind(filters.entity_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(DocumentChunk {
                    id: row.try_get("id")?,
                    document_id: row.try_get("document_id")?,
                    chunk_index: row.try_get("chunk_index")?,
                    content: row.try_get("content")?,
                    embedding: None,
                    word_count: row.try_get("word_count")?,
                    created_at: row.try_get("created_at")?,
                    source_path: row.try_get("source_path").ok(),
                    vector_score: row.try_get::<f32, _>("similarity").ok(),
                    keyword_score: None,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(Into::into)
    }

    async fn keyword_search(
        &self,
        tenant_id: &str,
        query: &str,
        filters: &SearchFilters,
        limit: usize,
    ) -> Result<Vec<DocumentChunk>, RAGError> {
        let tenant = Uuid::parse_str(tenant_id)?;

        // shortcut: 'simple' config — no Thai stemming until a tokenizer extension is installed
        let rows = sqlx::query(
            r#"
            SELECT dc.id, dc.document_id, dc.chunk_index, dc.content, dc.word_count, dc.created_at,
                   ad.source_path,
                   ts_rank_cd(dc.tsv, plainto_tsquery('simple', $2)) AS rank
            FROM ai_document_chunks dc
            JOIN ai_documents ad ON ad.id = dc.document_id
            WHERE ad.tenant_id = $1
              AND dc.tsv @@ plainto_tsquery('simple', $2)
              AND ($3::text IS NULL OR ad.doc_type = $3)
              AND ($4::uuid IS NULL OR ad.entity_id = $4)
            ORDER BY rank DESC
            LIMIT $5
            "#,
        )
        .bind(tenant)
        .bind(query)
        .bind(filters.doc_type.as_deref())
        .bind(filters.entity_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(DocumentChunk {
                    id: row.try_get("id")?,
                    document_id: row.try_get("document_id")?,
                    chunk_index: row.try_get("chunk_index")?,
                    content: row.try_get("content")?,
                    embedding: None,
                    word_count: row.try_get("word_count")?,
                    created_at: row.try_get("created_at")?,
                    source_path: row.try_get("source_path").ok(),
                    vector_score: None,
                    keyword_score: row.try_get::<f32, _>("rank").ok(),
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(Into::into)
    }

    /// Reciprocal Rank Fusion over the two ranked lists.
    fn reciprocal_rank_fusion(
        &self,
        vector_hits: &[DocumentChunk],
        keyword_hits: &[DocumentChunk],
        top_k: usize,
    ) -> Vec<RankedChunk> {
        reciprocal_rank_fusion(self.rrf_k, vector_hits, keyword_hits, top_k)
    }

    /// Convert ranked chunks into kernel `Citation`s for LLM grounding.
    pub fn build_citations(
        &self,
        ranked: &[RankedChunk],
        tenant_id: Uuid,
    ) -> Vec<talentos_kernel::shared::Citation> {
        ranked
            .iter()
            .map(|rc| talentos_kernel::shared::Citation {
                chunk_text: rc.chunk.content.clone(),
                source_doc: talentos_kernel::shared::DocReference {
                    tenant_id,
                    doc_id: rc.chunk.document_id,
                    source_path: rc.chunk.source_path.clone().unwrap_or_default(),
                },
                similarity: rc.rrf_score,
                chunk_index: rc.chunk.chunk_index as usize,
            })
            .collect()
    }
}

/// Pure RRF: rank-fuse two ranked lists. Deterministic, no I/O.
fn reciprocal_rank_fusion(
    rrf_k: f32,
    vector_hits: &[DocumentChunk],
    keyword_hits: &[DocumentChunk],
    top_k: usize,
) -> Vec<RankedChunk> {
    use std::collections::HashMap;

    let mut merged: HashMap<i64, RankedChunk> = HashMap::new();

    for (rank, chunk) in vector_hits.iter().enumerate() {
        let contribution = 1.0 / (rrf_k + rank as f32 + 1.0);
        let entry = merged.entry(chunk.id).or_insert_with(|| RankedChunk {
            chunk: chunk.clone(),
            vector_score: 0.0,
            keyword_score: 0.0,
            rrf_score: 0.0,
        });
        entry.vector_score = chunk.vector_score.unwrap_or(0.0);
        entry.rrf_score += contribution;
    }

    for (rank, chunk) in keyword_hits.iter().enumerate() {
        let contribution = 1.0 / (rrf_k + rank as f32 + 1.0);
        let entry = merged.entry(chunk.id).or_insert_with(|| RankedChunk {
            chunk: chunk.clone(),
            vector_score: 0.0,
            keyword_score: 0.0,
            rrf_score: 0.0,
        });
        entry.keyword_score = chunk.keyword_score.unwrap_or(0.0);
        entry.rrf_score += contribution;
    }

    let mut ranked: Vec<RankedChunk> = merged.into_values().collect();
    ranked.sort_by(|a, b| {
        b.rrf_score
            .partial_cmp(&a.rrf_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked.truncate(top_k);
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: i64, vector_score: Option<f32>, keyword_score: Option<f32>) -> DocumentChunk {
        DocumentChunk {
            id,
            document_id: Uuid::nil(),
            chunk_index: 0,
            content: format!("chunk {id}"),
            embedding: None,
            word_count: 2,
            created_at: chrono::Utc::now(),
            source_path: None,
            vector_score,
            keyword_score,
        }
    }

    #[test]
    fn rrf_prefers_chunk_present_in_both_arms() {
        let vector = vec![chunk(1, Some(0.9), None), chunk(2, Some(0.8), None)];
        let keyword = vec![chunk(3, None, Some(0.5)), chunk(2, None, Some(0.4))];
        let fused = reciprocal_rank_fusion(60.0, &vector, &keyword, 10);
        assert_eq!(fused[0].chunk.id, 2, "chunk in both arms must win");
    }

    #[test]
    fn rrf_respects_top_k() {
        let vector = vec![
            chunk(1, Some(0.9), None),
            chunk(2, Some(0.8), None),
            chunk(3, Some(0.7), None),
        ];
        assert_eq!(reciprocal_rank_fusion(60.0, &vector, &[], 2).len(), 2);
    }

    #[test]
    fn rrf_empty_inputs_yield_empty() {
        assert!(reciprocal_rank_fusion(60.0, &[], &[], 5).is_empty());
    }

    #[test]
    fn rrf_is_deterministic() {
        let v = vec![chunk(1, Some(0.9), None), chunk(2, Some(0.8), None)];
        let k = vec![chunk(2, None, Some(0.6))];
        let a = reciprocal_rank_fusion(60.0, &v, &k, 5);
        let b = reciprocal_rank_fusion(60.0, &v, &k, 5);
        assert_eq!(a[0].chunk.id, b[0].chunk.id);
        assert_eq!(a[0].rrf_score, b[0].rrf_score);
    }
}

use crate::models::{HybridSearchResult, RankedChunk, SearchFilters, DocumentChunk, Citation, DocReference};
use sqlx::{PgPool, Row};
use talents_model_gw::HuakatiClient;
use uuid::Uuid;
use chrono::NaiveDate;
use tracing::debug;

/// Hybrid search combining vector similarity + keyword search with RRF reranking
pub struct HybridSearch {
    model_gw: HuakatiClient,
    pool: PgPool,
    rrf_k: i32, // RRF constant (typically 60)
}

impl HybridSearch {
    pub fn new(model_gw: HuakatiClient, pool: PgPool) -> Self {
        Self {
            model_gw,
            pool,
            rrf_k: 60,
        }
    }

    /// Perform hybrid search
    pub async fn search(
        &self,
        tenant_id: &str,
        query: &str,
        filters: &SearchFilters,
        top_k: usize,
    ) -> Result<HybridSearchResult, crate::error::RAGError> {
        // Generate query embedding
        let query_embedding = self.model_gw.embed(tenant_id, query).await?;

        // Perform vector search
        let vector_results = self.vector_search(tenant_id, &query_embedding, filters, top_k * 2).await?;

        // Perform keyword search
        let keyword_results = self.keyword_search(tenant_id, query, filters, top_k * 2).await?;

        // Combine with RRF
        let combined = self.reciprocal_rank_fusion(vector_results, keyword_results, top_k);

        // Convert to ranked chunks
        let mut ranked_chunks = Vec::new();
        for (chunk, rank) in combined.into_iter().enumerate() {
            ranked_chunks.push(RankedChunk {
                chunk: chunk.clone(),
                vector_score: chunk.vector_score.unwrap_or(0.0),
                keyword_score: chunk.keyword_score.unwrap_or(0.0),
                rrf_score: 1.0 / (self.rrf_k as f32 + rank as f32 + 1.0),
            });
        }

        // Sort by RRF score descending
        ranked_chunks.sort_by(|a, b| b.rrf_score.partial_cmp(&a.rrf_score).unwrap_or(std::cmp::Ordering::Equal));

        let total_hits = ranked_chunks.len();

        Ok(HybridSearchResult {
            chunks: ranked_chunks.into_iter().take(top_k).collect(),
            query: query.to_string(),
            filters: filters.clone(),
            total_hits,
        })
    }

    /// Vector similarity search using pgvector
    async fn vector_search(
        &self,
        tenant_id: &str,
        query_embedding: &[f32],
        filters: &SearchFilters,
        limit: usize,
    ) -> Result<Vec<DocumentChunk>, crate::error::RAGError> {
        let vector_str = format!("[{}]", query_embedding.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(","));

        let mut query = r#"
            SELECT dc.id, dc.document_id, dc.chunk_index, dc.content, dc.embedding, dc.word_count, dc.created_at,
                   ad.tenant_id, ad.entity_id, ad.doc_type, ad.source_path
            FROM ai_document_chunks dc
            JOIN ai_documents ad ON ad.id = dc.document_id
            WHERE ad.tenant_id = $1
        "#.to_string();

        let mut params: Vec<Box<dyn sqlx::Encode<'_, sqlx::Postgres> + Send + Sync>> = vec![Box::new(Uuid::parse_str(tenant_id)?)];

        if let Some(entity_id) = filters.entity_id {
            query.push_str(" AND ad.entity_id = $2");
            params.push(Box::new(entity_id));
        }

        if let Some(doc_type) = &filters.doc_type {
            let param_idx = params.len() + 1;
            query.push_str(&format!(" AND ad.doc_type = ${}", param_idx));
            params.push(Box::new(doc_type.to_string()));
        }

        if let Some(date_from) = filters.date_from {
            let param_idx = params.len() + 1;
            query.push_str(&format!(" AND ad.created_at >= ${}", param_idx));
            params.push(Box::new(date_from));
        }

        if let Some(date_to) = filters.date_to {
            let param_idx = params.len() + 1;
            query.push_str(&format!(" AND ad.created_at <= ${}", param_idx));
            params.push(Box::new(date_to));
        }

        query.push_str(&format!(" ORDER BY dc.embedding <=> ${}::vector LIMIT {}", params.len() + 1, limit));
        params.push(Box::new(vector_str));

        // Note: In practice, use sqlx::query_as with proper types
        // This is a simplified version - actual implementation would use query_as!
        let rows = sqlx::query(&query)
            .bind(tenant_id)
            .bind(&vector_str)
            .fetch_all(&self.pool)
            .await?;

        let mut results = Vec::new();
        for row in rows {
            let embedding: Option<Vec<f32>> = row.try_get("embedding").ok().flatten();
            results.push(DocumentChunk {
                id: row.try_get("id")?,
                document_id: row.try_get("document_id")?,
                chunk_index: row.try_get("chunk_index")?,
                content: row.try_get("content")?,
                embedding,
                word_count: row.try_get("word_count")?,
                created_at: row.try_get("created_at")?,
                vector_score: Some(1.0 - row.try_get::<f32, _>("distance").unwrap_or(1.0)), // cosine distance to similarity
                keyword_score: None,
            });
        }

        Ok(results)
    }

    /// Keyword search using PostgreSQL tsvector
    async fn keyword_search(
        &self,
        tenant_id: &str,
        query: &str,
        filters: &SearchFilters,
        limit: usize,
    ) -> Result<Vec<DocumentChunk>, crate::error::RAGError> {
        let ts_query = query
            .split_whitespace()
            .map(|w| format!("{}:*", w))
            .collect::<Vec<_>>()
            .join(" & ");

        let mut query = r#"
            SELECT dc.id, dc.document_id, dc.chunk_index, dc.content, dc.embedding, dc.word_count, dc.created_at,
                   ad.tenant_id, ad.entity_id, ad.doc_type, ad.source_path,
                   ts_rank_cd(dc.tsv, plainto_tsquery('thai', $2)) as rank
            FROM ai_document_chunks dc
            JOIN ai_documents ad ON ad.id = dc.document_id
            WHERE ad.tenant_id = $1
              AND dc.tsv @@ plainto_tsquery('thai', $2)
        "#.to_string();

        let mut params_count = 2;

        if let Some(entity_id) = filters.entity_id {
            params_count += 1;
            query.push_str(&format!(" AND ad.entity_id = ${}", params_count));
        }

        if let Some(doc_type) = &filters.doc_type {
            params_count += 1;
            query.push_str(&format!(" AND ad.doc_type = ${}", params_count));
        }

        query.push_str(&format!(" ORDER BY rank DESC LIMIT {}", limit));

        // This is simplified - actual implementation would use query_as with proper binding
        let rows = sqlx::query(&query)
            .bind(tenant_id)
            .bind(&ts_query)
            .fetch_all(&self.pool)
            .await?;

        let mut results = Vec::new();
        for row in rows {
            let embedding: Option<Vec<f32>> = row.try_get("embedding").ok().flatten();
            results.push(DocumentChunk {
                id: row.try_get("id")?,
                document_id: row.try_get("document_id")?,
                chunk_index: row.try_get("chunk_index")?,
                content: row.try_get("content")?,
                embedding,
                word_count: row.try_get("word_count")?,
                created_at: row.try_get("created_at")?,
                vector_score: None,
                keyword_score: Some(row.try_get("rank").unwrap_or(0.0)),
            });
        }

        Ok(results)
    }

    /// Reciprocal Rank Fusion
    fn reciprocal_rank_fusion(
        &self,
        vector_results: Vec<DocumentChunk>,
        keyword_results: Vec<DocumentChunk>,
        top_k: usize,
    ) -> Vec<DocumentChunk> {
        use std::collections::HashMap;

        // Map chunk id -> (vector_rank, keyword_rank, chunk_data)
        let mut ranks: HashMap<Uuid, (Option<usize>, Option<usize>, DocumentChunk)> = HashMap::new();

        for (rank, chunk) in vector_results.into_iter().enumerate() {
            ranks.entry(chunk.id)
                .or_insert_with(|| (None, None, chunk.clone()))
                .0 = Some(rank);
        }

        for (rank, chunk) in keyword_results.into_iter().enumerate() {
            ranks.entry(chunk.id)
                .or_insert_with(|| (None, None, chunk.clone()))
                .1 = Some(rank);
        }

        // Calculate RRF scores
        let mut scored: Vec<(DocumentChunk, f32)> = Vec::new();

        for (chunk_id, (v_rank_opt, k_rank_opt, chunk_data)) in ranks {
            let rrf_score = v_rank_opt.map(|r| 1.0 / (self.rrf_k as f32 + r as f32 + 1.0)).unwrap_or(0.0)
                + k_rank_opt.map(|r| 1.0 / (self.rrf_k as f32 + r as f32 + 1.0)).unwrap_or(0.0);

            scored.push((chunk_data, rrf_score));
        }

        // Sort by RRF score descending
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Take top_k
        scored.into_iter().take(top_k).map(|(chunk, _)| chunk).collect()
    }

    /// Build citations from ranked chunks for LLM context
    pub fn build_citations(
        &self,
        ranked_chunks: &[RankedChunk],
        tenant_id: Uuid,
    ) -> Vec<Citation> {
        ranked_chunks.iter().map(|rc| Citation {
            chunk_text: rc.chunk.content.clone(),
            source_doc: DocReference {
                tenant_id,
                doc_id: rc.chunk.document_id,
                source_path: "".to_string(), // Would be populated from document metadata
            },
            similarity: rc.rrf_score,
            chunk_index: rc.chunk.chunk_index as usize,
        }).collect()
    }
}
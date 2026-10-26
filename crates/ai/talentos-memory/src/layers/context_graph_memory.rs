//! L4: Context Graph Memory - Semantica Context Graph
//!
//! Entities, decisions, causal relationships, entity resolution.
//! Structured graph of entities, decisions, and causal relationships.

use super::{MemoryLayer, LayerConfig, LayerMetadata, MemoryQuery, MemoryItem, LayerStats, LayerHealth, MemoryLayerError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use semantica::context::{ContextGraph, AgentContext};
use semantica::vector_store::VectorStore;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContextGraphConfig {
    pub base: super::LayerConfig,
    pub context_graph: ContextGraph,
    pub vector_store: VectorStore,
    pub enable_agent_context: bool,
}

impl Default for ContextGraphConfig {
    fn default() -> Self {
        Self {
            base: super::LayerConfig {
                layer_id: 4,
                enabled: true,
                max_size_bytes: Some(10 * 1024 * 1024 * 1024), // 10GB
                ttl_seconds: None,
                compression: true,
                encryption: true,
                custom: Value::Object(serde_json::Map::new()),
            },
            context_graph: ContextGraph::new(),
            vector_store: VectorStore::new("faiss"),
            enable_agent_context: true,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContextGraphStats {
    pub base: super::LayerStats,
    pub nodes: u64,
    pub edges: u64,
    pub agent_contexts: u64,
    pub vector_dimensions: usize,
}

pub struct ContextGraphMemory {
    config: ContextGraphConfig,
    graph: semantica::context::ContextGraph,
    vector_store: semantica::vector_store::VectorStore,
    stats: Arc<RwLock<ContextGraphStats>>,
    initialized: bool,
}

impl ContextGraphMemory {
    pub fn new(config: ContextGraphConfig) -> Self {
        Self {
            config,
            graph: semantica::context::ContextGraph::new(),
            vector_store: semantica::vector_store::VectorStore::new("faiss"),
            stats: Arc::new(RwLock::new(ContextGraphStats {
                base: super::LayerStats {
                    layer_id: 4,
                    layer_name: "Context Graph Memory".to_string(),
                    item_count: 0,
                    size_bytes: 0,
                    hit_rate: 0.0,
                    avg_latency_ms: 0.0,
                    last_cleanup: None,
                    errors_last_hour: 0,
                },
                nodes: 0,
                edges: 0,
                agent_contexts: 0,
                vector_dimensions: 1536,
            }),
            initialized: false,
        }
    }
}

#[async_trait::async_trait]
impl super::MemoryLayer for ContextGraphMemory {
    type Config = ContextGraphConfig;
    type Stats = ContextGraphStats;

    fn layer_id(&self) -> u8 { 4 }
    fn layer_name(&self) -> &'static str { "Context Graph Memory (Semantica)" }

    async fn initialize(&mut self, config: Self::Config) -> Result<(), super::MemoryLayerError> {
        self.config = config.clone();
        self.graph = config.context_graph.clone();
        self.vector_store = config.vector_store.clone();
        self.initialized = true;
        Ok(())
    }

    async fn store(&mut self, key: &str, value: &serde_json::Value, metadata: super::LayerMetadata) -> Result<(), super::MemoryLayerError> {
        // Add node to context graph
        let node_type = metadata.tags.first().cloned().unwrap_or("Entity".to_string());
        let properties: std::collections::HashMap<String, serde_json::Value> = value.as_object()
            .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        self.graph.add_node(key.to_string(), metadata.tags.first().cloned().unwrap_or("Entity".to_string()), properties)
            .map_err(|e| super::MemoryLayerError::StorageError(e.to_string()))?;

        // Also add to vector store for semantic search
        let text = format!("{} {}", key, value.to_string());
        let embedding = self.generate_embedding(&text).await?;
        self.vector_store.add(key.to_string(), embedding, metadata.tags.clone()).await
            .map_err(|e| super::MemoryLayerError::StorageError(e.to_string()))?;

        Ok(())
    }

    async fn retrieve(&self, query: &super::MemoryQuery) -> Result<Vec<super::MemoryItem>, super::MemoryLayerError> {
        let mut results = Vec::new();

        // Graph traversal
        if let Some(query_text) = &query.query_text {
            // Get neighbors in graph
            if let Ok(neighbors) = self.graph.get_neighbors(query_text, 2) {
                for neighbor in neighbors {
                    results.push(super::MemoryItem {
                        key: neighbor.id.clone(),
                        value: serde_json::json!({ "node": neighbor }),
                        metadata: super::LayerMetadata {
                            tenant_id: query.tenant_id,
                            tags: vec!["context_graph".to_string()],
                            importance: 0.8,
                            source: "context_graph".to_string(),
                            ..Default::default()
                        },
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                        access_count: 0,
                        last_accessed: None,
                    });
                }
            }

            // Vector similarity search
            if let Ok(query_embedding) = self.generate_embedding(query_text).await {
                if let Ok(similar) = self.vector_store.search(&embedding, query.limit).await {
                    for (key, score) in similar {
                        results.push(super::MemoryItem {
                            key,
                            value: serde_json::json!({ "score": score }),
                            metadata: super::LayerMetadata {
                                tags: vec!["vector_search".to_string()],
                                importance: score as f32,
                                source: "vector_store".to_string(),
                                ..Default::default()
                            },
                            created_at: chrono::Utc::now(),
                            updated_at: chrono::Utc::now(),
                            access_count: 0,
                            last_accessed: None,
                        });
                    }
                }
            }
        }

        results.truncate(query.limit);
        Ok(results)
    }

    async fn delete(&mut self, key: &str) -> Result<(), super::MemoryLayerError> {
        self.graph.remove_node(key).map_err(|e| super::MemoryLayerError::StorageError(e.to_string()))?;
        self.vector_store.delete(key).await.map_err(|e| super::MemoryLayerError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn stats(&self) -> Result<Self::Stats, super::MemoryLayerError> {
        let summary = self.graph.summary().await?;
        Ok(ContextGraphStats {
            base: super::LayerStats {
                layer_id: 4,
                layer_name: "Context Graph Memory".to_string(),
                item_count: summary.node_count as u64,
                size_bytes: 0,
                hit_rate: 0.0,
                avg_latency_ms: 0.0,
                last_cleanup: None,
                errors_last_hour: 0,
            },
            nodes: summary.node_count as u64,
            edges: summary.edge_count as u64,
            agent_contexts: 0,
            vector_dimensions: 1536,
        })
    }

    async fn health_check(&self) -> Result<super::LayerHealth, super::MemoryLayerError> {
        Ok(super::LayerHealth::Healthy)
    }

    async fn cleanup(&mut self, older_than: chrono::DateTime<Utc>) -> Result<u64, super::MemoryLayerError> {
        Ok(0)
    }
}

impl ContextGraphMemory {
    async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>, super::MemoryLayerError> {
        // Use the vector store's embedding model or external embedding service
        // For now, return a dummy embedding
        Ok(vec![0.0; self.stats.read().await.vector_dimensions])
    }
}
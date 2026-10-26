//! L2: Episodic/Experience Memory - Hindsight Integration
//!
//! Agent's own experiences, interactions, tool executions, and outcomes.
//! Uses Hindsight's experience pathway for learning from interactions.

use super::{MemoryLayer, LayerConfig, LayerMetadata, MemoryQuery, MemoryItem, LayerStats, LayerHealth, MemoryLayerError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpisodicMemoryConfig {
    pub base: super::LayerConfig,
    pub hindsight_server_url: String,
    pub bank_id: String,
    pub auto_consolidate: bool,
    pub consolidation_interval_hours: u64,
    max_items_per_user: usize,
}

impl Default for EpisodicMemoryConfig {
    fn default() -> Self {
        Self {
            base: super::LayerConfig {
                layer_id: 2,
                enabled: true,
                max_size_bytes: Some(10 * 1024 * 1024 * 1024), // 10GB
                ttl_seconds: None, // Experiences don't expire
                compression: true,
                encryption: true,
                custom: Value::Object(serde_json::Map::new()),
            },
            hindsight_server_url: "http://localhost:8888".to_string(),
            bank_id: "talentos-agent-experiences".to_string(),
            auto_consolidate: true,
            consolidation_interval_hours: 6,
            max_items_per_user: 100000,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpisodicMemoryStats {
    pub base: super::LayerStats,
    pub total_experiences: u64,
    pub consolidated_observations: u64,
    pub consolidation_runs: u64,
    pub last_consolidation: Option<chrono::DateTime<Utc>>,
}

pub struct EpisodicMemory {
    config: EpisodicMemoryConfig,
    client: Option<hindsight_client::HindsightClient>,
    stats: Arc<RwLock<EpisodicMemoryStats>>,
    initialized: bool,
}

impl EpisodicMemory {
    pub fn new(config: EpisodicMemoryConfig) -> Self {
        Self {
            config,
            client: None,
            stats: Arc::new(RwLock::new(EpisodicMemoryStats {
                base: super::LayerStats {
                    layer_id: 2,
                    layer_name: "Episodic/Experience Memory".to_string(),
                    item_count: 0,
                    size_bytes: 0,
                    hit_rate: 0.0,
                    avg_latency_ms: 0.0,
                    last_cleanup: None,
                    errors_last_hour: 0,
                },
                total_experiences: 0,
                consolidated_observations: 0,
                consolidation_runs: 0,
                last_consolidation: None,
            }),
            initialized: false,
        }
    }

    async fn get_client(&self) -> Result<hindsight_client::HindsightClient, super::MemoryLayerError> {
        if let Some(client) = &self.client {
            Ok(client.clone())
        } else {
            Err(super::MemoryLayerError::NotInitialized("Hindsight client not initialized".to_string()))
        }
    }
}

#[async_trait::async_trait]
impl super::MemoryLayer for EpisodicMemory {
    type Config = EpisodicMemoryConfig;
    type Stats = EpisodicMemoryStats;

    fn layer_id(&self) -> u8 { 2 }
    fn layer_name(&self) -> &'static str { "Episodic/Experience Memory (Hindsight)" }

    async fn initialize(&mut self, config: Self::Config) -> Result<(), super::MemoryLayerError> {
        self.config = config.clone();

        // Initialize Hindsight client
        let client = hindsight_client::HindsightClient::new(&config.hindsight_server_url)
            .map_err(|e| super::MemoryLayerError::ConfigError(format!("Failed to create Hindsight client: {}", e)))?;

        // Ensure bank exists
        let bank_id = &config.bank_id;
        let _ = client.create_bank(bank_id, Some("TalentOS Agent Experiences")).await;

        self.client = Some(client);
        self.initialized = true;

        // Start consolidation task if enabled
        if config.auto_consolidate {
            let server_url = config.hindsight_server_url.clone();
            let bank_id = config.bank_id.clone();
            let interval_hours = config.consolidation_interval_hours;

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_hours(interval_hours));
                loop {
                    tokio::time::interval(std::time::Duration::from_hours(interval_hours)).tick().await;
                    if let Ok(client) = hindsight_client::HindsightClient::new(&server_url) {
                        if let Err(e) = client.consolidate(&bank_id).await {
                            tracing::warn!("Hindsight consolidation failed: {}", e);
                        } else {
                            tracing::info!("Hindsight consolidation completed for bank: {}", bank_id);
                        }
                    }
                }
            });
        }

        Ok(())
    }

    async fn store(&mut self, key: &str, value: &serde_json::Value, metadata: super::LayerMetadata) -> Result<(), super::MemoryLayerError> {
        let client = self.get_client().await?;

        // Store as an experience in Hindsight
        let content = format!(
            "Experience: {} | Context: {} | Source: {}",
            key,
            serde_json::to_string(&metadata).unwrap_or_default(),
            metadata.source
        );

        let _ = self.get_client().await?.retain(
            &self.config.bank_id,
            &content,
            Some(hindsight_client::RetainOptions {
                context: Some(serde_json::to_string(&metadata).unwrap_or_default()),
                timestamp: Some(chrono::Utc::now().to_rfc3339()),
                ..Default::default()
            })
        ).await.map_err(|e| super::MemoryLayerError::StorageError(format!("Hindsight retain failed: {}", e)))?;

        // Update stats
        let mut stats = self.stats.write().await;
        stats.base.item_count += 1;
        stats.total_experiences += 1;

        Ok(())
    }

    async fn retrieve(&self, query: &super::MemoryQuery) -> Result<Vec<super::MemoryItem>, super::MemoryLayerError> {
        let client = self.get_client().await?;

        let query_text = query.query_text.as_deref().unwrap_or("");

        // Use Hindsight recall
        let recall_result = self.get_client().await?.recall(
            &self.config.bank_id,
            query_text,
            Some(hindsight_client::RecallOptions {
                limit: Some(query.limit as u32),
                metadata_filter: Some(serde_json::json!({
                    "tenant_id": query.tenant_id,
                    "user_id": query.user_id,
                })),
                ..Default::default()
            })
        ).await.map_err(|e| super::MemoryLayerError::QueryError(format!("Hindsight recall failed: {}", e)))?;

        // Convert Hindsight results to MemoryItems
        let mut items = Vec::new();
        for result in recall_result.results {
            items.push(super::MemoryItem {
                key: result.id,
                value: serde_json::json!({
                    "content": result.content,
                    "score": result.score,
                    "memory_type": result.memory_type,
                }),
                metadata: super::LayerMetadata {
                    tenant_id: query.tenant_id,
                    user_id: query.user_id,
                    entity_id: query.entity_id,
                    tags: vec!["experience".to_string(), "hindsight".to_string()],
                    importance: result.score as f32,
                    source: "hindsight".to_string(),
                    ..Default::default()
                },
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                access_count: 0,
                last_accessed: None,
            });

            if items.len() >= query.limit {
                break;
            }
        }

        Ok(items)
    }

    async fn delete(&mut self, key: &str) -> Result<(), super::MemoryLayerError> {
        // Hindsight doesn't support direct deletion by key
        // Would need to use bank-level operations
        Ok(())
    }

    async fn stats(&self) -> Result<Self::Stats, super::MemoryLayerError> {
        let stats = self.stats.read().await;
        Ok(stats.clone())
    }

    async fn health_check(&self) -> Result<super::LayerHealth, super::MemoryLayerError> {
        if !self.initialized {
            return Ok(super::LayerHealth::Unhealthy("Not initialized".to_string()));
        }

        // Check Hindsight server health
        if let Ok(client) = self.get_client() {
            match client.health().await {
                Ok(_) => Ok(super::LayerHealth::Healthy),
                Err(e) => Ok(super::LayerHealth::Degraded(format!("Hindsight unhealthy: {}", e))),
            }
        } else {
            Ok(super::LayerHealth::Unhealthy("Client not available".to_string()))
        }
    }

    async fn cleanup(&mut self, older_than: chrono::DateTime<Utc>) -> Result<u64, super::MemoryLayerError> {
        // Hindsight handles its own cleanup via TTL
        Ok(0)
    }
}
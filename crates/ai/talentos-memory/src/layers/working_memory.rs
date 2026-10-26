//! L1: Working/Immediate Memory
//!
//! Current task context, active conversation, temporary variables.
//! Fast in-memory storage with TTL eviction.

use super::{MemoryLayer, LayerConfig, LayerMetadata, MemoryQuery, MemoryItem, LayerStats, LayerHealth, MemoryLayerError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::time::Instant;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkingMemoryConfig {
    pub base: super::LayerConfig,
    pub max_items: usize,
    pub default_ttl_seconds: u64,
    pub cleanup_interval_seconds: u64,
}

impl Default for WorkingMemoryConfig {
    fn default() -> Self {
        Self {
            base: super::LayerConfig {
                layer_id: 1,
                enabled: true,
                max_size_bytes: Some(100 * 1024 * 1024), // 100MB
                ttl_seconds: Some(3600), // 1 hour
                compression: false,
                encryption: false,
                custom: Value::Object(serde_json::Map::new()),
            },
            max_items: 10000,
            default_ttl_seconds: 3600,
            cleanup_interval_seconds: 300,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkingMemoryItem {
    pub key: String,
    pub value: serde_json::Value,
    pub metadata: super::LayerMetadata,
    pub created_at: chrono::DateTime<Utc>,
    pub expires_at: Option<chrono::DateTime<Utc>>,
    pub access_count: u64,
    pub last_accessed: Option<chrono::DateTime<Utc>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkingMemoryStats {
    pub base: super::LayerStats,
    pub current_items: usize,
    pub expired_items: usize,
    pub hit_rate: f32,
    pub avg_access_time_ms: f32,
}

pub struct WorkingMemory {
    config: WorkingMemoryConfig,
    storage: Arc<RwLock<HashMap<String, WorkingMemoryItem>>>,
    hits: Arc<RwLock<u64>>,
    misses: Arc<RwLock<u64>>,
    total_access_time: Arc<RwLock<u128>>,
    access_count: Arc<RwLock<u64>>,
}

impl WorkingMemory {
    pub fn new(config: WorkingMemoryConfig) -> Self {
        Self {
            config,
            storage: Arc::new(RwLock::new(HashMap::new())),
            hits: Arc::new(RwLock::new(0)),
            misses: Arc::new(RwLock::new(0)),
            total_access_time: Arc::new(RwLock::new(0)),
            access_count: Arc::new(RwLock::new(0)),
        }
    }

    async fn cleanup_expired(&self) -> usize {
        let mut storage = self.storage.write().await;
        let now = chrono::Utc::now();
        let initial_len = storage.len();
        storage.retain(|_, item| {
            item.expires_at.map_or(true, |exp| exp > chrono::Utc::now())
        });
        initial_len - storage.len()
    }

    fn make_key(&self, tenant_id: Option<uuid::Uuid>, key: &str) -> String {
        match key.strip_prefix("tenant:") {
            Some(k) => format!("tenant:{}:{}", key, k),
            None => format!("tenant:{}:{}", key, "default"),
        }
    }
}

#[async_trait::async_trait]
impl super::MemoryLayer for WorkingMemory {
    type Config = WorkingMemoryConfig;
    type Stats = WorkingMemoryStats;

    fn layer_id(&self) -> u8 { 1 }
    fn layer_name(&self) -> &'static str { "Working/Immediate Memory" }

    async fn initialize(&mut self, config: Self::Config) -> Result<(), super::MemoryLayerError> {
        self.config = config;
        // Start background cleanup task
        let storage = self.storage.clone();
        let interval = self.config.cleanup_interval_seconds;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval));
            loop {
                interval.tick().await;
                let mut storage = storage.write().await;
                let now = chrono::Utc::now();
                let before = storage.len();
                storage.retain(|_, item| {
                    item.expires_at.map_or(true, |exp| exp > chrono::Utc::now())
                });
                let cleaned = before.saturating_sub(storage.len());
                if cleaned > 0 {
                    tracing::debug!("WorkingMemory: cleaned {} expired items", cleaned);
                }
            }
        });
        Ok(())
    }

    async fn store(&mut self, key: &str, value: &serde_json::Value, metadata: super::LayerMetadata) -> Result<(), super::MemoryLayerError> {
        let start = std::time::Instant::now();
        let key = self.make_key(metadata.tenant_id, key);
        let ttl = self.config.default_ttl_seconds;
        let expires_at = Some(chrono::Utc::now() + chrono::Duration::seconds(ttl as i64));

        let item = WorkingMemoryItem {
            key: key.clone(),
            value: value.clone(),
            metadata,
            created_at: chrono::Utc::now(),
            expires_at,
            access_count: 0,
            last_accessed: None,
        };

        let mut storage = self.storage.write().await;
        if storage.len() >= self.config.max_items {
            // Evict oldest accessed item
            if let Some((oldest_key, _)) = storage.iter()
                .min_by_key(|(_, v)| v.last_accessed.unwrap_or(v.created_at))
                .map(|(k, _)| k.clone()) {
                storage.remove(&oldest_key);
            }
        }
        storage.insert(key, item);

        *self.total_access_time.write().await += start.elapsed().as_millis() as u128;
        *self.access_count.write().await += 1;

        Ok(())
    }

    async fn retrieve(&self, query: &super::MemoryQuery) -> Result<Vec<super::MemoryItem>, super::MemoryLayerError> {
        let start = std::time::Instant::now();
        let storage = self.storage.read().await;

        let mut results = Vec::new();
        let query_text = query.query_text.as_deref().unwrap_or("");

        for (key, item) in storage.iter() {
            // Check tenant isolation
            if let Some(tenant_id) = query.tenant_id {
                if item.metadata.tenant_id != Some(tenant_id) {
                    continue;
                }
            }

            // Check user isolation
            if let Some(user_id) = query.user_id {
                if item.metadata.user_id != Some(user_id) {
                    continue;
                }
            }

            // Check tags
            if !query.tags.is_empty() {
                if !query.tags.iter().any(|t| item.metadata.tags.contains(t)) {
                    continue;
                }
            }

            // Check time range
            if let Some((start, end)) = query.time_range {
                if item.created_at < start || item.created_at > end {
                    continue;
                }
            }

            // Check importance
            if item.metadata.importance < query.min_importance {
                continue;
            }

            // Text search
            if !query.query_text.is_none() && !query_text.is_empty() {
                let value_str = serde_json::to_string(&item.value).unwrap_or_default();
                if !value_str.to_lowercase().contains(&query_text.to_lowercase()) {
                    continue;
                }
            }

            results.push(super::MemoryItem {
                key: item.key.clone(),
                value: item.value.clone(),
                metadata: item.metadata.clone(),
                created_at: item.created_at,
                updated_at: item.metadata.expires_at.unwrap_or(chrono::Utc::now()),
                access_count: item.access_count,
                last_accessed: item.last_accessed,
            });

            if results.len() >= query.limit {
                break;
            }
        }

        // Update hit/miss counters
        if !results.is_empty() {
            *self.hits.write().await += 1;
        } else {
            *self.misses.write().await += 1;
        }

        *self.total_access_time.write().await += start.elapsed().as_millis() as u128;
        *self.access_count.write().await += 1;

        Ok(results)
    }

    async fn delete(&mut self, key: &str) -> Result<(), super::MemoryLayerError> {
        let key = self.make_key(None, key);
        self.storage.write().await.remove(key);
        Ok(())
    }

    async fn stats(&self) -> Result<Self::Stats, super::MemoryLayerError> {
        let storage = self.storage.read().await;
        let hits = *self.hits.read().await;
        let misses = *self.misses.read().await;
        let total_access = *self.total_access_time.read().await;
        let access_count = *self.access_count.read().await;

        let hit_rate = if hits + misses > 0 {
            hits as f32 / (hits + misses) as f32
        } else {
            0.0
        };

        let avg_latency = if *self.access_count.read().await > 0 {
            *self.total_access_time.read().await as f32 / *self.access_count.read().await as f32
        } else {
            0.0
        };

        Ok(WorkingMemoryStats {
            base: super::LayerStats {
                layer_id: 1,
                layer_name: "Working/Immediate Memory".to_string(),
                item_count: storage.len() as u64,
                size_bytes: 0, // Would need serialization to calculate
                hit_rate,
                avg_latency_ms: avg_latency,
                last_cleanup: None,
                errors_last_hour: 0,
            },
            current_items: storage.len(),
            expired_items: 0,
            hit_rate,
            avg_access_time_ms: avg_latency,
        })
    }

    async fn health_check(&self) -> Result<super::LayerHealth, super::MemoryLayerError> {
        let storage = self.storage.read().await;
        let utilization = storage.len() as f32 / self.config.max_items as f32;

        if utilization > 0.9 {
            Ok(super::LayerHealth::Degraded(format!("Working memory at {:.1}% capacity", utilization * 100.0)))
        } else if utilization > 1.0 {
            Ok(super::LayerHealth::Unhealthy("Working memory over capacity".to_string()))
        } else {
            Ok(super::LayerHealth::Healthy)
        }
    }

    async fn cleanup(&mut self, older_than: chrono::DateTime<Utc>) -> Result<u64, super::MemoryLayerError> {
        let mut storage = self.storage.write().await;
        let before = storage.len();
        storage.retain(|_, item| item.created_at > older_than);
        Ok((storage.len().saturating_sub(before)) as u64)
    }
}
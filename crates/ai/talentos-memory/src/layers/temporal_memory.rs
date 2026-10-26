//! L7: Temporal/Provenance Memory - Hindsight Temporal + Semantica Bi-temporal
//!
//! Time-travel queries, bi-temporal graphs, audit trails, W3C PROV-O provenance.
//! Point-in-time snapshots, Allen interval algebra, full auditability.

use super::{MemoryLayer, LayerConfig, LayerMetadata, MemoryQuery, MemoryItem, LayerStats, LayerHealth, MemoryLayerError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc, NaiveDate};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use semantica::temporal::{TemporalGraph, BiTemporalFact, AllenRelation};
use semantica::provenance::{ProvenanceManager, ProvenanceRecord};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TemporalMemoryConfig {
    pub base: super::LayerConfig,
    pub temporal_graph: semantica::temporal::TemporalGraph,
    pub provenance_manager: semantica::provenance::ProvenanceManager,
    pub retention_years: u32,
    pub snapshot_interval_hours: u64,
    pub enable_allen_algebra: bool,
}

impl Default for TemporalMemoryConfig {
    fn default() -> Self {
        Self {
            base: super::LayerConfig {
                layer_id: 7,
                enabled: true,
                max_size_bytes: Some(50 * 1024 * 1024 * 1024), // 50GB
                ttl_seconds: None,
                compression: true,
                encryption: true,
                custom: Value::Object(serde_json::Map::new()),
            },
            temporal_graph: semantica::temporal::TemporalGraph::new(),
            provenance_manager: semantica::provenance::ProvenanceManager::new(),
            retention_years: 7, // Thai labor law requires 7 years retention
            snapshot_interval_hours: 24,
            enable_allen_algebra: true,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TemporalMemoryStats {
    pub base: super::LayerStats,
    pub bi_temporal_facts: u64,
    pub provenance_records: u64,
    pub snapshots: u64,
    pub allen_relations: u64,
    pub oldest_fact: Option<chrono::DateTime<Utc>>,
    pub newest_fact: Option<chrono::DateTime<Utc>>,
}

pub struct TemporalMemory {
    config: TemporalMemoryConfig,
    temporal_graph: semantica::temporal::TemporalGraph,
    provenance_manager: semantica::provenance::ProvenanceManager,
    stats: Arc<RwLock<TemporalMemoryStats>>,
    initialized: bool,
}

impl TemporalMemory {
    pub fn new(config: TemporalMemoryConfig) -> Self {
        Self {
            config,
            temporal_graph: semantica::temporal::TemporalGraph::new(),
            provenance_manager: semantica::provenance::ProvenanceManager::new(),
            stats: Arc::new(RwLock::new(TemporalMemoryStats {
                base: super::LayerStats {
                    layer_id: 7,
                    layer_name: "Temporal/Provenance Memory".to_string(),
                    item_count: 0,
                    size_bytes: 0,
                    hit_rate: 0.0,
                    avg_latency_ms: 0.0,
                    last_cleanup: None,
                    errors_last_hour: 0,
                },
                bi_temporal_facts: 0,
                provenance_records: 0,
                snapshots: 0,
                allen_relations: 0,
                oldest_fact: None,
                newest_fact: None,
            }),
            initialized: false,
        }
    }
}

#[async_trait::async_trait]
impl super::MemoryLayer for TemporalMemory {
    type Config = TemporalMemoryConfig;
    type Stats = TemporalMemoryStats;

    fn layer_id(&self) -> u8 { 7 }
    fn layer_name(&self) -> &'static str { "Temporal/Provenance Memory" }

    async fn initialize(&mut self, config: Self::Config) -> Result<(), super::MemoryLayerError> {
        self.config = config.clone();
        self.temporal_graph = config.temporal_graph.clone();
        self.provenance_manager = config.provenance_manager.clone();

        // Start snapshot timer
        let temporal_graph = self.temporal_graph.clone();
        let interval = self.config.snapshot_interval_hours;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_hours(interval));
            loop {
                tokio::time::interval(std::time::Duration::from_hours(interval)).tick().await;
                if let Err(e) = temporal_graph.create_snapshot().await {
                    tracing::warn!("Temporal snapshot failed: {}", e);
                }
            }
        });

        self.initialized = true;
        Ok(())
    }

    async fn store(&mut self, key: &str, value: &serde_json::Value, metadata: super::LayerMetadata) -> Result<(), super::MemoryLayerError> {
        // Store as bi-temporal fact
        let valid_from = metadata.expires_at.unwrap_or_else(chrono::Utc::now);
        let valid_to = None; // Open-ended

        let bi_temporal_fact = semantica::temporal::BiTemporalFact {
            entity_id: key.to_string(),
            attribute: metadata.tags.join(","),
            value: value.to_string(),
            valid_from,
            valid_to,
            transaction_from: chrono::Utc::now(),
            transaction_to: None,
        };

        self.temporal_graph.add_fact(bi_temporal_fact).map_err(|e| {
            super::MemoryLayerError::StorageError(e.to_string())
        })?;

        // Track provenance
        let provenance = ProvenanceRecord {
            entity_id: key.to_string(),
            activity: "store".to_string(),
            agent: metadata.source,
            timestamp: chrono::Utc::now(),
            used_entities: vec![],
            generated_entities: vec![key.to_string()],
            metadata: metadata.custom.clone(),
        };

        self.provenance_manager.record(provenance).map_err(|e| {
            super::MemoryLayerError::StorageError(e.to_string())
        })?;

        Ok(())
    }

    async fn retrieve(&self, query: &super::MemoryQuery) -> Result<Vec<super::MemoryItem>, super::MemoryLayerError> {
        let mut results = Vec::new();
        let query_text = query.query_text.as_deref().unwrap_or("");

        // Time-travel query
        if let Some(time_point) = query.layer_specific.get("as_of").and_then(|v| v.as_str()) {
            if let Ok(time) = chrono::DateTime::parse_from_rfc3339(time_point) {
                if let Ok(snapshot) = self.temporal_graph.state_at(time.with_timezone(&Utc)).await {
                    for (key, value) in snapshot {
                        results.push(super::MemoryItem {
                            key,
                            value: serde_json::json!({ "value": value }),
                            metadata: super::LayerMetadata {
                                tenant_id: query.tenant_id,
                                tags: vec!["temporal".to_string(), "snapshot".to_string()],
                                importance: 0.9,
                                source: "temporal_graph".to_string(),
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
        } else if !query.query_text.is_none() {
            // Query current state
            let query_text = query.query_text.as_deref().unwrap_or("");
            if let Ok(entity) = self.temporal_graph.get_entity(query_text).await {
                results.push(super::MemoryItem {
                    key: entity.id.clone(),
                    value: serde_json::json!({ "entity": entity }),
                    metadata: super::LayerMetadata {
                        tenant_id: query.tenant_id,
                        tags: vec!["temporal".to_string(), "current".to_string()],
                        importance: 0.8,
                        source: "temporal_graph".to_string(),
                        ..Default::default()
                    },
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    access_count: 0,
                    last_accessed: None,
                });
            }
        }

        // Also query provenance
        if let Some(entity_id) = query.entity_id {
            if let Ok(provenance) = self.provenance_manager.get_provenance(&entity_id.to_string()).await {
                results.push(super::MemoryItem {
                    key: format!("provenance:{}", entity_id),
                    value: serde_json::json!({ "provenance": provenance }),
                    metadata: super::LayerMetadata {
                        tenant_id: query.tenant_id,
                        tags: vec!["provenance".to_string(), "audit".to_string()],
                        importance: 1.0,
                        source: "provenance_manager".to_string(),
                        ..Default::default()
                    },
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    access_count: 0,
                    last_accessed: None,
                });
            }
        }

        results.truncate(query.limit);
        Ok(results)
    }

    async fn delete(&mut self, key: &str) -> Result<(), super::MemoryLayerError> {
        // Soft delete - mark as deleted with transaction_to = now
        self.temporal_graph.end_fact_validity(key, chrono::Utc::now()).map_err(|e| {
            super::MemoryLayerError::StorageError(e.to_string())
        })?;

        // Record deletion provenance
        let provenance = semantica::provenance::ProvenanceRecord {
            entity_id: key.to_string(),
            activity: "delete".to_string(),
            agent: "system".to_string(),
            timestamp: chrono::Utc::now(),
            used_entities: vec![],
            generated_entities: vec![],
            metadata: serde_json::json!({ "deleted": true }),
        };

        self.provenance_manager.record(provenance).map_err(|e| {
            super::MemoryLayerError::StorageError(e.to_string())
        })?;

        Ok(())
    }

    async fn stats(&self) -> Result<Self::Stats, super::MemoryLayerError> {
        let stats = self.stats.read().await;
        Ok(stats.clone())
    }

    async fn health_check(&self) -> Result<super::LayerHealth, super::MemoryLayerError> {
        Ok(super::LayerHealth::Healthy)
    }

    async fn cleanup(&mut self, older_than: chrono::DateTime<Utc>) -> Result<u64, super::MemoryLayerError> {
        // Archive old facts beyond retention period
        let cutoff = chrono::Utc::now() - chrono::Duration::days(self.config.retention_years as i64 * 365);
        let deleted = self.temporal_graph.archive_before(cutoff).await.map_err(|e| {
            super::MemoryLayerError::StorageError(e.to_string())
        })?;
        Ok(deleted)
    }
}

impl TemporalMemory {
    /// Get entity state at a specific point in time (time travel)
    pub async fn state_at(&self, time: chrono::DateTime<Utc>) -> Result<HashMap<String, String>, super::MemoryLayerError> {
        self.temporal_graph.state_at(time).await.map_err(|e| {
            super::MemoryLayerError::QueryError(e.to_string())
        })
    }

    /// Get full provenance chain for an entity
    pub async fn get_provenance(&self, entity_id: &str) -> Result<Vec<semantica::provenance::ProvenanceRecord>, super::MemoryLayerError> {
        self.provenance_manager.get_provenance(entity_id).await.map_err(|e| {
            super::MemoryLayerError::QueryError(e.to_string())
        })
    }

    /// Query using Allen interval algebra
    pub async fn query_allen(&self, entity: &str, relation: AllenRelation, time_range: (chrono::DateTime<Utc>, chrono::DateTime<Utc>)) -> Result<Vec<String>, super::MemoryLayerError> {
        self.temporal_graph.query_allen(entity, relation, time_range).await.map_err(|e| {
            super::MemoryLayerError::QueryError(e.to_string())
        })
    }

    /// Export audit trail as W3C PROV-O
    pub async fn export_prov_o(&self) -> Result<String, super::MemoryLayerError> {
        let kg = self.temporal_graph.to_kg_dict().await.map_err(|e| {
            super::MemoryLayerError::SerializationError(e.to_string())
        })?;

        let exporter = semantica::export::RDFExporter::new();
        exporter.export(kg, "audit_trail.ttl", semantica::export::RDFFormat::Turtle).await.map_err(|e| {
            super::MemoryLayerError::SerializationError(e.to_string())
        })?;

        std::fs::read_to_string("audit_trail.ttl").map_err(|e| {
            super::MemoryLayerError::StorageError(e.to_string())
        })
    }
}
//! L3: Semantic/World Facts Memory
//!
//! Domain knowledge, world facts, Thai HR/Payroll rules, compliance rules.
//! Combines Hindsight World Facts pathway with Semantica Knowledge Graph.

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
pub struct SemanticMemoryConfig {
    pub base: super::LayerConfig,
    pub hindsight_server_url: String,
    pub hindsight_bank_id: String,
    pub semantica_graph: semantica::context::ContextGraph,
    pub enable_auto_ingest: bool,
    pub ingest_batch_size: usize,
}

impl Default for SemanticMemoryConfig {
    fn default() -> Self {
        Self {
            base: super::LayerConfig {
                layer_id: 3,
                enabled: true,
                max_size_bytes: Some(5 * 1024 * 1024 * 1024), // 5GB
                ttl_seconds: None,
                compression: true,
                encryption: true,
                custom: Value::Object(serde_json::Map::new()),
            },
            hindsight_server_url: "http://localhost:8888".to_string(),
            hindsight_bank_id: "talentos-world-facts".to_string(),
            semantica_graph: semantica::context::ContextGraph::default(),
            enable_auto_ingest: true,
            ingest_batch_size: 100,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SemanticMemoryStats {
    pub base: super::LayerStats,
    pub world_facts_count: u64,
    pub kg_nodes: u64,
    pub kg_edges: u64,
    pub ontology_classes: u64,
    pub ontology_properties: u64,
}

pub struct SemanticMemory {
    config: SemanticMemoryConfig,
    hindsight_client: Option<hindsight_client::HindsightClient>,
    semantica_graph: semantica::context::ContextGraph,
    stats: Arc<RwLock<SemanticMemoryStats>>,
    initialized: bool,
}

impl SemanticMemory {
    pub fn new(config: SemanticMemoryConfig) -> Self {
        let graph = semantica::context::ContextGraph::new();
        Self {
            config,
            hindsight_client: None,
            semantica_graph: graph,
            stats: Arc::new(RwLock::new(SemanticMemoryStats {
                base: super::LayerStats {
                    layer_id: 3,
                    layer_name: "Semantic/World Facts Memory".to_string(),
                    item_count: 0,
                    size_bytes: 0,
                    hit_rate: 0.0,
                    avg_latency_ms: 0.0,
                    last_cleanup: None,
                    errors_last_hour: 0,
                },
                world_facts_count: 0,
                kg_nodes: 0,
                kg_edges: 0,
                ontology_classes: 0,
                ontology_properties: 0,
            }),
            initialized: false,
        }
    }
}

#[async_trait::async_trait]
impl super::MemoryLayer for SemanticMemory {
    type Config = SemanticMemoryConfig;
    type Stats = SemanticMemoryStats;

    fn layer_id(&self) -> u8 { 3 }
    fn layer_name(&self) -> &'static str { "Semantic/World Facts Memory" }

    async fn initialize(&mut self, config: Self::Config) -> Result<(), super::MemoryLayerError> {
        self.config = config.clone();

        // Initialize Hindsight client for world facts
        let client = hindsight_client::HindsightClient::new(&config.hindsight_server_url)
            .map_err(|e| super::MemoryLayerError::ConfigError(format!("Hindsight client failed: {}", e)))?;

        let _ = client.create_bank(&config.hindsight_bank_id, Some("TalentOS World Facts")).await;

        self.hindsight_client = Some(client);

        // Initialize Semantica ContextGraph with Thai HR ontology
        self.semantica_graph = self.build_thai_hr_ontology().await?;

        self.initialized = true;
        Ok(())
    }

    async fn build_thai_hr_ontology(&self) -> Result<semantica::context::ContextGraph, super::MemoryLayerError> {
        let mut graph = semantica::context::ContextGraph::new();

        // Add Thai HR core concepts as nodes
        let hr_concepts = vec![
            ("Employee", "Person", vec![("employee_code", "String"), ("base_salary", "Decimal")]),
            ("Department", "OrganizationUnit", vec![("name", "String"), ("cost_center", "String")]),
            ("Position", "Role", vec![("title", "String"), ("level", "Integer")]),
            ("PayrollRun", "Event", vec![("period_start", "Date"), ("period_end", "Date"), ("status", "String")]),
            ("Payslip", "Document", vec![("employee_id", "UUID"), ("net_pay", "Decimal"), ("tax", "Decimal")]),
            ("LeaveRequest", "Event", vec![("employee_id", "UUID"), ("leave_type", "String"), ("days", "Decimal")]),
            ("TaxBracket", "Regulation", vec![("lower_bound", "Decimal"), ("upper_bound", "Decimal"), ("rate", "Decimal")]),
            ("SSORule", "Regulation", vec![("rate", "Decimal"), ("max_wage_base", "Decimal")]),
            ("PFRule", "Regulation", vec![("employee_rate", "Decimal"), ("employer_rate", "Decimal")]),
            ("SeveranceRule", "Regulation", vec![("min_months", "Integer"), ("max_months", "Integer"), ("days", "Integer")]),
            ("OTRule", "Regulation", vec![("weekday_rate", "Decimal"), ("weekend_rate", "Decimal"), ("holiday_rate", "Decimal")]),
        ];

        for (name, type_, props) in hr_concepts {
            let mut props_map = std::collections::HashMap::new();
            for (k, v) in props {
                props_map.insert(k.to_string(), serde_json::Value::String(v.to_string()));
            }
            graph.add_node(name.to_string(), type_.to_string(), props_map).map_err(|e| {
                super::MemoryLayerError::ConfigError(format!("Failed to add node: {}", e))
            })?;
        }

        // Add relationships
        let relationships = vec![
            ("Employee", "Department", "belongs_to"),
            ("Employee", "Position", "holds"),
            ("Employee", "PayrollRun", "participates_in"),
            ("Employee", "Payslip", "receives"),
            ("Employee", "LeaveRequest", "submits"),
            ("PayrollRun", "Payslip", "generates"),
            ("TaxBracket", "Payslip", "applies_to"),
            ("SSORule", "Payslip", "applies_to"),
            ("PFRule", "Payslip", "applies_to"),
            ("SeveranceRule", "Employee", "applies_to"),
            ("OTRule", "Payslip", "applies_to"),
        ];

        for (from, to, rel_type) in relationships {
            graph.add_edge(from.to_string(), to.to_string(), rel_type.to_string(), std::collections::HashMap::new())
                .map_err(|e| super::MemoryLayerError::ConfigError(format!("Failed to add edge: {}", e)))?;
        }

        Ok(graph)
    }
}

#[async_trait::async_trait]
impl super::MemoryLayer for SemanticMemory {
    type Config = SemanticMemoryConfig;
    type Stats = SemanticMemoryStats;

    fn layer_id(&self) -> u8 { 3 }
    fn layer_name(&self) -> &'static str { "Semantic/World Facts Memory" }

    async fn initialize(&mut self, config: Self::Config) -> Result<(), super::MemoryLayerError> {
        self.config = config.clone();

        // Initialize Hindsight client for world facts
        let client = hindsight_client::HindsightClient::new(&config.hindsight_server_url)
            .map_err(|e| super::MemoryLayerError::ConfigError(format!("Hindsight client failed: {}", e)))?;

        let _ = client.create_bank(&config.hindsight_bank_id, Some("TalentOS World Facts")).await;

        self.hindsight_client = Some(client);

        // Initialize Semantica Graph with Thai HR ontology
        self.semantica_graph = self.build_thai_hr_ontology().await?;

        self.initialized = true;
        Ok(())
    }

    async fn store(&mut self, key: &str, value: &serde_json::Value, metadata: super::LayerMetadata) -> Result<(), super::MemoryLayerError> {
        // Store in Hindsight world facts
        if let Some(client) = &self.hindsight_client {
            let content = format!("Fact: {} | Source: {}", key, metadata.source);
            let _ = client.retain(
                &self.config.hindsight_bank_id,
                &content,
                Some(hindsight_client::RetainOptions {
                    context: Some(serde_json::to_string(&metadata).unwrap_or_default()),
                    timestamp: Some(chrono::Utc::now().to_rfc3339()),
                    ..Default::default()
                })
            ).await.map_err(|e| super::MemoryLayerError::StorageError(e.to_string()))?;
        }

        // Also store in Semantica graph if it's a fact
        if metadata.tags.contains(&"fact".to_string()) || metadata.tags.contains(&"rule".to_string()) {
            self.semantica_graph.add_node(
                key.to_string(),
                "Fact".to_string(),
                vec![
                    ("content".to_string(), value.clone()),
                    ("source".to_string(), serde_json::Value::String(metadata.source.clone())),
                ].into_iter().collect(),
            ).map_err(|e| super::MemoryLayerError::StorageError(e.to_string()))?;
        }

        Ok(())
    }

    async fn retrieve(&self, query: &super::MemoryQuery) -> Result<Vec<super::MemoryItem>, super::MemoryLayerError> {
        let mut results = Vec::new();

        // Search Hindsight world facts
        if let Some(client) = &self.hindsight_client {
            let query_text = query.query_text.as_deref().unwrap_or("");
            if let Ok(recall) = client.recall(
                &self.config.hindsight_bank_id,
                query_text,
                Some(hindsight_client::RecallOptions {
                    limit: Some(query.limit as u32),
                    ..Default::default()
                })
            ).await {
                for result in recall.results {
                    results.push(super::MemoryItem {
                        key: result.id,
                        value: serde_json::json!({ "content": result.content, "score": result.score }),
                        metadata: super::LayerMetadata {
                            tenant_id: query.tenant_id,
                            tags: vec!["world_fact".to_string()],
                            importance: result.score as f32,
                            source: "hindsight_world_facts".to_string(),
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

        // Also query Semantica graph
        if !query.query_text.is_none() {
            let query_text = query.query_text.as_deref().unwrap_or("");
            if let Ok(neighbors) = self.semantica_graph.get_neighbors(query_text, 2) {
                for neighbor in neighbors {
                    results.push(super::MemoryItem {
                        key: neighbor.id.clone(),
                        value: serde_json::json!({ "node": neighbor }),
                        metadata: super::LayerMetadata {
                            tenant_id: query.tenant_id,
                            tags: vec!["semantic_kg".to_string()],
                            importance: 0.8,
                            source: "semantica_kg".to_string(),
                            ..Default::default()
                        },
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                        access_count: 0,
                        last_accessed: None,
                    });
                }
            }

            // Limit results
            results.truncate(query.limit);
        }

        Ok(results)
    }

    async fn delete(&mut self, key: &str) -> Result<(), super::MemoryLayerError> {
        // Remove from Semantica graph
        self.semantica_graph.remove_node(key).map_err(|e| super::MemoryLayerError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn stats(&self) -> Result<Self::Stats, super::MemoryLayerError> {
        let graph_summary = self.semantica_graph.summary().await?;
        Ok(SemanticMemoryStats {
            base: super::LayerStats {
                layer_id: 3,
                layer_name: "Semantic/World Facts Memory".to_string(),
                item_count: graph_summary.node_count as u64,
                size_bytes: 0,
                hit_rate: 0.0,
                avg_latency_ms: 0.0,
                last_cleanup: None,
                errors_last_hour: 0,
            },
            world_facts_count: 0, // Would need Hindsight stats
            kg_nodes: graph_summary.node_count as u64,
            kg_edges: graph_summary.edge_count as u64,
            ontology_classes: 0,
            ontology_properties: 0,
        })
    }

    async fn health_check(&self) -> Result<super::LayerHealth, super::MemoryLayerError> {
        if !self.initialized {
            return Ok(super::LayerHealth::Unhealthy("Not initialized".to_string()));
        }
        Ok(super::LayerHealth::Healthy)
    }

    async fn cleanup(&mut self, older_than: chrono::DateTime<Utc>) -> Result<u64, super::MemoryLayerError> {
        Ok(0)
    }
}
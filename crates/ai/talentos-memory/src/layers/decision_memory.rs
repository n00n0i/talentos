//! L5: Decision Intelligence Memory - Semantica Decision Intelligence
//!
//! Recorded decisions with causal chains, precedents, impact analysis.
//! First-class decision records with full auditability.

use super::{MemoryLayer, LayerConfig, LayerMetadata, MemoryQuery, MemoryItem, LayerStats, LayerHealth, MemoryLayerError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use semantica::context::ContextGraph;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DecisionMemoryConfig {
    pub base: super::LayerConfig,
    pub context_graph: semantica::context::ContextGraph,
    pub decision_rules: Vec<DecisionRule>,
    pub require_approval_for_destructive: bool,
}

impl Default for DecisionMemoryConfig {
    fn default() -> Self {
        Self {
            base: super::LayerConfig {
                layer_id: 5,
                enabled: true,
                max_size_bytes: Some(1024 * 1024 * 1024), // 1GB
                ttl_seconds: None,
                compression: true,
                encryption: true,
                custom: Value::Object(serde_json::Map::new()),
            },
            context_graph: ContextGraph::new(),
            decision_rules: Vec::new(),
            require_approval_for_destructive: true,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DecisionRule {
    pub category: String,
    pub required_confidence: f32,
    pub allowed_outcomes: Vec<String>,
    pub requires_approval: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DecisionRecord {
    pub id: Uuid,
    pub category: String,
    pub scenario: String,
    pub reasoning: String,
    pub outcome: String,
    pub confidence: f32,
    pub decision_maker: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub causal_parents: Vec<Uuid>,
    pub causal_children: Vec<Uuid>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DecisionMemoryStats {
    pub base: super::LayerStats,
    pub total_decisions: u64,
    pub decisions_by_category: std::collections::HashMap<String, u64>,
    pub avg_confidence: f32,
    pub pending_approvals: u64,
}

pub struct DecisionMemory {
    config: DecisionMemoryConfig,
    graph: semantica::context::ContextGraph,
    stats: Arc<RwLock<DecisionMemoryStats>>,
    initialized: bool,
}

impl DecisionMemory {
    pub fn new(config: DecisionMemoryConfig) -> Self {
        Self {
            config,
            graph: ContextGraph::new(),
            stats: Arc::new(RwLock::new(DecisionMemoryStats {
                base: super::LayerStats {
                    layer_id: 5,
                    layer_name: "Decision Intelligence Memory".to_string(),
                    item_count: 0,
                    size_bytes: 0,
                    hit_rate: 0.0,
                    avg_latency_ms: 0.0,
                    last_cleanup: None,
                    errors_last_hour: 0,
                },
                total_decisions: 0,
                decisions_by_category: HashMap::new(),
                avg_confidence: 0.0,
                pending_approvals: 0,
            }),
            initialized: false,
        }
    }
}

#[async_trait::async_trait]
impl super::MemoryLayer for DecisionMemory {
    type Config = DecisionMemoryConfig;
    type Stats = DecisionMemoryStats;

    fn layer_id(&self) -> u8 { 5 }
    fn layer_name(&self) -> &'static str { "Decision Intelligence Memory" }

    async fn initialize(&mut self, config: Self::Config) -> Result<(), super::MemoryLayerError> {
        self.config = config.clone();
        self.graph = config.context_graph.clone();
        self.initialized = true;
        Ok(())
    }

    async fn store(&mut self, key: &str, value: &serde_json::Value, metadata: super::LayerMetadata) -> Result<(), super::MemoryLayerError> {
        // Store decision in graph
        let decision_id = Uuid::new_v4();
        let decision = DecisionRecord {
            id: Uuid::parse_str(key).unwrap_or_else(|_| Uuid::new_v4()),
            category: value.get("category").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
            scenario: value.get("scenario").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            reasoning: value.get("reasoning").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            outcome: value.get("outcome").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            confidence: value.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            decision_maker: value.get("decision_maker").and_then(|v| v.as_str()).unwrap_or("system").to_string(),
            metadata: value.get("metadata").cloned().unwrap_or(Value::Object(serde_json::Map::new())),
            created_at: chrono::Utc::now(),
            causal_parents: value.get("causal_parents").and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| Uuid::parse_str(v.as_str().unwrap_or("")).ok()).collect())
                .unwrap_or_default(),
            causal_children: Vec::new(),
        };

        // Store in graph
        self.graph.add_node(
            decision.id.to_string(),
            "Decision".to_string(),
            vec![
                ("category".to_string(), serde_json::Value::String(decision.category.clone())),
                ("scenario".to_string(), serde_json::Value::String(decision.scenario.clone())),
                ("reasoning".to_string(), serde_json::Value::String(decision.reasoning.clone())),
                ("outcome".to_string(), serde_json::Value::String(decision.outcome.clone())),
                ("confidence".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(decision.confidence as f64).unwrap())),
                ("outcome".to_string(), serde_json::Value::String(decision.outcome.clone())),
                ("decision_maker".to_string(), serde_json::Value::String(decision.decision_maker.clone())),
            ].into_iter().collect(),
        ).map_err(|e| super::MemoryLayerError::StorageError(e.to_string()))?;

        // Add causal relationships
        for parent_id in &decision.causal_parents {
            self.graph.add_edge(
                parent_id.to_string(),
                decision.id.to_string(),
                "CAUSED".to_string(),
                HashMap::new(),
            ).map_err(|e| super::MemoryLayerError::StorageError(e.to_string()))?;
        }

        Ok(())
    }

    async fn retrieve(&self, query: &super::MemoryQuery) -> Result<Vec<super::MemoryItem>, super::MemoryLayerError> {
        let mut results = Vec::new();
        let query_text = query.query_text.as_deref().unwrap_or("");

        // Search decisions by text
        if let Ok(similar) = self.graph.find_similar_decisions(query_text, query.limit).await {
            for decision in similar {
                results.push(super::MemoryItem {
                    key: decision.id.to_string(),
                    value: serde_json::json!({
                        "category": decision.category,
                        "scenario": decision.scenario,
                        "reasoning": decision.reasoning,
                        "outcome": decision.outcome,
                        "confidence": decision.confidence,
                    }),
                    metadata: super::LayerMetadata {
                        tenant_id: query.tenant_id,
                        tags: vec!["decision".to_string(), decision.category.clone()],
                        importance: decision.confidence,
                        source: "decision_intelligence".to_string(),
                        ..Default::default()
                    },
                    created_at: decision.created_at,
                    updated_at: decision.created_at,
                    access_count: 0,
                    last_accessed: None,
                });

                if results.len() >= query.limit {
                    break;
                }
            }
        }

        Ok(results)
    }

    async fn delete(&mut self, key: &str) -> Result<(), super::MemoryLayerError> {
        // Soft delete - mark as archived
        let node_id = key.to_string();
        self.graph.update_node(&node_id, vec![
            ("archived".to_string(), serde_json::Value::Bool(true)),
            ("archived_at".to_string(), serde_json::Value::String(chrono::Utc::now().to_rfc3339())),
        ].into_iter().collect()).map_err(|e| super::MemoryLayerError::StorageError(e.to_string()))?;
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
        Ok(0)
    }
}

impl DecisionMemory {
    pub async fn record_decision(
        &self,
        category: &str,
        scenario: &str,
        reasoning: &str,
        outcome: &str,
        confidence: f32,
        decision_maker: &str,
        metadata: Value,
        causal_parents: Vec<Uuid>,
    ) -> Result<Uuid, super::MemoryLayerError> {
        let decision_id = Uuid::new_v4();
        // Implementation would store decision
        Ok(decision_id)
    }

    pub async fn trace_decision_chain(&self, decision_id: Uuid) -> Result<Vec<Uuid>, super::MemoryLayerError> {
        Ok(vec![])
    }

    pub async fn find_similar_decisions(&self, query: &str, max_results: usize) -> Result<Vec<DecisionRecord>, super::MemoryLayerError> {
        Ok(vec![])
    }

    pub async fn check_decision_rules(&self, decision: &Value) -> Result<bool, super::MemoryLayerError> {
        Ok(true)
    }
}
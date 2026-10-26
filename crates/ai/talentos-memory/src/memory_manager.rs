//! Memory Manager - Unified interface for all 7 memory layers

use crate::layers::{
    MemoryLayer, LayerConfig, LayerMetadata, MemoryQuery, MemoryItem, LayerStats, LayerHealth, MemoryLayerError,
    WorkingMemory, WorkingMemoryConfig,
    EpisodicMemory, EpisodicMemoryConfig,
    SemanticMemory, SemanticMemoryConfig,
    ContextGraphMemory, ContextGraphConfig,
    DecisionMemory, DecisionMemoryConfig,
    OntologyMemory, OntologyMemoryConfig,
    TemporalMemory, TemporalMemoryConfig,
};
use crate::hindsight_integration::HindsightMemory;
use crate::semantica_integration::SemanticaMemory;
use crate::talentos_kernel::{TenantId, UserId, EntityId};
use crate::thai_hr_ontology::ThaiHROntology;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryManagerConfig {
    pub tenant_id: TenantId,
    pub working_memory: WorkingMemoryConfig,
    pub episodic_memory: EpisodicMemoryConfig,
    pub semantic_memory: SemanticMemoryConfig,
    pub context_graph: ContextGraphConfig,
    pub decision_memory: DecisionMemoryConfig,
    pub ontology_memory: OntologyMemoryConfig,
    pub temporal_memory: TemporalMemoryConfig,
    pub enable_cross_layer_queries: bool,
    pub cross_layer_weight: f32,
}

impl Default for MemoryManagerConfig {
    fn default() -> Self {
        Self {
            tenant_id: TenantId(Uuid::nil()),
            working_memory: WorkingMemoryConfig::default(),
            episodic_memory: EpisodicMemoryConfig::default(),
            semantic_memory: SemanticMemoryConfig::default(),
            context_graph: ContextGraphConfig::default(),
            decision_memory: DecisionMemoryConfig::default(),
            ontology_memory: OntologyMemoryConfig::default(),
            temporal_memory: TemporalMemoryConfig::default(),
            enable_cross_layer_queries: true,
            cross_layer_weight: 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryManagerStats {
    pub total_items: u64,
    pub total_size_bytes: u64,
    pub layer_stats: Vec<LayerStats>,
    pub cross_layer_hits: u64,
    pub cross_layer_misses: u64,
}

pub struct MemoryManager {
    config: MemoryManagerConfig,
    working_memory: WorkingMemory,
    episodic_memory: EpisodicMemory,
    semantic_memory: SemanticMemory,
    context_graph: ContextGraphMemory,
    decision_memory: DecisionMemory,
    ontology_memory: OntologyMemory,
    temporal_memory: TemporalMemory,
    hindsight_memory: HindsightMemory,
    semantica_memory: SemanticaMemory,
    thai_hr_ontology: ThaiHROntology,
    stats: Arc<RwLock<MemoryManagerStats>>,
    initialized: bool,
}

impl MemoryManager {
    pub fn new(config: MemoryManagerConfig) -> Self {
        let tenant_id = config.tenant_id;

        // Initialize all layers
        let working_memory = WorkingMemory::new(config.working_memory.clone());
        let episodic_memory = EpisodicMemory::new(config.episodic_memory.clone());
        let semantic_memory = SemanticMemory::new(config.semantic_memory.clone());
        let context_graph = ContextGraphMemory::new(config.context_graph.clone());
        let decision_memory = DecisionMemory::new(config.decision_memory.clone());
        let ontology_memory = OntologyMemory::new(config.ontology_memory.clone());
        let temporal_memory = TemporalMemory::new(config.temporal_memory.clone());

        // Initialize integrations
        let hindsight_config = crate::hindsight_integration::HindsightConfig {
            server_url: "http://localhost:8888".to_string(),
            default_bank_prefix: "talentos".to_string(),
            experience_bank_suffix: "experiences".to_string(),
            world_facts_bank_suffix: "world-facts".to_string(),
            auto_consolidate: true,
            consolidation_interval_hours: 6,
            api_key: None,
        };
        let hindsight_memory = HindsightMemory::new(hindsight_config, config.tenant_id);

        let semantica_config = crate::semantica_integration::SemanticaConfig::default();
        let semantica_memory = crate::semantica_integration::SemanticaMemory::new(semantica_config, config.tenant_id);

        let thai_hr_ontology = ThaiHROntology::new();

        Self {
            config,
            working_memory,
            episodic_memory,
            semantic_memory,
            context_graph,
            decision_memory,
            ontology_memory,
            temporal_memory,
            hindsight_memory,
            semantica_memory,
            thai_hr_ontology,
            stats: Arc::new(RwLock::new(MemoryManagerStats {
                total_items: 0,
                total_size_bytes: 0,
                layer_stats: Vec::new(),
                cross_layer_hits: 0,
                cross_layer_misses: 0,
            })),
            initialized: false,
        }
    }

    pub async fn initialize(&mut self) -> Result<(), MemoryError> {
        // Initialize all layers in order
        self.working_memory.initialize(self.config.working_memory.clone()).await?;
        self.episodic_memory.initialize(self.config.episodic_memory.clone()).await?;
        self.semantic_memory.initialize(self.config.semantic_memory.clone()).await?;
        self.context_graph.initialize(self.config.context_graph.clone()).await?;
        self.decision_memory.initialize(self.config.decision_memory.clone()).await?;
        self.ontology_memory.initialize(self.config.ontology_memory.clone()).await?;
        self.temporal_memory.initialize(self.config.temporal_memory.clone()).await?;

        // Initialize integrations
        self.hindsight_memory.initialize().await?;
        self.semantica_memory.initialize().await?;

        // Initialize Thai HR ontology
        self.thai_hr_ontology.initialize().await?;

        self.initialized = true;
        Ok(())
    }

    /// Store data across appropriate layers based on metadata
    pub async fn store(
        &mut self,
        key: &str,
        value: &Value,
        metadata: LayerMetadata,
    ) -> Result<(), MemoryError> {
        // Route to appropriate layers based on metadata tags
        let tags = &metadata.tags;

        // L1: Working memory - always store for immediate access
        if metadata.importance > 0.7 || tags.contains(&"working".to_string()) {
            self.working_memory.store(key, value, metadata.clone()).await?;
        }

        // L2: Episodic memory - for experiences
        if tags.contains(&"experience".to_string()) || tags.contains(&"interaction".to_string()) {
            self.episodic_memory.store(key, value, metadata.clone()).await?;
        }

        // L3: Semantic memory - for facts and rules
        if tags.contains(&"fact".to_string()) || tags.contains(&"rule".to_string()) {
            self.semantic_memory.store(key, value, metadata.clone()).await?;
        }

        // L4: Context graph - for entities and relationships
        if tags.contains(&"entity".to_string()) || tags.contains(&"relationship".to_string()) {
            self.context_graph.store(key, value, metadata.clone()).await?;
        }

        // L5: Decision memory - for decisions
        if tags.contains(&"decision".to_string()) {
            self.decision_memory.store(key, value, metadata.clone()).await?;
        }

        // L6: Ontology - for schema/vocabulary
        if tags.contains(&"ontology".to_string()) || tags.contains(&"vocabulary".to_string()) {
            self.ontology_memory.store(key, value, metadata.clone()).await?;
        }

        // L7: Temporal - for audit trail
        if tags.contains(&"temporal".to_string()) || tags.contains(&"audit".to_string()) {
            self.temporal_memory.store(key, value, metadata).await?;
        }

        Ok(())
    }

    /// Retrieve from all relevant layers with cross-layer synthesis
    pub async fn retrieve(&self, query: &MemoryQuery) -> Result<Vec<MemoryItem>, MemoryError> {
        let mut all_results = Vec::new();

        // Query each layer
        let layers: Vec<&dyn MemoryLayer<Config = _, Stats = _>> = vec![
            &self.working_memory,
            &self.episodic_memory,
            &self.semantic_memory,
            &self.context_graph,
            &self.decision_memory,
            &self.ontology_memory,
            &self.temporal_memory,
        ];

        for layer in layers {
            if layer.config().base.enabled {
                let results = layer.retrieve(query).await?;
                all_results.extend(results);
            }
        }

        // Cross-layer synthesis if enabled
        if self.config.enable_cross_layer_queries && all_results.len() > 1 {
            all_results = self.synthesize_cross_layer(all_results, query).await?;
        }

        // Sort by importance and relevance
        all_results.sort_by(|a, b| {
            b.metadata.importance.partial_cmp(&a.metadata.importance).unwrap_or(std::cmp::Ordering::Equal)
        });

        all_results.truncate(query.limit);
        Ok(all_results)
    }

    async fn synthesize_cross_layer(&self, mut results: Vec<MemoryItem>, query: &MemoryQuery) -> Result<Vec<MemoryItem>, MemoryError> {
        // Deduplicate by key
        let mut seen = std::collections::HashSet::new();
        results.retain(|item| seen.insert(item.key.clone()));

        // Boost scores for items found in multiple layers
        let mut score_map: HashMap<String, f32> = HashMap::new();
        for item in &results {
            *score_map.entry(item.key.clone()).or_insert(0.0) += item.metadata.importance;
        }

        // Re-sort by combined score
        results.sort_by(|a, b| {
            let score_a = score_map.get(&a.key).unwrap_or(&0.0);
            let score_b = score_map.get(&b.key).unwrap_or(&0.0);
            score_b.partial_cmp(score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(results)
    }

    /// Record a decision with full causal chain
    pub async fn record_decision(
        &self,
        category: &str,
        scenario: &str,
        reasoning: &str,
        outcome: &str,
        confidence: f32,
        decision_maker: &str,
        causal_parents: Vec<Uuid>,
    ) -> Result<Uuid, MemoryError> {
        self.decision_memory.record_decision(category, scenario, reasoning, outcome, confidence, decision_maker, causal_parents).await
    }

    /// Record an agent experience
    pub async fn record_experience(&self, content: &str, context: Option<String>, metadata: LayerMetadata) -> Result<(), MemoryError> {
        self.hindsight_memory.retain_experience(content, context, metadata).await
    }

    /// Search policies and regulations
    pub async fn search_policy(&self, query: &str, limit: usize) -> Result<Vec<MemoryItem>, MemoryError> {
        let hindsight_results = self.hindsight_memory.recall_world_fact(query, limit).await?;
        let semantic_results = self.semantic_memory.retrieve(&MemoryQuery {
            query_text: Some(query.to_string()),
            limit,
            ..Default::default()
        }).await?;

        let mut combined = hindsight_results;
        combined.extend(semantic_results);
        combined.truncate(limit);
        Ok(combined)
    }

    /// Get current state at a point in time (time travel)
    pub async fn get_state_at(&self, time: DateTime<Utc>) -> Result<HashMap<String, String>, MemoryError> {
        self.temporal_memory.state_at(time).await
    }

    /// Export audit trail as PROV-O
    pub async fn export_audit_trail(&self) -> Result<String, MemoryError> {
        self.temporal_memory.export_prov_o().await
    }

    /// Query using Allen interval algebra
    pub async fn query_allen(
        &self,
        entity: &str,
        relation: crate::layers::temporal_memory::AllenRelation,
        time_range: (DateTime<Utc>, DateTime<Utc>),
    ) -> Result<Vec<String>, MemoryError> {
        self.temporal_memory.query_allen(entity, relation, time_range).await
    }

    /// Get Thai HR concept definition
    pub fn get_thai_hr_concept(&self, concept: &str) -> Option<&crate::thai_hr_ontology::ThaiHRConcept> {
        self.thai_hr_ontology.get_concept(concept)
    }

    /// Validate data against Thai HR ontology
    pub async fn validate_thai_hr(&self, data: &Value) -> Result<ValidationResult, MemoryError> {
        self.thai_hr_ontology.validate(data)
    }

    pub async fn stats(&self) -> Result<MemoryManagerStats, MemoryError> {
        let mut all_stats = Vec::new();
        let layers: Vec<&dyn MemoryLayer<Config = _, Stats = _>> = vec![
            &self.working_memory,
            &self.episodic_memory,
            &self.semantic_memory,
            &self.context_graph,
            &self.decision_memory,
            &self.ontology_memory,
            &self.temporal_memory,
        ];

        for layer in layers {
            if layer.config().base.enabled {
                all_stats.push(layer.stats().await?);
            }
        }

        let stats = self.stats.read().await;
        Ok(MemoryManagerStats {
            total_items: all_stats.iter().map(|s| s.base.item_count).sum(),
            total_size_bytes: all_stats.iter().map(|s| s.base.size_bytes).sum(),
            layer_stats: all_stats,
            cross_layer_hits: stats.cross_layer_hits,
            cross_layer_misses: stats.cross_layer_misses,
        })
    }

    pub async fn health_check(&self) -> Result<HashMap<String, LayerHealth>, MemoryError> {
        let mut health = HashMap::new();
        let layers: Vec<(&str, &dyn MemoryLayer<Config = _, Stats = _>)> = vec![
            ("working", &self.working_memory),
            ("episodic", &self.episodic_memory),
            ("semantic", &self.semantic_memory),
            ("context_graph", &self.context_graph),
            ("decision", &self.decision_memory),
            ("ontology", &self.ontology_memory),
            ("temporal", &self.temporal_memory),
        ];

        for (name, layer) in layers {
            health.insert(name.to_string(), layer.health_check().await?);
        }
        Ok(health)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("Memory layer error: {0}")]
    LayerError(#[from] crate::layers::MemoryLayerError),

    #[error("Not initialized: {0}")]
    NotInitialized(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Query error: {0}")]
    QueryError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
    pub severity: ValidationSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWarning {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationSeverity {
    Error,
    Warning,
    Info,
}
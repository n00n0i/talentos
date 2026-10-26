//! Semantica Integration Module

use crate::layers::{
    ContextGraphMemory, ContextGraphConfig,
    DecisionMemory, DecisionMemoryConfig,
    OntologyMemory, OntologyMemoryConfig,
    TemporalMemory, TemporalMemoryConfig,
};
use crate::talentos_kernel::{TenantId, UserId, EntityId};
use semantica::context::ContextGraph;
use semantica::vector_store::VectorStore;
use semantica::temporal::TemporalGraph;
use semantica::provenance::ProvenanceManager;
use semantica::ontology::Ontology;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::talentos_kernel::{TenantId, EntityId};

/// Semantica configuration for TalentOS
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SemanticaConfig {
    pub context_graph: ContextGraphConfig,
    pub decision_memory: DecisionMemoryConfig,
    pub ontology_memory: OntologyMemoryConfig,
    pub temporal_memory: TemporalMemoryConfig,
    pub enable_context_graph: bool,
    pub enable_decision_intelligence: bool,
    pub enable_ontology: bool,
    pub enable_temporal: bool,
}

impl Default for SemanticaConfig {
    fn default() -> Self {
        Self {
            context_graph: ContextGraphConfig::default(),
            decision_memory: DecisionMemoryConfig::default(),
            ontology_memory: OntologyMemoryConfig::default(),
            temporal_memory: TemporalMemoryConfig::default(),
            enable_context_graph: true,
            enable_decision_intelligence: true,
            enable_ontology: true,
            enable_temporal: true,
        }
    }
}

/// Semantica memory wrapper for TalentOS
pub struct SemanticaMemory {
    config: SemanticaConfig,
    context_graph: Option<crate::layers::ContextGraphMemory>,
    decision_memory: Option<crate::layers::DecisionMemory>,
    ontology_memory: Option<crate::layers::OntologyMemory>,
    temporal_memory: Option<crate::layers::TemporalMemory>,
}

impl SemanticaMemory {
    pub fn new(config: SemanticaConfig, tenant_id: TenantId) -> Self {
        let mut context_graph_config = ContextGraphConfig::default();
        context_graph_config.base.layer_id = 4;
        let context_graph = if config.enable_context_graph {
            Some(crate::layers::ContextGraphMemory::new(context_graph_config))
        } else { None };

        let mut decision_config = DecisionMemoryConfig::default();
        decision_config.base.layer_id = 5;
        let decision_memory = if config.enable_decision_intelligence {
            Some(crate::layers::DecisionMemory::new(decision_config))
        } else { None };

        let mut ontology_config = OntologyMemoryConfig::default();
        ontology_config.base.layer_id = 6;
        let ontology_memory = if config.enable_ontology {
            Some(crate::layers::OntologyMemory::new(ontology_config))
        } else { None };

        let mut temporal_config = TemporalMemoryConfig::default();
        temporal_config.base.layer_id = 7;
        let temporal_memory = if config.enable_temporal {
            Some(crate::layers::TemporalMemory::new(temporal_config))
        } else { None };

        Self {
            config,
            context_graph,
            decision_memory,
            ontology_memory,
            temporal_memory,
        }
    }

    pub async fn initialize(&mut self) -> Result<(), crate::MemoryError> {
        if let Some(mem) = &mut self.context_graph {
            mem.initialize(mem.config.clone()).await?;
        }
        if let Some(mem) = &mut self.decision_memory {
            mem.initialize(mem.config.clone()).await?;
        }
        if let Some(mem) = &mut self.ontology_memory {
            mem.initialize(mem.config.clone()).await?;
        }
        if let Some(mem) = &mut self.temporal_memory {
            mem.initialize(mem.config.clone()).await?;
        }
        Ok(())
    }

    // Context Graph operations
    pub async fn add_entity(
        &self,
        id: &str,
        entity_type: &str,
        properties: serde_json::Value,
    ) -> Result<(), crate::MemoryError> {
        if let Some(mem) = &self.context_graph {
            mem.store(id, &properties, crate::layers::LayerMetadata {
                tags: vec!["entity".to_string()],
                source: "semantica".to_string(),
                ..Default::default()
            }).await
        } else {
            Err(crate::MemoryError::NotInitialized("Context graph not enabled".to_string()))
        }
    }

    pub async fn add_relationship(
        &self,
        from: &str,
        to: &str,
        relation_type: &str,
    ) -> Result<(), crate::MemoryError> {
        if let Some(mem) = &self.context_graph {
            mem.graph.add_edge(from.to_string(), to.to_string(), relation_type.to_string(), std::collections::HashMap::new())
                .map_err(|e| crate::MemoryError::StorageError(e.to_string()))?;
            Ok(())
        } else {
            Err(crate::MemoryError::NotInitialized("Context graph not enabled".to_string()))
        }
    }

    pub async fn search_entities(&self, query: &str, limit: usize) -> Result<Vec<crate::layers::MemoryItem>, crate::MemoryError> {
        if let Some(mem) = &self.context_graph {
            let query_obj = crate::layers::MemoryQuery {
                query_text: Some(query.to_string()),
                limit,
                ..Default::default()
            };
            mem.retrieve(&query_obj).await
        } else {
            Err(crate::MemoryError::NotInitialized("Context graph not enabled".to_string()))
        }
    }

    // Decision Intelligence operations
    pub async fn record_decision(
        &self,
        category: &str,
        scenario: &str,
        reasoning: &str,
        outcome: &str,
        confidence: f32,
        decision_maker: &str,
        causal_parents: Vec<uuid::Uuid>,
    ) -> Result<uuid::Uuid, crate::MemoryError> {
        if let Some(mem) = &self.decision_memory {
            mem.record_decision(category, scenario, reasoning, outcome, confidence, decision_maker, serde_json::Value::Object(serde_json::Map::new()), causal_parents).await
        } else {
            Err(crate::MemoryError::NotInitialized("Decision memory not enabled".to_string()))
        }
    }

    pub async fn trace_decision_chain(&self, decision_id: uuid::Uuid) -> Result<Vec<uuid::Uuid>, crate::MemoryError> {
        if let Some(mem) = &self.decision_memory {
            mem.trace_decision_chain(decision_id).await
        } else {
            Err(crate::MemoryError::NotInitialized("Decision memory not enabled".to_string()))
        }
    }

    // Ontology operations
    pub async fn validate_against_ontology(&self, data: &serde_json::Value) -> Result<bool, crate::MemoryError> {
        if let Some(mem) = &self.ontology_memory {
            // Would validate against SHACL shapes
            Ok(true)
        } else {
            Err(crate::MemoryError::NotInitialized("Ontology not enabled".to_string()))
        }
    }

    // Temporal operations
    pub async fn get_state_at(&self, time: chrono::DateTime<chrono::Utc>) -> Result<std::collections::HashMap<String, String>, crate::MemoryError> {
        if let Some(mem) = &self.temporal_memory {
            mem.state_at(time).await
        } else {
            Err(crate::MemoryError::NotInitialized("Temporal memory not enabled".to_string()))
        }
    }

    pub async fn export_audit_trail(&self) -> Result<String, crate::MemoryError> {
        if let Some(mem) = &self.temporal_memory {
            mem.export_prov_o().await
        } else {
            Err(crate::MemoryError::NotInitialized("Temporal memory not enabled".to_string()))
        }
    }
}
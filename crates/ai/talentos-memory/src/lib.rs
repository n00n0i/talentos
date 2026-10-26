//! TalentOS 7-Layered Memory System
//!
//! Integrates Hindsight (agent memory that learns) + Semantica (graph-native context infrastructure)
//! to provide a 7-layered memory system for AI agents.
//!
//! Layer Architecture:
//! L1: Working/Immediate Memory - Current task context, active conversation
//! L2: Episodic/Experience Memory - Hindsight Experience Pathway (agent's own experiences)
//! L3: Semantic/World Facts - Hindsight World Facts + Semantica KG (domain knowledge)
//! L4: Context Graph - Semantica Context Graph (entities, decisions, causal relationships)
//! L5: Decision Intelligence - Semantica Decision Intelligence (recorded decisions with causal chains)
//! L6: Ontology/Schema - Semantica Ontology (OWL/SHACL Thai HR ontology)
//! L7: Temporal/Provenance - Hindsight Temporal + Semantica Bi-temporal (time-travel, audit trails)

pub mod layers;
pub mod hindsight_integration;
pub mod semantica_integration;
pub mod memory_manager;
pub mod thai_hr_ontology;
pub mod memory_manager_trait;

pub use layers::{
    MemoryLayer, LayerConfig, LayerStats,
    WorkingMemory, EpisodicMemory, SemanticMemory,
    ContextGraphMemory, DecisionMemory, OntologyMemory, TemporalMemory,
};
pub use memory_manager::{MemoryManager, MemoryManagerConfig, MemoryManagerStats};
pub use memory_manager_trait::{MemoryManagerTrait, MemoryQuery, MemoryResponse};
pub use thai_hr_ontology::{ThaiHROntology, ThaiHRConcept, ThaiHRRelation};

/// Re-exports for convenience
pub use hindsight_integration::{HindsightConfig, HindsightMemory};
pub use semantica_integration::{SemanticaConfig, SemanticaMemory};
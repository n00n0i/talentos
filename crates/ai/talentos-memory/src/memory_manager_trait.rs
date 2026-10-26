//! Memory Manager Trait - Unified interface for memory operations

use async_trait::async_trait;
use crate::layers::{MemoryQuery, MemoryItem, LayerMetadata};
use crate::talentos_kernel::{TenantId, UserId, EntityId};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;

#[async_trait]
pub trait MemoryManagerTrait: Send + Sync {
    /// Store data in memory
    async fn store(
        &mut self,
        key: &str,
        value: &Value,
        metadata: LayerMetadata,
    ) -> Result<(), MemoryError>;

    /// Retrieve from memory
    async fn retrieve(&self, query: &MemoryQuery) -> Result<Vec<MemoryItem>, MemoryError>;

    /// Delete from memory
    async fn delete(&mut self, key: &str) -> Result<(), MemoryError>;

    /// Record a decision with causal chain
    async fn record_decision(
        &self,
        category: &str,
        scenario: &str,
        reasoning: &str,
        outcome: &str,
        confidence: f32,
        decision_maker: &str,
        causal_parents: Vec<Uuid>,
    ) -> Result<Uuid, MemoryError>;

    /// Record an agent experience
    async fn record_experience(&self, content: &str, context: Option<String>, metadata: LayerMetadata) -> Result<(), MemoryError>;

    /// Search policies and regulations
    async fn search_policy(&self, query: &str, limit: usize) -> Result<Vec<MemoryItem>, MemoryError>;

    /// Get state at a point in time (time travel)
    async fn get_state_at(&self, time: DateTime<Utc>) -> Result<HashMap<String, String>, MemoryError>;

    /// Export audit trail as PROV-O
    async fn export_audit_trail(&self) -> Result<String, MemoryError>;

    /// Query using Allen interval algebra
    async fn query_allen(
        &self,
        entity: &str,
        relation: crate::layers::temporal_memory::AllenRelation,
        time_range: (chrono::DateTime<Utc>, chrono::DateTime<Utc>),
    ) -> Result<Vec<String>, MemoryError>;

    /// Validate against Thai HR ontology
    async fn validate_thai_hr(&self, data: &Value) -> Result<ValidationResult, MemoryError>;

    /// Get statistics
    async fn stats(&self) -> Result<MemoryManagerStats, MemoryError>;

    /// Health check
    async fn health_check(&self) -> Result<HashMap<String, crate::layers::LayerHealth>, MemoryError>;
}

use crate::layers::MemoryQuery;
use crate::layers::MemoryItem;
use crate::layers::LayerMetadata;
use crate::layers::LayerHealth;
use crate::MemoryError;
use crate::MemoryManagerStats;
use crate::ValidationResult;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
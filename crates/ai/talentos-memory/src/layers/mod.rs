//! 7-Layer Memory System - Layer Definitions

pub mod working_memory;
pub mod episodic_memory;
pub mod semantic_memory;
pub mod context_graph_memory;
pub mod decision_memory;
pub mod ontology_memory;
pub mod temporal_memory;

pub use working_memory::{WorkingMemory, WorkingMemoryConfig, WorkingMemoryStats};
pub use episodic_memory::{EpisodicMemory, EpisodicMemoryConfig, EpisodicMemoryStats};
pub use semantic_memory::{SemanticMemory, SemanticMemoryConfig, SemanticMemoryStats};
pub use context_graph_memory::{ContextGraphMemory, ContextGraphConfig, ContextGraphStats};
pub use decision_memory::{DecisionMemory, DecisionMemoryConfig, DecisionMemoryStats};
pub use ontology_memory::{OntologyMemory, OntologyMemoryConfig, OntologyMemoryStats};
pub use temporal_memory::{TemporalMemory, TemporalMemoryConfig, TemporalMemoryStats};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde_json::Value;
use crate::talentos_kernel::{TenantId, UserId, EntityId};

/// Trait that all memory layers must implement
#[async_trait::async_trait]
pub trait MemoryLayer: Send + Sync {
    type Config: Send + Sync;
    type Stats: Send + Sync;

    /// Layer identifier (L1-L7)
    fn layer_id(&self) -> u8;

    /// Layer name
    fn layer_name(&self) -> &'static str;

    /// Initialize the layer with configuration
    async fn initialize(&mut self, config: Self::Config) -> Result<(), MemoryLayerError>;

    /// Store data in this layer
    async fn store(&mut self, key: &str, value: &Value, metadata: LayerMetadata) -> Result<(), MemoryLayerError>;

    /// Retrieve data from this layer
    async fn retrieve(&self, query: &MemoryQuery) -> Result<Vec<MemoryItem>, MemoryLayerError>;

    /// Delete data from this layer
    async fn delete(&mut self, key: &str) -> Result<(), MemoryLayerError>;

    /// Get layer statistics
    async fn stats(&self) -> Result<Self::Stats, MemoryLayerError>;

    /// Health check
    async fn health_check(&self) -> Result<LayerHealth, MemoryLayerError>;

    /// Cleanup old/expired data
    async fn cleanup(&mut self, older_than: DateTime<Utc>) -> Result<u64, MemoryLayerError>;
}

/// Configuration for a memory layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerConfig {
    pub layer_id: u8,
    pub enabled: bool,
    pub max_size_bytes: Option<u64>,
    pub ttl_seconds: Option<u64>,
    pub compression: bool,
    pub encryption: bool,
    pub custom: Value,
}

impl Default for LayerConfig {
    fn default() -> Self {
        Self {
            layer_id: 0,
            enabled: true,
            max_size_bytes: None,
            ttl_seconds: None,
            compression: false,
            encryption: false,
            custom: Value::Object(serde_json::Map::new()),
        }
    }
}

/// Metadata for stored items
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerMetadata {
    pub tenant_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub entity_id: Option<Uuid>,
    pub tags: Vec<String>,
    pub importance: f32, // 0.0 - 1.0
    pub expires_at: Option<chrono::DateTime<Utc>>,
    pub source: String, // "agent", "user", "system", "tool"
    pub custom: Value,
}

impl Default for LayerMetadata {
    fn default() -> Self {
        Self {
            tenant_id: None,
            user_id: None,
            entity_id: None,
            tags: Vec::new(),
            importance: 0.5,
            expires_at: None,
            source: "system".to_string(),
            custom: Value::Object(serde_json::Map::new()),
        }
    }
}

/// Query for retrieving memory items
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryQuery {
    pub query_text: Option<String>,
    pub tenant_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub entity_id: Option<Uuid>,
    pub tags: Vec<String>,
    pub time_range: Option<(chrono::DateTime<Utc>, chrono::DateTime<Utc>)>,
    pub limit: usize,
    pub offset: usize,
    pub min_importance: f32,
    pub layer_specific: Value,
}

impl Default for MemoryQuery {
    fn default() -> Self {
        Self {
            query_text: None,
            tenant_id: None,
            user_id: None,
            entity_id: None,
            tags: Vec::new(),
            time_range: None,
            limit: 100,
            offset: 0,
            min_importance: 0.0,
            layer_specific: Value::Object(serde_json::Map::new()),
        }
    }
}

/// A memory item returned from a layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    pub key: String,
    pub value: Value,
    pub metadata: LayerMetadata,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub access_count: u64,
    pub last_accessed: Option<DateTime<Utc>>,
}

/// Layer health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayerHealth {
    Healthy,
    Degraded(String),
    Unhealthy(String),
}

/// Layer statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerStats {
    pub layer_id: u8,
    pub layer_name: String,
    pub item_count: u64,
    pub size_bytes: u64,
    pub hit_rate: f32,
    pub avg_latency_ms: f32,
    pub last_cleanup: Option<DateTime<Utc>>,
    pub errors_last_hour: u64,
}

/// Error type for memory layer operations
#[derive(Debug, thiserror::Error)]
pub enum MemoryLayerError {
    #[error("Layer not initialized: {0}")]
    NotInitialized(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Query error: {0}")]
    QueryError(String),

    #[error("Layer disabled: {0}")]
    LayerDisabled(String),

    #[error("Capacity exceeded: {0}")]
    CapacityExceeded(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<anyhow::Error> for MemoryLayerError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
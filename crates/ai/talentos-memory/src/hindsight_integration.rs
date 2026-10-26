//! Hindsight Integration Module

use crate::layers::EpisodicMemoryConfig;
use crate::layers::EpisodicMemory;
use crate::layers::SemanticMemoryConfig;
use crate::layers::SemanticMemory;
use crate::talentos_kernel::{TenantId, UserId, EntityId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Hindsight configuration for TalentOS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HindsightConfig {
    pub server_url: String,
    pub api_key: Option<String>,
    pub default_bank_prefix: String,
    pub experience_bank_suffix: String,
    pub world_facts_bank_suffix: String,
    pub auto_consolidate: bool,
    pub consolidation_interval_hours: u64,
}

impl Default for HindsightConfig {
    fn default() -> Self {
        Self {
            server_url: "http://localhost:8888".to_string(),
            api_key: None,
            default_bank_prefix: "talentos".to_string(),
            experience_bank_suffix: "experiences".to_string(),
            world_facts_bank_suffix: "world-facts".to_string(),
            auto_consolidate: true,
            consolidation_interval_hours: 6,
        }
    }
}

/// Hindsight memory wrapper for TalentOS
pub struct HindsightMemory {
    config: HindsightConfig,
    experience_memory: EpisodicMemory,
    semantic_memory: SemanticMemory,
}

impl HindsightMemory {
    pub fn new(config: HindsightConfig, tenant_id: TenantId) -> Self {
        let experience_config = EpisodicMemoryConfig {
            base: super::layers::LayerConfig {
                layer_id: 2,
                enabled: true,
                max_size_bytes: Some(10 * 1024 * 1024 * 1024),
                ttl_seconds: None,
                compression: true,
                encryption: true,
                custom: serde_json::Value::Object(serde_json::Map::new()),
            },
            hindsight_server_url: config.server_url.clone(),
            bank_id: format!("{}-{}-{}", config.default_bank_prefix, tenant_id, config.experience_bank_suffix),
            auto_consolidate: config.auto_consolidate,
            consolidation_interval_hours: config.consolidation_interval_hours,
            max_items_per_user: 100000,
        };

        let semantic_config = SemanticMemoryConfig {
            base: super::layers::LayerConfig {
                layer_id: 3,
                enabled: true,
                max_size_bytes: Some(5 * 1024 * 1024 * 1024),
                ttl_seconds: None,
                compression: true,
                encryption: true,
                custom: serde_json::Value::Object(serde_json::Map::new()),
            },
            hindsight_server_url: config.server_url.clone(),
            hindsight_bank_id: format!("{}-{}-{}", config.default_bank_prefix, tenant_id, config.world_facts_bank_suffix),
            semantica_graph: semantica::context::ContextGraph::new(),
            enable_auto_ingest: true,
            ingest_batch_size: 100,
        };

        Self {
            config,
            experience_memory: EpisodicMemory::new(experience_config),
            semantic_memory: SemanticMemory::new(semantic_config),
        }
    }

    pub async fn initialize(&mut self) -> Result<(), crate::MemoryError> {
        self.experience_memory.initialize(self.experience_memory.config.clone()).await?;
        self.semantic_memory.initialize(self.semantic_memory.config.clone()).await?;
        Ok(())
    }

    pub async fn retain_experience(
        &self,
        content: &str,
        context: Option<String>,
        metadata: crate::layers::LayerMetadata,
    ) -> Result<(), crate::MemoryError> {
        self.experience_memory.store("experience", &serde_json::json!({ "content": content }), metadata).await
    }

    pub async fn recall_experience(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<crate::layers::MemoryItem>, crate::MemoryError> {
        let query = crate::layers::MemoryQuery {
            query_text: Some(query.to_string()),
            limit,
            ..Default::default()
        };
        self.experience_memory.retrieve(&query).await
    }

    pub async fn retain_world_fact(
        &self,
        key: &str,
        content: &str,
        metadata: crate::layers::LayerMetadata,
    ) -> Result<(), crate::MemoryError> {
        self.semantic_memory.store(key, &serde_json::json!({ "content": content }), metadata).await
    }

    pub async fn recall_world_fact(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<crate::layers::MemoryItem>, crate::MemoryError> {
        let query_obj = crate::layers::MemoryQuery {
            query_text: Some(query.to_string()),
            limit,
            ..Default::default()
        };
        self.semantic_memory.retrieve(&query_obj).await
    }
}
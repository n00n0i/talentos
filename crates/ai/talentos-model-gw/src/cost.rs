use serde::{Deserialize, Serialize};

/// Token usage tracking for cost calculation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cached_tokens: u64,
}

impl TokenUsage {
    /// Calculate estimated cost in USD
    pub fn estimated_cost_usd(&self, model: &str) -> f64 {
        // Example pricing per 1M tokens
        let (input_per_m, output_per_m) = match model {
            "qwen2.5-72b-instruct" => (0.50, 1.50),
            "gpt-4o-mini" => (0.15, 0.60),
            "text-embedding-3-large" => (0.13, 0.0),
            _ => (0.50, 1.50),
        };

        let input_cost = (self.prompt_tokens as f64 / 1_000_000.0) * input_per_m;
        let output_cost = (self.completion_tokens as f64 / 1_000_000.0) * output_per_m;
        
        input_cost + output_cost
    }
}

/// Cost estimate for a request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    pub estimated_input_tokens: usize,
    pub estimated_output_tokens: usize,
    pub estimated_cost_usd: f64,
    pub model: String,
}

/// Daily cost tracking per tenant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyCost {
    pub tenant_id: uuid::Uuid,
    pub date: chrono::NaiveDate,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub cost_usd: f64,
}

impl DailyCost {
    pub fn new(tenant_id: uuid::Uuid, model: String, usage: &TokenUsage) -> Self {
        let cost_usd = usage.estimated_cost_usd(&model);
        Self {
            tenant_id,
            date: chrono::Utc::now().date_naive(),
            model,
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
            cached_tokens: usage.cached_tokens,
            cost_usd,
        }
    }
}
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fred::prelude::*;
use reqwest::{Client as HttpClient, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::{cost::TokenUsage, error::ModelError, models::*};

/// Configuration for the model gateway
#[derive(Clone, Debug)]
pub struct GatewayConfig {
    pub base_url: String,
    pub api_key: String,
    pub primary_model: String,
    pub fallback_model: String,
    pub embedding_model: String,
    pub rate_limit_rpm: u32,
    pub rate_limit_burst: u32,
    pub rate_limit_window_secs: u64,
    pub timeout_seconds: u64,
    pub circuit_breaker_threshold: f64, // e.g., 0.1 = 10%
    pub circuit_breaker_window_secs: u64,
    pub circuit_breaker_half_open_after_secs: u64,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            base_url: "https://llmapi.huakati.online/v1".to_string(),
            api_key: std::env::var("HUAKATI_API_KEY").unwrap_or_default(),
            primary_model: "qwen2.5-72b-instruct".to_string(),
            fallback_model: "gpt-4o-mini".to_string(),
            embedding_model: "text-embedding-3-large".to_string(),
            rate_limit_rpm: 60,
            rate_limit_burst: 10,
            rate_limit_window_secs: 60,
            timeout_seconds: 30,
            circuit_breaker_threshold: 0.1,
            circuit_breaker_window_secs: 60,
            circuit_breaker_half_open_after_secs: 30,
        }
    }
}

/// Rate limiter using token bucket algorithm with Redis backend
struct RateLimiter {
    redis: Client,
    tenant_id: String,
    limit: u32,
    window_secs: u64,
    burst: u32,
}

impl RateLimiter {
    fn new(redis: Client, tenant_id: String, limit: u32, burst: u32, window_secs: u64) -> Self {
        Self {
            redis,
            tenant_id,
            limit,
            window_secs,
            burst,
        }
    }

    async fn check_limit(&self) -> Result<bool, ModelError> {
        let key = format!("ratelimit:{}:{}", self.tenant_id, chrono::Utc::now().timestamp() / self.window_secs as i64);
        
        // Use Redis INCR with expiry for sliding window
        let count: i64 = self.redis.incr(&key, 1).await?;
        
        if count == 1 {
            self.redis.expire(&key, self.window_secs as i64, None).await?;
        }
        
        Ok(count <= self.limit as i64)
    }
}

/// Circuit breaker state
#[derive(Clone, Copy, Debug, PartialEq)]
enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// Circuit breaker for model failures
struct CircuitBreaker {
    state: Mutex<CircuitState>,
    failure_count: Mutex<u32>,
    success_count: Mutex<u32>,
    last_failure: Mutex<Option<Instant>>,
    threshold: f64,
    window_secs: u64,
    half_open_after: Duration,
}

impl CircuitBreaker {
    fn new(threshold: f64, window_secs: u64, half_open_after_secs: u64) -> Self {
        Self {
            state: Mutex::new(CircuitState::Closed),
            failure_count: Mutex::new(0),
            success_count: Mutex::new(0),
            last_failure: Mutex::new(None),
            threshold,
            window_secs,
            half_open_after: Duration::from_secs(half_open_after_secs),
        }
    }

    async fn record_success(&self) {
        let mut state = self.state.lock().await;
        let mut success_count = self.success_count.lock().await;
        let mut failure_count = self.failure_count.lock().await;
        
        *success_count += 1;
        
        if *state == CircuitState::HalfOpen && *success_count >= 2 {
            *state = CircuitState::Closed;
            *failure_count = 0;
            *success_count = 0;
            debug!("Circuit breaker closed after successful probes");
        }
    }

    async fn record_failure(&self) {
        let mut state = self.state.lock().await;
        let mut failure_count = self.failure_count.lock().await;
        let mut last_failure = self.last_failure.lock().await;
        
        *failure_count += 1;
        *last_failure = Some(Instant::now());
        
        let total = *failure_count + *self.success_count.lock().await;
        if total > 0 && (*failure_count as f64 / total as f64) > self.threshold {
            *state = CircuitState::Open;
            warn!("Circuit breaker opened due to failure rate");
        }
    }

    async fn can_execute(&self) -> bool {
        let mut state = self.state.lock().await;
        let last_failure = self.last_failure.lock().await;
        
        match *state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if let Some(lf) = *last_failure {
                    if lf.elapsed() > self.half_open_after {
                        *state = CircuitState::HalfOpen;
                        *self.success_count.lock().await = 0;
                        debug!("Circuit breaker half-open, allowing probe request");
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }
}

/// Main client for huakati.online API
pub struct HuakatiClient {
    http: HttpClient,
    config: GatewayConfig,
    rate_limiter: Option<RateLimiter>,
    circuit_breaker: Arc<CircuitBreaker>,
    redis: Option<Client>,
}

impl HuakatiClient {
    /// Create a new client with default config (requires HUAKATI_API_KEY env var)
    pub fn new() -> Result<Self, ModelError> {
        let config = GatewayConfig::default();
        Self::with_config(config)
    }

    /// Create a new client with custom config
    pub fn with_config(config: GatewayConfig) -> Result<Self, ModelError> {
        if config.api_key.is_empty() {
            return Err(ModelError::ConfigError("HUAKATI_API_KEY not set".to_string()));
        }

        let http = HttpClient::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()?;

        // Initialize Redis for rate limiting if available
        let redis_url = std::env::var("REDIS_URL").ok();
        let (redis, rate_limiter) = if let Some(url) = redis_url {
            let client = Client::new(Config::from_url(&url)?);
            client.connect().await?;
            let limiter = RateLimiter::new(
                client.clone(),
                "default".to_string(), // Will be overridden per-request
                config.rate_limit_rpm,
                config.rate_limit_burst,
                config.rate_limit_window_secs,
            );
            (Some(client), Some(limiter))
        } else {
            (None, None)
        };

        let circuit_breaker = Arc::new(CircuitBreaker::new(
            config.circuit_breaker_threshold,
            config.circuit_breaker_window_secs,
            config.circuit_breaker_half_open_after_secs,
        ));

        Ok(Self {
            http,
            config,
            rate_limiter,
            circuit_breaker,
            redis,
        })
    }

    /// Create a client with a specific tenant for rate limiting
    pub fn for_tenant(&self, tenant_id: &str) -> Self {
        let mut client = self.clone();
        if let Some(redis) = &self.redis {
            client.rate_limiter = Some(RateLimiter::new(
                redis.clone(),
                tenant_id.to_string(),
                self.config.rate_limit_rpm,
                self.config.rate_limit_burst,
                self.config.rate_limit_window_secs,
            ));
        }
        client
    }

    /// Single-turn chat with optional function calling
    pub async fn chat(
        &self,
        tenant_id: &str,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDef>>,
    ) -> Result<ChatResponse, ModelError> {
        // Check rate limit
        if let Some(limiter) = &self.rate_limiter {
            if !limiter.check_limit().await? {
                return Err(ModelError::RateLimited("Rate limit exceeded".to_string()));
            }
        }

        // Check circuit breaker
        if !self.circuit_breaker.can_execute().await {
            // Try fallback model
            return self.chat_fallback(tenant_id, messages, tools).await;
        }

        let request = ChatRequest {
            model: self.config.primary_model.clone(),
            messages,
            tools,
            stream: false,
            temperature: Some(0.7),
            max_tokens: Some(4096),
        };

        let response = self.make_chat_request(&request).await;
        
        match response {
            Ok(resp) => {
                self.circuit_breaker.record_success().await;
                Ok(resp)
            }
            Err(e) => {
                self.circuit_breaker.record_failure().await;
                
                // Check if we should try fallback
                if matches!(e, ModelError::ApiError { status: 500..=599, .. } | ModelError::Timeout) {
                    self.chat_fallback(tenant_id, messages, tools).await
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn chat_fallback(
        &self,
        tenant_id: &str,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDef>>,
    ) -> Result<ChatResponse, ModelError> {
        debug!("Attempting fallback model: {}", self.config.fallback_model);
        
        // Try Ollama if configured
        if let Ok(ollama_url) = std::env::var("HUAKATI_LOCAL_OLLAMA_URL") {
            return self.chat_ollama(&ollama_url, tenant_id, messages, tools).await;
        }
        
        // Try fallback model on huakati
        let request = ChatRequest {
            model: self.config.fallback_model.clone(),
            messages,
            tools,
            stream: false,
            temperature: Some(0.7),
            max_tokens: Some(4096),
        };
        
        let response = self.make_chat_request(&request).await;
        
        match response {
            Ok(resp) => {
                self.circuit_breaker.record_success().await;
                Ok(resp)
            }
            Err(e) => {
                self.circuit_breaker.record_failure().await;
                Err(ModelError::ModelUnavailable(format!("All models failed: {}", e)))
            }
        }
    }

    async fn chat_ollama(
        &self,
        ollama_url: &str,
        _tenant_id: &str,
        messages: Vec<Message>,
        _tools: Option<Vec<ToolDef>>,
    ) -> Result<ChatResponse, ModelError> {
        let request = OllamaChatRequest {
            model: "qwen2.5:72b".to_string(),
            messages: messages.into_iter().map(|m| OllamaMessage {
                role: m.role,
                content: m.content,
            }).collect(),
            stream: false,
        };
        
        let resp = self.http
            .post(&format!("{}/api/chat", ollama_url))
            .json(&request)
            .send()
            .await?;
        
        if !resp.status().is_success() {
            let message = resp.text().await.unwrap_or_else(|_| "Failed to read error response".to_string());
            return Err(ModelError::ApiError {
                status: resp.status().as_u16(),
                message,
            });
        }
        
        let ollama_resp: OllamaChatResponse = resp.json().await?;
        
        Ok(ChatResponse {
            id: uuid::Uuid::new_v4().to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp() as u64,
            model: "qwen2.5:72b".to_string(),
            choices: vec![ChatChoice {
                index: 0,
                message: Message {
                    role: ollama_resp.message.role,
                    content: ollama_resp.message.content,
                },
                finish_reason: Some(ollama_resp.done_reason.unwrap_or("stop".to_string())),
            }],
            usage: TokenUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                cached_tokens: 0,
            },
        })
    }

    /// Generate embedding vector (1536 dims)
    pub async fn embed(&self, tenant_id: &str, text: &str) -> Result<Vec<f32>, ModelError> {
        if let Some(limiter) = &self.rate_limiter {
            if !limiter.check_limit().await? {
                return Err(ModelError::RateLimited("Rate limit exceeded".to_string()));
            }
        }

        let request = EmbeddingRequest {
            model: self.config.embedding_model.clone(),
            input: text.to_string(),
        };

        let url = format!("{}/embeddings", self.config.base_url);
        let resp = self.http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ModelError::ApiError {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let embed_resp: EmbeddingResponse = resp.json().await?;
        
        Ok(embed_resp.data.first().map(|d| d.embedding.clone()).unwrap_or_default())
    }

    /// Make chat request to huakati API
    async fn make_chat_request(&self, request: &ChatRequest) -> Result<ChatResponse, ModelError> {
        let url = format!("{}/chat/completions", self.config.base_url);
        
        let resp = self.http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let message = resp.text().await.unwrap_or_else(|_| "Failed to read error response".to_string());
            return Err(ModelError::ApiError { status: status.as_u16(), message });
        }

        let chat_resp: ChatResponse = resp.json().await?;
        Ok(chat_resp)
    }

    /// Get token counts and estimated cost for a request
    pub fn estimate_cost(&self, input_tokens: usize, output_tokens: usize) -> TokenUsage {
        // Pricing per 1M tokens (example rates)
        let input_cost_per_m = 0.50;  // $0.50 per 1M input tokens
        let output_cost_per_m = 1.50; // $1.50 per 1M output tokens
        
        let input_cost = (input_tokens as f64 / 1_000_000.0) * input_cost_per_m;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * output_cost_per_m;
        
        TokenUsage {
            prompt_tokens: input_tokens as u64,
            completion_tokens: output_tokens as u64,
            total_tokens: (input_tokens + output_tokens) as u64,
            cached_tokens: 0,
        }
    }

    /// Records actual token usage for cost tracking
    pub async fn record_usage(
        &self,
        tenant_id: &str,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        cached_tokens: u64,
    ) -> Result<(), ModelError> {
        // In a real implementation, this would write to the ai_cost_tracking table
        debug!(
            tenant_id = tenant_id,
            model = model,
            input_tokens = input_tokens,
            output_tokens = output_tokens,
            cached_tokens = cached_tokens,
            "Recording token usage"
        );
        Ok(())
    }
}

impl Clone for HuakatiClient {
    fn clone(&self) -> Self {
        Self {
            http: self.http.clone(),
            config: self.config.clone(),
            rate_limiter: None, // Will be recreated per-tenant
            circuit_breaker: self.circuit_breaker.clone(),
            redis: self.redis.clone(),
        }
    }
}

// --- Request/Response types ---

/// Chat completion request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// Chat completion response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: TokenUsage,
}

/// Chat choice
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: u32,
    pub message: Message,
    pub finish_reason: Option<String>,
}

/// Message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// Tool call in a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String,
    pub function: FunctionCall,
}

/// Function call details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Tool definition for function calling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub r#type: String,
    pub function: FunctionDef,
}

/// Function definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Embedding request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: String,
}

/// Embedding response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    pub object: String,
    pub data: Vec<EmbeddingData>,
    pub model: String,
    pub usage: TokenUsage,
}

/// Embedding data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingData {
    pub object: String,
    pub embedding: Vec<f32>,
    pub index: u32,
}

// Ollama types for fallback
#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: OllamaMessage,
    done: bool,
    done_reason: Option<String>,
}
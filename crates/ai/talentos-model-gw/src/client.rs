//! huakati.online (OpenAI-compatible) client with in-process rate limiting,
//! a circuit breaker, and an Ollama fallback for on-prem deployments.
//!
//! Per-tenant/edge rate limiting lives in the API gateway; this limiter only
//! protects the upstream provider from a single hot process.

use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Client as HttpClient;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::cost::TokenUsage;
use crate::error::ModelError;
use crate::models::*;

/// Runtime configuration for the model gateway.
#[derive(Clone, Debug)]
pub struct GatewayConfig {
    pub base_url: String,
    pub api_key: String,
    pub primary_model: String,
    pub fallback_model: String,
    pub embedding_model: String,
    pub rate_limit_rpm: u32,
    pub rate_limit_burst: u32,
    pub timeout_seconds: u64,
    pub circuit_breaker_threshold: f64,
    pub circuit_breaker_window_secs: u64,
    pub circuit_breaker_half_open_after_secs: u64,
    pub ollama_url: Option<String>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            base_url: std::env::var("HUAKATI_API_BASE")
                .unwrap_or_else(|_| "https://llmapi.huakati.online/v1".to_string()),
            api_key: std::env::var("HUAKATI_API_KEY").unwrap_or_default(),
            primary_model: "qwen2.5-72b-instruct".to_string(),
            fallback_model: "gpt-4o-mini".to_string(),
            embedding_model: "text-embedding-3-large".to_string(),
            rate_limit_rpm: 60,
            rate_limit_burst: 10,
            timeout_seconds: 30,
            circuit_breaker_threshold: 0.1,
            circuit_breaker_window_secs: 60,
            circuit_breaker_half_open_after_secs: 30,
            ollama_url: std::env::var("HUAKATI_LOCAL_OLLAMA_URL").ok(),
        }
    }
}

/// Sliding-window counter, in-process. Cheap and lock-contended only per call.
struct RateLimiter {
    limit: u32,
    window: Duration,
    state: Mutex<(Instant, u32)>,
}

impl RateLimiter {
    fn new(limit: u32, window_secs: u64) -> Self {
        Self {
            limit,
            window: Duration::from_secs(window_secs),
            state: Mutex::new((Instant::now(), 0)),
        }
    }

    async fn check(&self) -> bool {
        let mut state = self.state.lock().await;
        let (window_start, count) = &mut *state;
        if window_start.elapsed() >= self.window {
            *window_start = Instant::now();
            *count = 0;
        }
        if *count >= self.limit {
            return false;
        }
        *count += 1;
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

struct CircuitBreaker {
    state: Mutex<CircuitState>,
    failures: Mutex<u32>,
    last_failure: Mutex<Option<Instant>>,
    threshold: f64,
    half_open_after: Duration,
}

impl CircuitBreaker {
    fn new(threshold: f64, half_open_after_secs: u64) -> Self {
        Self {
            state: Mutex::new(CircuitState::Closed),
            failures: Mutex::new(0),
            last_failure: Mutex::new(None),
            threshold,
            half_open_after: Duration::from_secs(half_open_after_secs),
        }
    }

    async fn allow(&self) -> bool {
        let mut state = self.state.lock().await;
        match *state {
            CircuitState::Closed | CircuitState::HalfOpen => true,
            CircuitState::Open => {
                let last = *self.last_failure.lock().await;
                match last {
                    Some(t) if t.elapsed() >= self.half_open_after => {
                        *state = CircuitState::HalfOpen;
                        debug!("circuit breaker half-open, allowing probe");
                        true
                    }
                    _ => false,
                }
            }
        }
    }

    async fn record_success(&self) {
        let mut state = self.state.lock().await;
        *self.failures.lock().await = 0;
        if *state == CircuitState::HalfOpen {
            *state = CircuitState::Closed;
            debug!("circuit breaker closed");
        }
    }

    async fn record_failure(&self) {
        let mut failures = self.failures.lock().await;
        *failures += 1;
        *self.last_failure.lock().await = Some(Instant::now());
        // Treat `threshold` as a failure count within the window.
        let trip_at = (1.0 / self.threshold.max(0.01)).ceil() as u32;
        if *failures >= trip_at {
            *self.state.lock().await = CircuitState::Open;
            warn!(failures = *failures, "circuit breaker opened");
        }
    }
}

/// Client for the huakati.online OpenAI-compatible API.
#[derive(Clone)]
pub struct HuakatiClient {
    http: HttpClient,
    config: Arc<GatewayConfig>,
    rate_limiter: Arc<RateLimiter>,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl HuakatiClient {
    /// Create from environment (`HUAKATI_API_KEY` required).
    pub fn new() -> Result<Self, ModelError> {
        Self::with_config(GatewayConfig::default())
    }

    pub fn with_config(config: GatewayConfig) -> Result<Self, ModelError> {
        if config.api_key.is_empty() {
            return Err(ModelError::ConfigError("HUAKATI_API_KEY not set".into()));
        }
        let http = HttpClient::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()?;
        let rate_limiter = Arc::new(RateLimiter::new(
            config.rate_limit_rpm,
            config.circuit_breaker_window_secs.max(1),
        ));
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            config.circuit_breaker_threshold,
            config.circuit_breaker_half_open_after_secs,
        ));
        Ok(Self {
            http,
            config: Arc::new(config),
            rate_limiter,
            circuit_breaker,
        })
    }

    pub fn config(&self) -> &GatewayConfig {
        &self.config
    }

    /// Single-turn chat with optional function calling.
    pub async fn chat(
        &self,
        tenant_id: &str,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDef>>,
    ) -> Result<ChatResponse, ModelError> {
        let request = ChatRequest {
            model: self.config.primary_model.clone(),
            messages,
            tools,
            stream: Some(false),
            temperature: Some(0.7),
            max_tokens: Some(4096),
        };
        self.chat_with_request(tenant_id, request).await
    }

    /// Single-turn chat from a fully-specified request.
    pub async fn chat_with_request(
        &self,
        tenant_id: &str,
        request: ChatRequest,
    ) -> Result<ChatResponse, ModelError> {
        if !self.rate_limiter.check().await {
            return Err(ModelError::RateLimited(format!(
                "tenant {} exceeded {} req/window",
                tenant_id, self.config.rate_limit_rpm
            )));
        }

        if !self.circuit_breaker.allow().await {
            return self.chat_fallback(tenant_id, request).await;
        }

        match self.make_chat_request(&request).await {
            Ok(resp) => {
                self.circuit_breaker.record_success().await;
                Ok(resp)
            }
            Err(e) => {
                self.circuit_breaker.record_failure().await;
                if matches!(
                    e,
                    ModelError::ApiError {
                        status: 500..=599,
                        ..
                    } | ModelError::Timeout(_)
                ) {
                    self.chat_fallback(tenant_id, request).await
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Fallback chain: local Ollama (if configured) → secondary hosted model.
    async fn chat_fallback(
        &self,
        tenant_id: &str,
        request: ChatRequest,
    ) -> Result<ChatResponse, ModelError> {
        if let Some(url) = &self.config.ollama_url {
            match self.chat_ollama(url, request.clone()).await {
                Ok(resp) => return Ok(resp),
                Err(e) => warn!(error = %e, "ollama fallback failed"),
            }
        }

        debug!(model = %self.config.fallback_model, tenant = %tenant_id, "trying fallback model");
        let mut fallback = request;
        fallback.model = self.config.fallback_model.clone();
        self.make_chat_request(&fallback)
            .await
            .map_err(|e| ModelError::ModelUnavailable(format!("all models failed: {e}")))
    }

    async fn chat_ollama(
        &self,
        ollama_url: &str,
        request: ChatRequest,
    ) -> Result<ChatResponse, ModelError> {
        let body = OllamaChatRequest {
            model: request.model,
            messages: request
                .messages
                .into_iter()
                .map(|m| OllamaMessage {
                    role: m.role,
                    content: m.content,
                })
                .collect(),
            stream: false,
        };
        let resp = self
            .http
            .post(format!("{ollama_url}/api/chat"))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let message = resp
                .text()
                .await
                .unwrap_or_else(|_| "unreadable body".into());
            return Err(ModelError::ApiError { status, message });
        }
        let out: OllamaChatResponse = resp.json().await?;
        Ok(ChatResponse {
            id: uuid::Uuid::new_v4().to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: out.model,
            choices: vec![ChatChoice {
                index: 0,
                message: Message {
                    role: out.message.role,
                    content: out.message.content,
                    tool_calls: None,
                },
                finish_reason: out.done_reason.or(Some("stop".to_string())),
            }],
            usage: Some(TokenUsage::default()),
        })
    }

    /// Generate an embedding vector for `text`.
    pub async fn embed(&self, tenant_id: &str, text: &str) -> Result<Vec<f32>, ModelError> {
        if !self.rate_limiter.check().await {
            return Err(ModelError::RateLimited(format!("tenant {tenant_id}")));
        }
        let request = EmbeddingRequest {
            model: self.config.embedding_model.clone(),
            input: text.to_string(),
        };
        let resp = self
            .http
            .post(format!("{}/embeddings", self.config.base_url))
            .bearer_auth(&self.config.api_key)
            .json(&request)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let message = resp
                .text()
                .await
                .unwrap_or_else(|_| "unreadable body".into());
            return Err(ModelError::ApiError { status, message });
        }
        let out: EmbeddingResponse = resp.json().await?;
        out.data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| ModelError::InvalidResponse("empty embedding data".into()))
    }

    async fn make_chat_request(&self, request: &ChatRequest) -> Result<ChatResponse, ModelError> {
        let resp = self
            .http
            .post(format!("{}/chat/completions", self.config.base_url))
            .bearer_auth(&self.config.api_key)
            .json(request)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let message = resp
                .text()
                .await
                .unwrap_or_else(|_| "unreadable body".into());
            return Err(ModelError::ApiError { status, message });
        }
        Ok(resp.json::<ChatResponse>().await?)
    }
}

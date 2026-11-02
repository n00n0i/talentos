//! AI surface: list available tools and run the agent loop on a task.
//!
//! The agent runs in-process; see `docs/BUILD_STATUS.md` for the Temporal swap.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use talentos_agent::{AgentRunner, RunReport};
use talentos_hr_runner::kg::{KgEdge, KgError, KgNode};
use talentos_kernel::shared::{AgentContext, AgentResult};
use talentos_kernel::TenantId;
use talentos_model_gw::{ChatRequest, GatewayConfig, HuakatiClient, Message};
use talentos_rag::HybridSearch;

use crate::dto::requests::AiSettingsUpdate;
use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::secrets::{ai_secret_path, AiSecret};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct RunAgentRequest {
    pub task: String,
    /// Allow the run to execute irreversible tools (payroll approval, termination).
    #[serde(default)]
    pub allow_destructive: bool,
}

#[derive(Debug, Serialize)]
pub struct RunAgentResponse {
    pub run_id: String,
    pub outcome: String,
    pub cycles: usize,
    pub approvals_requested: Vec<String>,
}

/// GET /api/v1/ai/tools
pub async fn list_tools() -> Response {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "tools": talentos_tools::tools::TOOL_NAMES })),
    )
        .into_response()
}

/// POST /api/v1/ai/agents/run
pub async fn run_agent(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<RunAgentRequest>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    let model_gw = match HuakatiClient::new() {
        Ok(c) => c,
        Err(e) => return unavailable(&format!("model gateway not configured: {e}")),
    };

    let mut built = talentos_tools::tools::default_registry(
        Some(Arc::new(model_gw.clone())),
        state.pool.clone(),
    );
    talentos_memory::tools::register_tools(&mut built, state.memory.clone());
    let registry = Arc::new(built);

    let agent_ctx = AgentContext {
        tenant_id: ctx.tenant_id,
        user_id: ctx.user_id,
        entity_id: ctx.entity_id,
        roles: ctx.roles.clone(),
        run_id: Uuid::new_v4(),
    };

    let runner = AgentRunner {
        context: agent_ctx,
        model_gw: model_gw.clone(),
        db: state.pool.clone(),
        hybrid_search: HybridSearch::new(model_gw, state.pool.pool().clone()),
        tool_registry: registry,
        max_cycles: 3,
    };

    // Human gate: this endpoint never auto-approves destructive tools unless the
    // caller explicitly opted in.
    let allow = body.allow_destructive;
    let report: RunReport = runner
        .run(&body.task, move |_tool| async move { allow })
        .await;

    let outcome = match &report.result {
        AgentResult::Success(_) => "success",
        AgentResult::Failed(_) => "failed",
        AgentResult::MaxCyclesExceeded => "max_cycles_exceeded",
        AgentResult::ApprovalRejected(_) => "approval_rejected",
    };

    (
        StatusCode::OK,
        Json(RunAgentResponse {
            run_id: runner.context.run_id.to_string(),
            outcome: outcome.to_string(),
            cycles: report.cycles.len(),
            approvals_requested: report.approvals_requested,
        }),
    )
        .into_response()
}

#[derive(Debug, Serialize)]
pub struct AiSettingsResponse {
    pub provider: String,
    pub base_url: Option<String>,
    pub model: String,
    pub mcp_enabled: bool,
    pub skills: serde_json::Value,
    pub api_key_set: bool,
    pub secrets_backend: &'static str,
}

/// GET /api/v1/ai/settings — never reads or returns the provider key.
pub async fn get_settings(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let row = sqlx::query(
        "SELECT provider, base_url, model, mcp_enabled, skills, api_key_set
           FROM ai_settings WHERE tenant_id = $1",
    )
    .bind(ctx.tenant_id.0)
    .fetch_optional(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    let backend = secrets_backend(&state);
    match row {
        Ok(Some(row)) => (
            StatusCode::OK,
            Json(AiSettingsResponse {
                provider: row.get("provider"),
                base_url: row.get("base_url"),
                model: row.get("model"),
                mcp_enabled: row.get("mcp_enabled"),
                skills: row.get("skills"),
                api_key_set: row.get("api_key_set"),
                secrets_backend: backend,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::OK,
            Json(AiSettingsResponse {
                provider: "huakati".into(),
                base_url: None,
                model: "qwen2.5-72b-instruct".into(),
                mcp_enabled: true,
                skills: serde_json::json!([]),
                api_key_set: false,
                secrets_backend: backend,
            }),
        )
            .into_response(),
        Err(e) => internal_err(&e.to_string()),
    }
}

/// PUT /api/v1/ai/settings — non-secrets in Postgres, key only in OpenBao.
pub async fn put_settings(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<AiSettingsUpdate>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }

    let key = body.api_key.as_deref().filter(|k| !k.trim().is_empty());
    if let Some(key) = key {
        // Fail closed before any write: a disabled backend never stores a key.
        let Some(client) = &state.secrets else {
            return unavailable("OpenBao is not configured");
        };
        let secret = AiSecret {
            provider: body.provider.clone(),
            base_url: body.base_url.clone(),
            model: body.model.clone(),
            api_key: key.to_string(),
        };
        if let Err(e) = client.write_ai_secret(ctx.tenant_id.0, &secret).await {
            return unavailable(&format!("secret backend write failed: {e}"));
        }
    }

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let saved = sqlx::query(
        r#"INSERT INTO ai_settings
             (tenant_id, provider, base_url, model, mcp_enabled, skills, api_key_set, secret_ref)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
           ON CONFLICT (tenant_id) DO UPDATE SET
               provider = EXCLUDED.provider,
               base_url = EXCLUDED.base_url,
               model = EXCLUDED.model,
               mcp_enabled = EXCLUDED.mcp_enabled,
               skills = EXCLUDED.skills,
               api_key_set = CASE WHEN $7 THEN true ELSE ai_settings.api_key_set END,
               secret_ref = CASE WHEN $7 THEN EXCLUDED.secret_ref ELSE ai_settings.secret_ref END,
               updated_at = now()
           RETURNING api_key_set"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(&body.provider)
    .bind(&body.base_url)
    .bind(&body.model)
    .bind(body.mcp_enabled)
    .bind(&body.skills)
    .bind(key.is_some())
    .bind(key.map(|_| ai_secret_path(ctx.tenant_id.0)))
    .fetch_one(&mut *tx)
    .await;
    let api_key_set = match saved {
        Ok(row) => row.get::<bool, _>("api_key_set"),
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    (
        StatusCode::OK,
        Json(AiSettingsResponse {
            provider: body.provider,
            base_url: body.base_url,
            model: body.model,
            mcp_enabled: body.mcp_enabled,
            skills: body.skills,
            api_key_set,
            secrets_backend: secrets_backend(&state),
        }),
    )
        .into_response()
}

#[derive(Debug, Serialize)]
pub struct AiSettingsTestResponse {
    pub ok: bool,
    pub latency_ms: u64,
    pub model: String,
}

/// POST /api/v1/ai/settings/test — one cheap completion with the stored secret.
pub async fn test_settings(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    let Some(secrets) = &state.secrets else {
        return unavailable("OpenBao is not configured");
    };
    let secret = match secrets.read_ai_secret(ctx.tenant_id.0).await {
        Ok(Some(secret)) => secret,
        Ok(None) => return unavailable("AI provider is not configured"),
        Err(e) => return unavailable(&format!("secret backend read failed: {e}")),
    };

    let defaults = GatewayConfig::default();
    let config = GatewayConfig {
        base_url: secret.base_url.clone().unwrap_or(defaults.base_url.clone()),
        api_key: secret.api_key.clone(),
        primary_model: secret.model.clone(),
        ..defaults
    };
    let client = match HuakatiClient::with_config(config) {
        Ok(client) => client,
        Err(e) => return unavailable(&format!("model gateway not configured: {e}")),
    };

    let request = ChatRequest {
        model: secret.model.clone(),
        messages: vec![Message {
            role: "user".into(),
            content: "ping".into(),
            tool_calls: None,
        }],
        tools: None,
        stream: Some(false),
        temperature: Some(0.0),
        max_tokens: Some(8),
    };
    let started = Instant::now();
    match client
        .chat_with_request(&ctx.tenant_id.0.to_string(), request)
        .await
    {
        Ok(response) => (
            StatusCode::OK,
            Json(AiSettingsTestResponse {
                ok: true,
                latency_ms: started.elapsed().as_millis() as u64,
                model: response.model,
            }),
        )
            .into_response(),
        Err(e) => unavailable(&format!("model request failed: {e}")),
    }
}

#[derive(Debug, Serialize)]
pub struct KgCountsResponse {
    pub nodes: std::collections::BTreeMap<talentos_hr_runner::kg::NodeClass, i64>,
    pub edges: i64,
    /// Derived from `max(kg_nodes.updated_at)`: no extra column or migration.
    pub last_rebuilt_at: Option<DateTime<Utc>>,
}

/// GET /api/v1/ai/kg — class counts, edge count, last rebuild time.
pub async fn get_kg(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "ai", "chat") {
        return forbidden;
    }
    let nodes = match talentos_hr_runner::kg::counts(&state.pool, ctx.tenant_id).await {
        Ok(nodes) => nodes,
        Err(e) => return internal_err(&e.to_string()),
    };
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let stats = sqlx::query(
        "SELECT (SELECT count(*) FROM kg_edges WHERE tenant_id = $1) AS edges,
                (SELECT max(updated_at) FROM kg_nodes WHERE tenant_id = $1) AS last_rebuilt_at",
    )
    .bind(ctx.tenant_id.0)
    .fetch_one(&mut *tx)
    .await;
    let _ = tx.rollback().await;
    let stats = match stats {
        Ok(row) => row,
        Err(e) => return internal_err(&e.to_string()),
    };
    (
        StatusCode::OK,
        Json(KgCountsResponse {
            nodes,
            edges: stats.get("edges"),
            last_rebuilt_at: stats.get("last_rebuilt_at"),
        }),
    )
        .into_response()
}

/// POST /api/v1/ai/kg/rebuild — idempotent sync, capped by the service.
pub async fn rebuild_kg(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "ai.settings", "manage") {
        return forbidden;
    }
    match talentos_hr_runner::kg_sync::rebuild(&state.pool, ctx.tenant_id).await {
        Ok(report) => (StatusCode::OK, Json(report)).into_response(),
        Err(e) => internal_err(&e.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct KgNeighborsQuery {
    pub node_id: Uuid,
    pub depth: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct KgNeighborsResponse {
    pub nodes: Vec<talentos_hr_runner::kg::KgNode>,
    pub edges: Vec<talentos_hr_runner::kg::KgEdge>,
}

/// GET /api/v1/ai/kg/neighbors?node_id=&depth= — the graph inspector's data.
pub async fn kg_neighbors(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<KgNeighborsQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ai", "chat") {
        return forbidden;
    }
    // neighbors() clamps depth to 1..=5; 2 matches the agent's subgraph depth.
    let depth = query.depth.unwrap_or(2);
    match talentos_hr_runner::kg::neighbors(&state.pool, ctx.tenant_id, query.node_id, depth).await
    {
        Ok((nodes, edges)) => {
            (StatusCode::OK, Json(KgNeighborsResponse { nodes, edges })).into_response()
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

/// Ontology summary from the design (§4 classes, §5 relations). Lives in the
/// system prompt so the model can name classes and relations it sees in triples.
pub const ONTOLOGY_HEADER: &str = "\
You are the TalentOS HR assistant for one tenant.

Ontology classes (12):
Organization (องค์กร), LegalEntity (นิติบุคคล/สาขา), Department (แผนก),
Position (ตำแหน่ง), Employee (พนักงาน), Skill (ทักษะ), Policy (นโยบาย),
Benefit (สวัสดิการ), PayrollItem (รายการเงินเดือน), Cycle (รอบ/งวด),
Document (เอกสาร), Process (กระบวนการ).

Relations:
Organization --has_entity--> LegalEntity
LegalEntity --has_department--> Department
Department --has_position--> Position
Position --held_by--> Employee (inverse: Employee --holds--> Position)
Employee --member_of--> Department
Employee --has_skill--> Skill
Policy --applies_to--> Organization | LegalEntity | Department | Position
Benefit --granted_by--> Policy
PayrollItem --derived_from--> Policy | Benefit
Cycle --contains--> PayrollItem
Document --belongs_to--> Employee | Department
Process --step_of--> Process

Rules: an Employee belongs to exactly one Department and one Position, under one
LegalEntity, inside the Organization.

Answer in Thai (ตอบเป็นภาษาไทย). Use only the knowledge-graph facts below and the tenant's real data.
Never invent policy numbers, rates, thresholds or dates; if a fact is absent, say
you do not have it and point to the relevant Policy or document.";

/// System prompt: ontology + retrieved triples + tool names + Thai instruction.
/// Triples are rendered once each, in the order given.
pub fn build_chat_prompt(triples: &[String], tool_names: &[&str]) -> String {
    let mut prompt = String::from(ONTOLOGY_HEADER);
    prompt.push_str("\n\nKnowledge graph facts:\n");
    if triples.is_empty() {
        prompt.push_str("- (no matching facts)\n");
    } else {
        for triple in triples {
            prompt.push_str("- ");
            prompt.push_str(triple);
            prompt.push('\n');
        }
    }
    prompt.push_str("\nAvailable tools: ");
    prompt.push_str(&tool_names.join(", "));
    prompt.push('\n');
    prompt
}

#[derive(Debug, Deserialize)]
pub struct ChatBody {
    pub message: String,
    #[serde(default)]
    pub agent: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Citation {
    pub class: String,
    pub label: String,
    pub relation: String,
}

#[derive(Debug, Default, Serialize)]
pub struct ChatUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ChatReply {
    pub reply: String,
    pub citations: Vec<Citation>,
    pub usage: ChatUsage,
}

/// POST /api/v1/ai/chat — answer grounded in the tenant's knowledge graph.
pub async fn chat(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<ChatBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ai", "chat") {
        return forbidden;
    }
    let message = body.message.trim();
    if message.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid_request".into(),
                message: "message must not be empty".into(),
                details: vec![],
                request_id: Uuid::new_v4().to_string(),
                retry_after_seconds: None,
            }),
        )
            .into_response();
    }

    // 1. Settings (existence check) + provider secret. Both must be configured.
    let Some(secrets) = &state.secrets else {
        return unavailable("ยังไม่ได้ตั้งค่า AI กรุณาตั้งค่า provider และ API key ที่หน้าตั้งค่า AI");
    };
    let secret = match secrets.read_ai_secret(ctx.tenant_id.0).await {
        Ok(Some(secret)) => secret,
        Ok(None) => return unavailable("ยังไม่ได้ตั้งค่า AI กรุณาตั้งค่า provider และ API key ที่หน้าตั้งค่า AI"),
        Err(e) => {
            tracing::warn!(error = %e, "chat secret read failed");
            return unavailable("ไม่สามารถอ่านการตั้งค่า AI ได้ กรุณาลองใหม่ภายหลัง");
        }
    };
    let settings_configured = match state
        .pool
        .begin_tenant(ctx.tenant_id)
        .await
        .map_err(|e| e.to_string())
    {
        Ok(mut tx) => {
            let row = sqlx::query("SELECT 1 FROM ai_settings WHERE tenant_id = $1")
                .bind(ctx.tenant_id.0)
                .fetch_optional(&mut *tx)
                .await;
            let _ = tx.rollback().await;
            match row {
                Ok(row) => row.is_some(),
                Err(e) => return internal_err(&e.to_string()),
            }
        }
        Err(e) => return internal_err(&e),
    };
    if !settings_configured {
        return unavailable("ยังไม่ได้ตั้งค่า AI กรุณาตั้งค่า provider และ API key ที่หน้าตั้งค่า AI");
    }

    // 2. Retrieve the subgraph around nodes mentioned in the message.
    let candidates = match candidate_nodes(&state.pool, ctx.tenant_id, message).await {
        Ok(nodes) => nodes,
        Err(e) => return internal_err(&e.to_string()),
    };
    let mut nodes: BTreeMap<Uuid, KgNode> = BTreeMap::new();
    let mut edges: BTreeMap<Uuid, KgEdge> = BTreeMap::new();
    for candidate in &candidates {
        match talentos_hr_runner::kg::neighbors(&state.pool, ctx.tenant_id, candidate.id, 2).await {
            Ok((ns, es)) => {
                for node in ns {
                    nodes.entry(node.id).or_insert(node);
                }
                for edge in es {
                    edges.entry(edge.id).or_insert(edge);
                }
            }
            Err(e) => return internal_err(&e.to_string()),
        }
    }

    // 3. Prompt: each triple once, citations map to the triple's object.
    let index: HashMap<Uuid, &KgNode> = nodes.values().map(|n| (n.id, n)).collect();
    let mut triples = BTreeSet::new();
    let mut seen_citations = BTreeSet::new();
    let mut citations = Vec::new();
    for edge in edges.values() {
        let (Some(from), Some(to)) = (index.get(&edge.from_id), index.get(&edge.to_id)) else {
            continue;
        };
        triples.insert(format!(
            "{} --{}--> {}",
            from.label_th, edge.relation, to.label_th
        ));
        let citation = (
            to.class.as_str(),
            to.label_th.as_str(),
            edge.relation.as_str(),
        );
        if seen_citations.insert(citation) {
            citations.push(Citation {
                class: to.class.as_str().to_string(),
                label: to.label_th.clone(),
                relation: edge.relation.clone(),
            });
        }
    }
    let triples: Vec<String> = triples.into_iter().collect();

    // 4. Call the tenant's provider. Errors never echo the prompt or the key.
    let defaults = GatewayConfig::default();
    let config = GatewayConfig {
        base_url: secret.base_url.clone().unwrap_or(defaults.base_url.clone()),
        api_key: secret.api_key.clone(),
        primary_model: secret.model.clone(),
        ..defaults
    };
    let client = match HuakatiClient::with_config(config) {
        Ok(client) => client,
        Err(_) => return unavailable("การตั้งค่า AI ไม่สมบูรณ์ กรุณาตรวจสอบที่หน้าตั้งค่า AI"),
    };
    let request = ChatRequest {
        model: secret.model.clone(),
        messages: vec![
            Message {
                role: "system".into(),
                content: build_chat_prompt(&triples, talentos_tools::tools::TOOL_NAMES),
                tool_calls: None,
            },
            Message {
                role: "user".into(),
                content: message.to_string(),
                tool_calls: None,
            },
        ],
        tools: None,
        stream: Some(false),
        temperature: Some(0.3),
        max_tokens: Some(1024),
    };
    let response = match client
        .chat_with_request(&ctx.tenant_id.0.to_string(), request)
        .await
    {
        Ok(response) => response,
        Err(e) => {
            tracing::warn!(error = %e, "chat provider request failed");
            return unavailable("ไม่สามารถเชื่อมต่อผู้ให้บริการ AI ได้ กรุณาลองใหม่ภายหลัง");
        }
    };
    let reply = response
        .choices
        .first()
        .map(|c| c.message.content.trim().to_string())
        .unwrap_or_default();
    if reply.is_empty() {
        return unavailable("ผู้ให้บริการ AI ส่งคำตอบว่างเปล่า กรุณาลองใหม่ภายหลัง");
    }

    // 5. Persist the conversation, plus usage/cost when the provider reported it.
    persist_chat(
        &state,
        ctx.tenant_id,
        ctx.entity_id.map(|e| e.0),
        body.agent.as_deref().unwrap_or("chat"),
        message,
        &reply,
        &secret.model,
        response.usage.as_ref(),
    )
    .await;

    let usage = ChatUsage {
        input_tokens: response.usage.as_ref().map(|u| u.prompt_tokens),
        output_tokens: response.usage.as_ref().map(|u| u.completion_tokens),
    };
    (
        StatusCode::OK,
        Json(ChatReply {
            reply,
            citations,
            usage,
        }),
    )
        .into_response()
}

/// Candidate nodes whose label/key matches a message token (or whose label the
/// message mentions), capped so a long question cannot fan out the graph walk.
async fn candidate_nodes(
    pool: &talentos_db::TenantPool,
    tenant_id: TenantId,
    message: &str,
) -> Result<Vec<KgNode>, KgError> {
    let patterns: Vec<String> = message
        .split_whitespace()
        .filter(|token| token.chars().count() >= 2)
        .take(8)
        .map(|token| format!("%{token}%"))
        .collect();

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT id, class, key, label_th, label_en, properties
             FROM kg_nodes
            WHERE tenant_id = $1
              AND (
                   label_th ILIKE ANY($2)
                   OR key ILIKE ANY($2)
                   OR coalesce(label_en, '') ILIKE ANY($2)
                   OR (char_length(label_th) >= 2 AND $3 ILIKE '%' || label_th || '%')
              )
            ORDER BY class, key
            LIMIT 5"#,
    )
    .bind(tenant_id.0)
    .bind(&patterns)
    .bind(message)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.iter()
        .map(|row| {
            let class: String = row.try_get("class")?;
            Ok(KgNode {
                id: row.try_get("id")?,
                class: class.parse()?,
                key: row.try_get("key")?,
                label_th: row.try_get("label_th")?,
                label_en: row.try_get("label_en")?,
                properties: row.try_get("properties")?,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
async fn persist_chat(
    state: &AppState,
    tenant_id: TenantId,
    entity_id: Option<Uuid>,
    agent: &str,
    message: &str,
    reply: &str,
    model: &str,
    usage: Option<&talentos_model_gw::TokenUsage>,
) {
    let messages = serde_json::json!([
        {"role": "user", "content": message},
        {"role": "assistant", "content": reply},
    ]);
    let persisted = async {
        let mut tx = state.pool.begin_tenant(tenant_id).await?;
        sqlx::query(
            r#"INSERT INTO ai_conversations (agent_id, tenant_id, entity_id, messages, state)
               VALUES ($1, $2, $3, $4, 'completed')"#,
        )
        .bind(agent)
        .bind(tenant_id.0)
        .bind(entity_id)
        .bind(&messages)
        .execute(&mut *tx)
        .await?;

        // Cost is only recorded when the provider reported token usage.
        if let Some(usage) = usage {
            let cost = rust_decimal::Decimal::try_from(usage.estimated_cost_usd(model))
                .unwrap_or_default();
            sqlx::query(
                r#"INSERT INTO ai_cost_tracking
                     (tenant_id, model, input_tokens, output_tokens, cached_tokens, cost_usd)
                   VALUES ($1, $2, $3, $4, $5, $6)
                   ON CONFLICT (tenant_id, model, date) DO UPDATE SET
                       input_tokens = ai_cost_tracking.input_tokens + EXCLUDED.input_tokens,
                       output_tokens = ai_cost_tracking.output_tokens + EXCLUDED.output_tokens,
                       cached_tokens = ai_cost_tracking.cached_tokens + EXCLUDED.cached_tokens,
                       cost_usd = ai_cost_tracking.cost_usd + EXCLUDED.cost_usd"#,
            )
            .bind(tenant_id.0)
            .bind(model)
            .bind(usage.prompt_tokens as i64)
            .bind(usage.completion_tokens as i64)
            .bind(usage.cached_tokens as i64)
            .bind(cost)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok::<(), sqlx::Error>(())
    }
    .await;
    // A reply the user already has must not be lost to an analytics write.
    if let Err(e) = persisted {
        tracing::error!(error = %e, "failed to persist ai chat");
    }
}

fn secrets_backend(state: &AppState) -> &'static str {
    if state.secrets.is_some() {
        "openbao"
    } else {
        "none"
    }
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "ai settings request failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "internal_error".into(),
            message: "Internal server error".into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn unavailable(message: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: "ai_unavailable".into(),
            message: message.to_string(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_has_ontology_header_and_each_triple_once() {
        let triples = vec![
            "ฝ่ายขาย --member_of--> ฝ่ายขาย".to_string(),
            "สมชาย --held_by--> วิศวกร".to_string(),
            "นโยบายลาพักร้อน --applies_to--> องค์กร".to_string(),
        ];
        let prompt = build_chat_prompt(&triples, &["calculate_tax", "query_employee"]);

        for class in [
            "Organization",
            "LegalEntity",
            "Department",
            "Position",
            "Employee",
            "Skill",
            "Policy",
            "Benefit",
            "PayrollItem",
            "Cycle",
            "Document",
            "Process",
        ] {
            assert!(prompt.contains(class), "missing class {class}");
        }
        for relation in [
            "has_entity",
            "has_department",
            "has_position",
            "held_by",
            "member_of",
            "has_skill",
            "applies_to",
            "granted_by",
            "derived_from",
            "contains",
            "belongs_to",
            "step_of",
        ] {
            assert!(prompt.contains(relation), "missing relation {relation}");
        }
        for triple in &triples {
            assert_eq!(prompt.matches(triple).count(), 1, "triple once: {triple}");
        }
        assert!(prompt.contains("ภาษาไทย"));
        assert!(prompt.contains("Never invent policy numbers"));
        assert!(prompt.contains("calculate_tax"));
        assert!(prompt.contains("query_employee"));
    }
}

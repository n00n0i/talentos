-- AI Core Schema
-- Agent conversations, runs, tool calls, cost tracking, and per-tenant API keys

-- Agent conversation store
CREATE TABLE ai_conversations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id TEXT NOT NULL,           -- agent type identifier
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    entity_id UUID REFERENCES entities(id) ON DELETE SET NULL,
    messages JSONB NOT NULL DEFAULT '[]',
    state TEXT NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'paused', 'completed', 'failed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_ai_conversations_tenant ON ai_conversations(tenant_id, created_at DESC);
CREATE INDEX idx_ai_conversations_agent ON ai_conversations(agent_id, state);

-- Agent run tracking
CREATE TABLE ai_agent_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_type TEXT NOT NULL CHECK (agent_type IN (
        'PayrollAnomalyAgent', 'PolicyQueryAgent', 'PayrollRunAgent',
        'LeavePolicyAgent', 'ComplianceMonitorAgent'
    )),
    task TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'running' CHECK (state IN (
        'running', 'paused_for_approval', 'completed', 'failed', 'timeout'
    )),
    context JSONB NOT NULL DEFAULT '{}',
    result JSONB,
    human_approver UUID REFERENCES users(id),
    approved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE
);

CREATE INDEX idx_ai_agent_runs_tenant ON ai_agent_runs(tenant_id, created_at DESC);
CREATE INDEX idx_ai_agent_runs_type ON ai_agent_runs(agent_type, state);
CREATE INDEX idx_ai_agent_runs_temporal ON ai_agent_runs(id) WHERE state IN ('running', 'paused_for_approval');

-- Tool call audit log
CREATE TABLE ai_tool_calls (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES ai_agent_runs(id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    input JSONB NOT NULL,
    output JSONB,
    duration_ms BIGINT NOT NULL DEFAULT 0,
    status TEXT NOT NULL CHECK (status IN ('success', 'failed', 'pending_approval')),
    error JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_ai_tool_calls_run ON ai_tool_calls(run_id, created_at);

-- Cost tracking per tenant per day per model
CREATE TABLE ai_cost_tracking (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    input_tokens BIGINT NOT NULL DEFAULT 0,
    output_tokens BIGINT NOT NULL DEFAULT 0,
    cached_tokens BIGINT NOT NULL DEFAULT 0,
    cost_usd NUMERIC(12,6) NOT NULL DEFAULT 0,
    date DATE NOT NULL DEFAULT current_date,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(tenant_id, model, date)
);

CREATE INDEX idx_ai_cost_tenant_date ON ai_cost_tracking(tenant_id, date DESC);
CREATE INDEX idx_ai_cost_model ON ai_cost_tracking(model, date DESC);

-- Per-tenant API key override (optional)
CREATE TABLE tenant_api_keys (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE UNIQUE,
    api_key TEXT NOT NULL,          -- encrypted at rest
    provider TEXT NOT NULL DEFAULT 'huakati',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Enable RLS on all AI tables
ALTER TABLE ai_conversations ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_conversations FORCE ROW LEVEL SECURITY;
ALTER TABLE ai_agent_runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_agent_runs FORCE ROW LEVEL SECURITY;
ALTER TABLE ai_tool_calls ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_tool_calls FORCE ROW LEVEL SECURITY;
ALTER TABLE ai_cost_tracking ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_cost_tracking FORCE ROW LEVEL SECURITY;
ALTER TABLE tenant_api_keys ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenant_api_keys FORCE ROW LEVEL SECURITY;

-- RLS policies
CREATE POLICY ai_conversations_isolation ON ai_conversations USING (tenant_id = current_tenant_id());
CREATE POLICY ai_agent_runs_isolation ON ai_agent_runs USING (tenant_id = current_tenant_id());
CREATE POLICY ai_tool_calls_isolation ON ai_tool_calls USING (EXISTS (
    SELECT 1 FROM ai_agent_runs ar WHERE ar.id = ai_tool_calls.run_id AND ar.tenant_id = current_tenant_id()
));
CREATE POLICY ai_cost_tracking_isolation ON ai_cost_tracking USING (tenant_id = current_tenant_id());
CREATE POLICY tenant_api_keys_isolation ON tenant_api_keys USING (tenant_id = current_tenant_id());
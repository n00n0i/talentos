-- Multi-provider AI configuration (design 2026-11-03 §2 and §8).
--
-- One row per provider per tenant; keys live in OpenBao. Model roles are the
-- only runtime source of truth for which model serves a job.
--
-- Version note: the design's 2026-11-03 timestamp collides with the already
-- applied 20261103000001_user_totp migration and would run before ai_settings
-- exists, so this lands after 20261215000001_ai_kg.
CREATE TABLE ai_providers (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id          UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name               TEXT NOT NULL,
    kind               TEXT NOT NULL,
    base_url           TEXT NOT NULL,
    api_key_ref        TEXT,
    models_cache       JSONB NOT NULL DEFAULT '[]',
    models_fetched_at  TIMESTAMPTZ,
    enabled            BOOLEAN NOT NULL DEFAULT true,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, name)
);

CREATE TABLE ai_model_roles (
    tenant_id    UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    role         TEXT NOT NULL CHECK (role IN ('chat','embedding','ocr','vision','graph_extract')),
    provider_id  UUID NOT NULL REFERENCES ai_providers(id) ON DELETE CASCADE,
    model        TEXT NOT NULL,
    params       JSONB NOT NULL DEFAULT '{}',
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, role)
);

ALTER TABLE ai_providers ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_providers FORCE ROW LEVEL SECURITY;
CREATE POLICY ai_providers_isolation ON ai_providers
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

ALTER TABLE ai_model_roles ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai_model_roles FORCE ROW LEVEL SECURITY;
CREATE POLICY ai_model_roles_isolation ON ai_model_roles
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

-- §8: move the first cut's single provider into the new tables. The old
-- `secret_ref` becomes `api_key_ref` unchanged; OpenBao reads go through the
-- stored path, so existing tenant secrets keep working.
INSERT INTO ai_providers (tenant_id, name, kind, base_url, api_key_ref)
SELECT s.tenant_id,
       'Default',
       s.provider,
       COALESCE(NULLIF(btrim(s.base_url), ''), 'https://llmapi.huakati.online/v1'),
       CASE WHEN s.api_key_set THEN s.secret_ref ELSE NULL END
  FROM ai_settings s
 WHERE s.provider IS NOT NULL
   AND btrim(s.provider) <> ''
ON CONFLICT (tenant_id, name) DO NOTHING;

INSERT INTO ai_model_roles (tenant_id, role, provider_id, model, params)
SELECT s.tenant_id, 'chat', p.id, s.model, '{}'::jsonb
  FROM ai_settings s
  JOIN ai_providers p ON p.tenant_id = s.tenant_id AND p.name = 'Default'
 WHERE s.model IS NOT NULL
   AND btrim(s.model) <> ''
ON CONFLICT (tenant_id, role) DO NOTHING;

-- ai_settings keeps only cross-cutting flags from here on.
ALTER TABLE ai_settings ADD COLUMN IF NOT EXISTS agents JSONB NOT NULL DEFAULT '[]';
ALTER TABLE ai_settings DROP COLUMN IF EXISTS provider;
ALTER TABLE ai_settings DROP COLUMN IF EXISTS base_url;
ALTER TABLE ai_settings DROP COLUMN IF EXISTS model;
ALTER TABLE ai_settings DROP COLUMN IF EXISTS secret_ref;
ALTER TABLE ai_settings DROP COLUMN IF EXISTS api_key_set;

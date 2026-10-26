-- Per-tenant payslip branding. Configuration only — no free-form HTML is stored
-- or rendered, so a tenant cannot inject markup into another payer's payslip.
CREATE TABLE payslip_templates (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    entity_id   UUID REFERENCES entities(id) ON DELETE CASCADE,  -- NULL = all entities
    name        TEXT NOT NULL DEFAULT 'default',
    is_default  BOOLEAN NOT NULL DEFAULT TRUE,
    company_display_name TEXT,
    logo_url    TEXT,
    accent_color TEXT NOT NULL DEFAULT '#0369a1'
                 CHECK (accent_color ~ '^#[0-9a-fA-F]{6}$'),
    footer_text TEXT,
    show_fields JSONB NOT NULL DEFAULT '["ot","sso","pf","tax","anomalies"]'::jsonb,
    paper_size  TEXT NOT NULL DEFAULT 'A4' CHECK (paper_size IN ('A4', 'A5', 'Letter')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, entity_id, name)
);

CREATE INDEX idx_payslip_templates_tenant ON payslip_templates(tenant_id);

ALTER TABLE payslip_templates ENABLE ROW LEVEL SECURITY;
ALTER TABLE payslip_templates FORCE ROW LEVEL SECURITY;

CREATE POLICY payslip_templates_isolation ON payslip_templates
    USING (tenant_id = current_tenant_id());

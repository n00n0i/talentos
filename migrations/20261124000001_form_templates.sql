-- Editable, versioned statutory form templates (50 ทวิ and สปส.1-10).
-- Tenant rows override the global defaults (`tenant_id IS NULL`); defaults are
-- writable only via migration, tenants can only edit their own rows.
CREATE TABLE form_templates (
    id          BIGSERIAL PRIMARY KEY,
    tenant_id   UUID REFERENCES tenants(id) ON DELETE CASCADE,  -- NULL = global default
    form_type   TEXT NOT NULL CHECK (form_type IN ('50tawi', 'sso1_10')),
    version     INT NOT NULL DEFAULT 1,
    config      JSONB NOT NULL,
    is_active   BOOLEAN NOT NULL DEFAULT TRUE,
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, form_type, version)
);

CREATE INDEX idx_form_templates_tenant_type ON form_templates(tenant_id, form_type);

ALTER TABLE form_templates ENABLE ROW LEVEL SECURITY;
ALTER TABLE form_templates FORCE ROW LEVEL SECURITY;

-- Everyone may read their own tenant rows and the global defaults.
CREATE POLICY form_templates_select ON form_templates
    FOR SELECT USING (tenant_id = current_tenant_id() OR tenant_id IS NULL);

-- Only the owning tenant may insert/update (global defaults are migration-only).
CREATE POLICY form_templates_insert ON form_templates
    FOR INSERT WITH CHECK (tenant_id = current_tenant_id());

CREATE POLICY form_templates_update ON form_templates
    FOR UPDATE USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

INSERT INTO form_templates (tenant_id, form_type, version, config, is_active)
VALUES
  (NULL, '50tawi', 1, $${
    "title": "หนังสือรับรองการหักภาษี ณ ที่จ่าย",
    "fields": [
      {"key":"payer_name","label":"ผู้จ่ายเงิน","source":"entity_name"},
      {"key":"payer_tax_id","label":"เลขประจำตัวผู้เสียภาษีผู้จ่าย","source":"entity_tax_id"},
      {"key":"payer_address","label":"ที่อยู่ผู้จ่าย","source":"entity_address"},
      {"key":"payee_name","label":"ผู้รับเงิน","source":"employee_name"},
      {"key":"payee_tax_id","label":"เลขประจำตัวผู้เสียภาษีผู้รับ","source":"national_id"},
      {"key":"income_type","label":"ประเภทเงินได้","source":"income_type"},
      {"key":"payment_date","label":"วันที่จ่าย","source":"payment_date"},
      {"key":"amount","label":"จำนวนเงินที่จ่าย","source":"gross"},
      {"key":"tax","label":"ภาษีที่หัก","source":"tax"},
      {"key":"cert_no","label":"เลขที่หนังสือรับรอง","source":"cert_no"}
    ]
  }$$::jsonb, TRUE),
  (NULL, 'sso1_10', 1, $${
    "title": "แบบรายการแสดงการส่งเงินสมทบ",
    "header": [
      {"key":"employer_name","label":"ชื่อสถานประกอบการ","source":"entity_name"},
      {"key":"employer_account","label":"เลขที่บัญชีนายจ้าง","source":"sso_account"},
      {"key":"branch","label":"ลำดับที่สาขา","source":"branch"}
    ],
    "columns": [
      {"label":"เลขประจำตัวประชาชน","source":"national_id"},
      {"label":"ชื่อ-สกุล","source":"employee_name"},
      {"label":"ค่าจ้าง","source":"gross"},
      {"label":"เงินสมทบลูกจ้าง","source":"sso_employee"},
      {"label":"เงินสมทบนายจ้าง","source":"sso_employer"}
    ]
  }$$::jsonb, TRUE);

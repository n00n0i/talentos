-- Per-bank payout file registry.
-- A row is one bank's transfer-file layout for this tenant: the column order
-- (field_order) the bank expects and its header labels (header_row, comma-
-- separated, aligned with field_order). Export resolves each source against
-- the employee's pay record (employee_code / employee_name / bank_name /
-- bank_account / net) and writes field_order cells.
--
-- Global seed rows (tenant_id IS NULL) are the built-in reference layouts;
-- they are migration-only and marked unverified so a tenant or accountant must
-- confirm against the live bank spec before upload. Tenant rows are created
-- via the API and start unverified too.
CREATE TABLE bank_templates (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     UUID REFERENCES tenants(id) ON DELETE CASCADE,  -- NULL = global seed
    bank_name     TEXT NOT NULL,
    layout_name   TEXT NOT NULL,
    field_order   TEXT[] NOT NULL,
    header_row    TEXT,
    unverified    BOOLEAN NOT NULL DEFAULT TRUE,
    sign_off_note TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_bank_templates_tenant ON bank_templates (tenant_id, bank_name);

ALTER TABLE bank_templates ENABLE ROW LEVEL SECURITY;
ALTER TABLE bank_templates FORCE ROW LEVEL SECURITY;

-- Everyone may read their own tenant rows and the global seeds.
CREATE POLICY bank_templates_select ON bank_templates
    FOR SELECT USING (tenant_id = current_tenant_id() OR tenant_id IS NULL);

-- Only the owning tenant may insert (global seeds are migration-only).
CREATE POLICY bank_templates_insert ON bank_templates
    FOR INSERT WITH CHECK (tenant_id = current_tenant_id());

-- Global seeds: Bangkok Bank transfer-file layout.
-- unverified = TRUE, sign_off_note flags it as a public sample pending
-- accountant sign-off. Do not rely on it for a real upload without checking
-- the bank's current spec.
INSERT INTO bank_templates
    (tenant_id, bank_name, layout_name, field_order, header_row, unverified, sign_off_note)
VALUES
    (NULL,
     'Bangkok Bank',
     'Standard CSV transfer (public sample)',
     ARRAY['employee_code','employee_name','bank_name','bank_account','net'],
     'employee_code,employee_name,bank_name,bank_account,amount',
     TRUE,
     'needs accountant sign-off');

-- Employee acknowledgement of a payslip.
CREATE TABLE payslip_acknowledgements (
    payslip_id UUID PRIMARY KEY REFERENCES payslips(id) ON DELETE CASCADE,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    acknowledged_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ip_address INET
);

CREATE INDEX idx_payslip_ack_tenant ON payslip_acknowledgements(tenant_id);

ALTER TABLE payslip_acknowledgements ENABLE ROW LEVEL SECURITY;
ALTER TABLE payslip_acknowledgements FORCE ROW LEVEL SECURITY;

CREATE POLICY payslip_ack_isolation ON payslip_acknowledgements
    USING (tenant_id = current_tenant_id())
    WITH CHECK (tenant_id = current_tenant_id());

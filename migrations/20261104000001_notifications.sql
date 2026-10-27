-- Outbox for async notification delivery.
CREATE TABLE IF NOT EXISTS notifications (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    channel TEXT NOT NULL CHECK (channel IN ('email', 'line')),
    recipient TEXT NOT NULL,
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'sent', 'failed')),
    attempts INT NOT NULL DEFAULT 0,
    last_error TEXT,
    sent_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_notifications_tenant_status
    ON notifications (tenant_id, status, created_at);

-- Tenant isolation: enforced by the app connection pool via app.current_tenant_id.
ALTER TABLE notifications ENABLE ROW LEVEL SECURITY;
ALTER TABLE notifications FORCE ROW LEVEL SECURITY;

CREATE POLICY notification_isolation ON notifications
    USING (tenant_id = current_tenant_id());

-- LINE push identifier for users. Task 2 will expose the API/UI to set it.
ALTER TABLE users ADD COLUMN IF NOT EXISTS line_user_id TEXT;

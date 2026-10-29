-- Approval delegation: a manager/HR delegates leave-approval authority to an
-- active tenant user for a date range. `decide_request` falls back to an active
-- delegation when the caller lacks the step role.
CREATE TABLE approval_delegations (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id         UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    delegator_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    delegate_user_id  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    starts_on         DATE NOT NULL,
    ends_on           DATE NOT NULL,
    created_by        UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, delegator_user_id, delegate_user_id, starts_on),
    CHECK (delegator_user_id <> delegate_user_id),
    CHECK (ends_on >= starts_on)
);

-- Active-range lookups in decide_request and the delegate's inbox.
CREATE INDEX idx_approval_delegations_active
    ON approval_delegations (tenant_id, delegate_user_id, starts_on, ends_on);

ALTER TABLE approval_delegations ENABLE ROW LEVEL SECURITY;
ALTER TABLE approval_delegations FORCE ROW LEVEL SECURITY;

CREATE POLICY approval_delegations_isolation ON approval_delegations
    USING (tenant_id = current_tenant_id());

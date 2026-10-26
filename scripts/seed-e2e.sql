-- Deterministic E2E tenant. Destructive for slug 'e2e' only.
CREATE EXTENSION IF NOT EXISTS pgcrypto;
-- Run as the owner role (bypasses RLS) before the gateway starts.
DELETE FROM tenants WHERE slug = 'e2e';

INSERT INTO tenants (id, name, slug)
VALUES ('eeeeeeee-0000-0000-0000-000000000001', 'E2E Co', 'e2e');

INSERT INTO entities (id, tenant_id, name, tax_id, address)
VALUES (
    'eeeeeeee-0000-0000-0000-000000000002',
    'eeeeeeee-0000-0000-0000-000000000001',
    'E2E Main',
    '0105555555555',
    '{"line1":"1 Test Road","province":"Bangkok"}'::jsonb
);

-- 7-day, single-use invite. Token (plain) = e2e-invite-token-4f8a2c1d9b7e6a3f5c0d8e2b1a9f7c6d
INSERT INTO user_invites
    (id, tenant_id, email, full_name_th, role, entity_id, token_hash, expires_at)
VALUES (
    'eeeeeeee-0000-0000-0000-000000000003',
    'eeeeeeee-0000-0000-0000-000000000001',
    'e2e@example.com',
    'ผู้ทดสอบ E2E',
    'hr_admin',
    'eeeeeeee-0000-0000-0000-000000000002',
    digest('e2e-invite-token-4f8a2c1d9b7e6a3f5c0d8e2b1a9f7c6d', 'sha256'),
    now() + interval '7 days'
);

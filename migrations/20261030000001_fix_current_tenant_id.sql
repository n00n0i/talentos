-- COALESCE(text, uuid) is a type error, but it never fired while the dev role was
-- a superuser: RLS (and therefore this function) was skipped. A NOBYPASSRLS role
-- exposes it immediately. Cast the fallback to text, then the result to UUID.
CREATE OR REPLACE FUNCTION current_tenant_id() RETURNS UUID AS $$
BEGIN
    RETURN COALESCE(
        NULLIF(current_setting('app.current_tenant_id', TRUE), ''),
        '00000000-0000-0000-0000-000000000000'
    )::UUID;
END;
$$ LANGUAGE plpgsql STABLE;

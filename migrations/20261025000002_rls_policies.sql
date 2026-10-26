-- Row Level Security: enforced at DB level regardless of application bugs.
-- Connection pool middleware sets: SET LOCAL app.current_tenant_id = '<uuid>';

ALTER TABLE tenants ENABLE ROW LEVEL SECURITY;
ALTER TABLE entities ENABLE ROW LEVEL SECURITY;
ALTER TABLE users ENABLE ROW LEVEL SECURITY;
ALTER TABLE roles ENABLE ROW LEVEL SECURITY;
ALTER TABLE user_roles ENABLE ROW LEVEL SECURITY;
ALTER TABLE employees ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_log ENABLE ROW LEVEL SECURITY;

-- Helper function to read current tenant from session
CREATE OR REPLACE FUNCTION current_tenant_id() RETURNS UUID AS $$
BEGIN
    RETURN COALESCE(
        NULLIF(current_setting('app.current_tenant_id', TRUE), ''),
        '00000000-0000-0000-0000-000000000000'::UUID
    );
END;
$$ LANGUAGE plpgsql STABLE;

-- Tenants: a tenant can only see itself (superuser/bypass role handles platform admin)
CREATE POLICY tenant_isolation ON tenants
    USING (id = current_tenant_id());

-- Entities: scoped to tenant
CREATE POLICY entity_isolation ON entities
    USING (tenant_id = current_tenant_id());

-- Users: scoped to tenant
CREATE POLICY user_isolation ON users
    USING (tenant_id = current_tenant_id());

-- Roles: scoped to tenant
CREATE POLICY role_isolation ON roles
    USING (tenant_id = current_tenant_id());

-- User roles: join through users table implicitly via tenant_id on role
CREATE POLICY user_role_isolation ON user_roles
    USING (
        EXISTS (
            SELECT 1 FROM roles r
            JOIN users u ON u.id = user_roles.user_id
            WHERE r.id = user_roles.role_id
              AND u.tenant_id = current_tenant_id()
        )
    );

-- Employees: scoped to tenant
CREATE POLICY employee_isolation ON employees
    USING (tenant_id = current_tenant_id());

-- Audit log: readable within tenant, insert handled by application with explicit tenant_id
CREATE POLICY audit_isolation ON audit_log
    USING (tenant_id = current_tenant_id());

-- Force RLS even for table owner (except superuser/BYPASSRLS role)
ALTER TABLE tenants FORCE ROW LEVEL SECURITY;
ALTER TABLE entities FORCE ROW LEVEL SECURITY;
ALTER TABLE users FORCE ROW LEVEL SECURITY;
ALTER TABLE roles FORCE ROW LEVEL SECURITY;
ALTER TABLE user_roles FORCE ROW LEVEL SECURITY;
ALTER TABLE employees FORCE ROW LEVEL SECURITY;
ALTER TABLE audit_log FORCE ROW LEVEL SECURITY;
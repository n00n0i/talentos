CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS pgcrypto;
-- Bootstrap/auth role: the ONLY role allowed to bypass RLS, used exclusively by
-- login/refresh token lookup where the tenant is not yet known (spec §1.2).
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'talentos_auth') THEN
        CREATE ROLE talentos_auth LOGIN PASSWORD 'auth_dev_secret' BYPASSRLS;
    END IF;
END $$;

GRANT USAGE ON SCHEMA public TO talentos_auth;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO talentos_auth;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO talentos_auth;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO talentos_auth;

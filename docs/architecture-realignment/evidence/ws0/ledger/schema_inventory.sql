-- Stable catalog inventory used to prove no schema change.
-- Run with: psql -U talentos -d <db> -X -At -f schema_inventory.sql | sha256sum
SELECT 'table:' || table_name
  FROM information_schema.tables
 WHERE table_schema = 'public'
   AND table_type = 'BASE TABLE'
 ORDER BY table_name;

SELECT 'rls:' || c.relname || ':' || c.relrowsecurity || ':' || c.relforcerowsecurity
  FROM pg_class c
  JOIN pg_namespace n ON n.oid = c.relnamespace
 WHERE n.nspname = 'public'
   AND c.relkind = 'r'
 ORDER BY c.relname;

SELECT 'column:' || table_name || ':' || column_name || ':' || data_type || ':'
       || COALESCE(character_maximum_length::text, '') || ':'
       || COALESCE(numeric_precision::text, '') || ':'
       || COALESCE(numeric_scale::text, '') || ':'
       || is_nullable || ':'
       || COALESCE(column_default, '') || ':'
       || ordinal_position
  FROM information_schema.columns
 WHERE table_schema = 'public'
 ORDER BY table_name, ordinal_position;

SELECT 'index:' || indexname || ':' || indexdef
  FROM pg_indexes
 WHERE schemaname = 'public'
 ORDER BY indexname;

SELECT 'constraint:' || con.conname || ':' || con.contype::text || ':' || pg_get_constraintdef(con.oid)
  FROM pg_constraint con
  JOIN pg_class cl ON cl.oid = con.conrelid
  JOIN pg_namespace n ON n.oid = cl.relnamespace
 WHERE n.nspname = 'public'
 ORDER BY con.conname;

SELECT 'policy:' || pol.policyname || ':' || pol.tablename || ':' || pol.permissive || ':'
       || pol.roles::text || ':' || pol.cmd || ':' || COALESCE(pol.qual, '') || ':'
       || COALESCE(pol.with_check, '')
  FROM pg_policies pol
 WHERE pol.schemaname = 'public'
 ORDER BY pol.policyname;

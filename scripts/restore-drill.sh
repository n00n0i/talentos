#!/usr/bin/env bash
# Restore a database dump into a scratch database and verify critical counts.
#
# Required:
#   SRC_DATABASE_URL   admin/superuser URL of the source database
#   DEST_DATABASE_URL  admin/superuser URL of an existing, disposable database
#
# The destination's public schema is dropped and replaced. Never point it at
# production.
set -euo pipefail

: "${SRC_DATABASE_URL:?set SRC_DATABASE_URL}"
: "${DEST_DATABASE_URL:?set DEST_DATABASE_URL}"

if [ "$SRC_DATABASE_URL" = "$DEST_DATABASE_URL" ]; then
  echo "refusing to restore onto the source database" >&2
  exit 1
fi

for bin in pg_dump pg_restore psql; do
  command -v "$bin" >/dev/null || {
    echo "missing $bin (install postgresql-client)" >&2
    exit 1
  }
done

DUMP="$(mktemp /tmp/talentos-restore-XXXXXX.dump)"
trap 'rm -f "$DUMP"' EXIT

echo "dumping source..."
pg_dump --format=custom --no-owner --no-privileges "$SRC_DATABASE_URL" -f "$DUMP"

echo "resetting destination schema..."
psql "$DEST_DATABASE_URL" -v ON_ERROR_STOP=1 -q \
  -c 'DROP SCHEMA IF EXISTS public CASCADE;' \
  -c 'CREATE SCHEMA public;'

echo "restoring..."
pg_restore --no-owner --no-privileges --exit-on-error -d "$DEST_DATABASE_URL" "$DUMP"

TABLES=(tenants employees payroll_runs payslips attendance_events audit_log)
for table in "${TABLES[@]}"; do
  src="$(psql "$SRC_DATABASE_URL" -tAc "SELECT count(*) FROM $table")"
  dst="$(psql "$DEST_DATABASE_URL" -tAc "SELECT count(*) FROM $table")"
  if [ "$src" != "$dst" ]; then
    echo "count mismatch for $table: source=$src restored=$dst" >&2
    exit 1
  fi
  echo "  $table: $dst"
done

src_policies="$(psql "$SRC_DATABASE_URL" -tAc \
  "SELECT count(*) FROM pg_policies WHERE schemaname = 'public'")"
dst_policies="$(psql "$DEST_DATABASE_URL" -tAc \
  "SELECT count(*) FROM pg_policies WHERE schemaname = 'public'")"
if [ "$src_policies" != "$dst_policies" ] || [ "${dst_policies:-0}" -eq 0 ]; then
  echo "RLS policy mismatch: source=$src_policies restored=$dst_policies" >&2
  exit 1
fi
echo "  rls policies: $dst_policies"

echo "RESTORE DRILL OK"

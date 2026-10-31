#!/usr/bin/env bash
# Database backup for on-prem installs. Keep 14 days by default.
set -euo pipefail
cd "$(dirname "$0")/.."

BACKUP_DIR="${BACKUP_DIR:-./backups}"
KEEP="${KEEP:-14}"
mkdir -p "$BACKUP_DIR"
stamp="$(date +%Y%m%d-%H%M%S)"
file="$BACKUP_DIR/talentos-$stamp.sql.gz"

if [ -n "${PG_HOST:-}" ]; then
    # Host mode: psql on the host with DATABASE_ADMIN_URL or PG_* variables.
    PGPASSWORD="${PGPASSWORD:-}" psql -h "$PG_HOST" -U "${PG_USER:-talentos}" \
        -d "${PG_DATABASE:-talentos}" --no-owner --clean --if-exists \
        | gzip > "$file"
else
    # Container mode (default): run pg_dump inside the compose postgres service.
    docker compose exec -T postgres pg_dump -U "${PG_USER:-talentos}" \
        -d "${PG_DATABASE:-talentos}" --no-owner \
        | gzip > "$file"
fi

# Rotate: keep the newest $KEEP dumps, delete older ones.
ls -1t "$BACKUP_DIR"/talentos-*.sql.gz 2>/dev/null | tail -n +$((KEEP + 1)) |
    xargs -r rm -f
printf 'backup written: %s (%s, keep %s)\n' "$file" "$(du -h "$file" | cut -f1)" "$KEEP"

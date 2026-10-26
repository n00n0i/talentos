#!/usr/bin/env bash
# Database backup for on-prem installs. Keep 14 days by default.
set -euo pipefail
cd "$(dirname "$0")/.."

BACKUP_DIR="${BACKUP_DIR:-./backups}"
RETENTION_DAYS="${RETENTION_DAYS:-14}"
mkdir -p "$BACKUP_DIR"
stamp="$(date +%Y%m%d-%H%M%S)"
file="$BACKUP_DIR/talentos-$stamp.sql.gz"

docker compose exec -T postgres pg_dump -U talentos -d talentos --no-owner \
    | gzip > "$file"
find "$BACKUP_DIR" -name 'talentos-*.sql.gz' -mtime +"$RETENTION_DAYS" -delete
printf 'backup written: %s (%s)\n' "$file" "$(du -h "$file" | cut -f1)"

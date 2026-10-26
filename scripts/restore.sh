#!/usr/bin/env bash
# Restore a backup. Stops the app first so nothing writes during the restore.
set -euo pipefail
cd "$(dirname "$0")/.."

file="${1:-}"
[[ -n "$file" && -f "$file" ]] || { echo "usage: $0 <backup.sql.gz>" >&2; exit 1; }
if [[ "${2:-}" != "--force" ]]; then
    read -r -p "This overwrites the current database. Type 'restore' to continue: " answer
    [[ "$answer" == "restore" ]] || { echo "aborted"; exit 1; }
fi

docker compose stop gateway web nginx 2>/dev/null || true
gunzip -c "$file" | docker compose exec -T postgres psql -U talentos -d talentos
docker compose up -d gateway web nginx
echo "restored $file"

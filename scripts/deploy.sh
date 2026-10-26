#!/usr/bin/env bash
# Compose-based deploy for on-prem: migrate, rebuild app images, health-check.
# Rollback tags the previous gateway/web images and starts them again.
set -euo pipefail
cd "$(dirname "$0")/.."

MODE="${1:-deploy}"
HEALTH_URL="${HEALTH_URL:-http://localhost/health}"
TIMEOUT="${HEALTH_TIMEOUT:-120}"
PREV_DIR=".deploy"

log() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
die() { log "ERROR: $*" >&2; exit 1; }
wait_healthy() {
    local deadline=$((SECONDS + TIMEOUT))
    until curl -fsS "$HEALTH_URL" >/dev/null 2>&1; do
        (( SECONDS >= deadline )) && die "health check failed: $HEALTH_URL"
        sleep 2
    done
}

command -v docker >/dev/null || die "docker not found"
command -v curl >/dev/null || die "curl not found"
mkdir -p "$PREV_DIR"

if [[ "$MODE" == "--rollback" ]]; then
    [[ -f "$PREV_DIR/gateway.id" && -f "$PREV_DIR/web.id" ]] \
        || die "no previous images recorded in $PREV_DIR"
    docker tag "$(cat "$PREV_DIR/gateway.id")" talentos-gateway:latest
    docker tag "$(cat "$PREV_DIR/web.id")" talentos-web:latest
    docker compose up -d --no-build gateway web nginx
    wait_healthy
    log "rolled back"
    exit 0
fi

# Record current images for rollback.
for svc in gateway web; do
    id="$(docker compose images -q "$svc" 2>/dev/null | head -1 || true)"
    [[ -n "$id" ]] && echo "$id" > "$PREV_DIR/$svc.id"
done

log "applying migrations + roles"
docker compose run --rm migrate

log "building and starting the app"
docker compose up -d --build gateway web nginx
wait_healthy
log "deployed: $(curl -fsS "$HEALTH_URL")"

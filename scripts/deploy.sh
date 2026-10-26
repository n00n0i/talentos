#!/usr/bin/env bash
# Zero-downtish deployment for TalentOS on-prem.
# Flow: tag current -> build new -> run migrations -> start new container alongside old ->
#       health-check new -> switch nginx upstream -> drain old -> cleanup.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$ROOT_DIR"

COMPOSE_FILE="docker-compose.yml"
NEW_TAG="talentos/api-gw:latest"
PREV_TAG_FILE=".last_deployed_tag"
HEALTH_ENDPOINT="http://localhost:8080/health"
HEALTH_TIMEOUT=60   # seconds to wait for healthy response
DRAIN_WAIT=10       # give in-flight requests time to finish before killing old

MODE="${1:-deploy}"

log() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
die() { log "ERROR: $*" >&2; exit 1; }

require_cmd() { command -v "$1" >/dev/null 2>&1 || die "Required command not found: $1"; }
for cmd in docker cargo curl git sqlx; do require_cmd "$cmd"; done

# ---------- ROLLBACK MODE ----------
if [[ "$MODE" == "--rollback" ]]; then
    [[ -f "$PREV_TAG_FILE" ]] || die "No previous deployment recorded ($PREV_TAG_FILE)."
    PREV_TAG="$(cat "$PREV_TAG_FILE")"
    log "Rolling back to $PREV_TAG"
    docker tag "$PREV_TAG" "$NEW_TAG"
    docker compose -f "$COMPOSE_FILE" up -d --no-build api-gw
    log "Waiting for health check..."
    deadline=$((SECONDS + HEALTH_TIMEOUT))
    until curl -sf "$HEALTH_ENDPOINT" >/dev/null; do
        (( SECONDS >= deadline )) && die "Rollback target failed health check."
        sleep 2
    done
    log "Rolled back to $PREV_TAG"
    exit 0
fi

# ---------- DEPLOY MODE ----------
GIT_SHA="$(git rev-parse --short HEAD 2>/dev/null || echo 'local')"
BUILD_TAG="talentos/api-gw:${GIT_SHA}-$(date +%Y%m%d%H%M%S)"
log "Building $BUILD_TAG"

# 1. Compile release binary locally first to catch errors early.
cargo build --release -p talentos-gateway

# 2. Record what's currently deployed so rollback has a target.
CURRENT_IMAGE="$(docker inspect -f '{{.Config.Image}}' talentos-api-gw-1 2>/dev/null || echo "")"
if [[ -n "$CURRENT_IMAGE" ]]; then
    echo "$CURRENT_IMAGE" > "$PREV_TAG_FILE"
    log "Previous deployment saved: $CURRENT_IMAGE"
fi

# 3. Build new image.
docker build -t "$BUILD_TAG" -t "$NEW_TAG" -f Dockerfile .

# 4. Run migrations BEFORE swapping traffic. Migrations use the owner role; the
# container itself runs as DATABASE_APP_URL (NOBYPASSRLS), or RLS is disabled.
: "${DATABASE_APP_URL:?set DATABASE_APP_URL (NOBYPASSRLS role; DATABASE_URL is used only for migrations)}"
: "${DATABASE_AUTH_URL:?set DATABASE_AUTH_URL (BYPASSRLS role for login/refresh)}"
log "Running migrations..."
DATABASE_URL="$DATABASE_URL" sqlx migrate run || die "Migration failed — aborting before traffic swap."

# 5. Start new container on an alternate port so it doesn't conflict with old.
NEW_PORT=18080
log "Starting candidate container on :$NEW_PORT ..."
CID_NEW="$(docker run -d --rm \
    --name talentos-candidate \
    --network "${COMPOSE_FILE%.yml}_default" \
    -e DATABASE_URL="$DATABASE_APP_URL" \
    -e DATABASE_AUTH_URL="$DATABASE_AUTH_URL" \
    -e REDIS_URL="$REDIS_URL" \
    -e AUTH_JWT_SECRET="$AUTH_JWT_SECRET" \
    -e GATEWAY_PORT="$NEW_PORT" \
    -p "$NEW_PORT:$NEW_PORT" \
    "$BUILD_TAG")"

cleanup_candidate() { docker rm -f "$CID_NEW" 2>/dev/null || true; }
trap cleanup_candidate ERR

# 6. Health-check candidate directly (bypass nginx).
log "Health-checking candidate..."
deadline=$((SECONDS + HEALTH_TIMEOUT))
until curl -sf "http://localhost:$NEW_PORT/health" | grep -q '"status":"ok"'; do
    (( SECONDS >= deadline )) && die "Candidate failed health check within ${HEALTH_TIMEOUT}s."
    sleep 2
done
log "Candidate healthy."

# 7. Swap: stop old, promote new to canonical name/port.
log "Swapping traffic..."
docker compose -f "$COMPOSE_FILE" stop api-gw
docker rm -f "$CID_NEW" 2>/dev/null || trap - ERR   # disown so it survives our shell
docker compose -f "$COMPOSE_FILE" up -d api-gw      # picks NEW_TAG because we retagged above

# 8. Final verification through the public endpoint.
sleep "$DRAIN_WAIT"
deadline=$((SECONDS + HEALTH_TIMEOUT))
until curl -sf "$HEALTH_ENDPOINT" | grep -q '"status":"ok"'; do
    (( SECONDS >= deadline )) && die "Production endpoint unhealthy after swap!"
    sleep 2
done

log "Deploy complete: $BUILD_TAG"
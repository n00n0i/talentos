#!/usr/bin/env bash
# Idempotent OpenBao bootstrap for TalentOS provider secrets.
#
# Writes a static-auto-unseal config under /etc/openbao, starts the container,
# enables KV v2 at secret/, applies the scoped `talentos-ai` policy, and ensures
# a token. Re-running is safe; it reuses the container, seal key, and token.
set -euo pipefail

CONF_DIR=/etc/openbao
CONF_FILE="$CONF_DIR/openbao.hcl"
SEAL_KEY="$CONF_DIR/seal.key"
DATA_DIR="$CONF_DIR/data"
ROOT_TOKEN_FILE="$CONF_DIR/root.token"
AI_TOKEN_FILE="$CONF_DIR/ai.token"
CONTAINER=openbao
IMAGE=openbao/openbao:2.6.0
OPENBAO_ADDR=${OPENBAO_ADDR:-http://127.0.0.1:8200}
BAO_ADDR="$OPENBAO_ADDR"
export BAO_ADDR

if [ "$(id -u)" -eq 0 ]; then
    SUDO=()
else
    SUDO=(sudo)
fi

log() { echo "==> $*"; }

# --- config ----------------------------------------------------------------
CONFIG_CONTENT=$(
    cat <<'EOF'
storage "file" {
  path = "/openbao/data"
}

listener "tcp" {
  address     = "0.0.0.0:8200"
  tls_disable = 1
}

seal "static" {
  current_key_id = "2026-11"
  current_key    = "file:///openbao/seal.key"
}

api_addr = "http://127.0.0.1:8200"
ui       = false
EOF
)

TMP_CONF=$(mktemp)
trap 'rm -f "$TMP_CONF"' EXIT
printf '%s\n' "$CONFIG_CONTENT" >"$TMP_CONF"

"${SUDO[@]}" mkdir -p "$CONF_DIR" "$DATA_DIR"
CONFIG_CHANGED=0
if ! "${SUDO[@]}" test -f "$CONF_FILE" || ! "${SUDO[@]}" cmp -s "$TMP_CONF" "$CONF_FILE"; then
    "${SUDO[@]}" install -m 0644 "$TMP_CONF" "$CONF_FILE"
    CONFIG_CHANGED=1
    log "wrote $CONF_FILE"
fi

valid_seal_key() {
    local size
    size=$("${SUDO[@]}" stat -c %s "$SEAL_KEY" 2>/dev/null || echo 0)
    case "$size" in
        32 | 43 | 44 | 64) return 0 ;;
        *) return 1 ;;
    esac
}

if ! valid_seal_key; then
    # 32-byte hex key (64 hex chars, no trailing newline); container uid 100/gid 1000.
    openssl rand -hex 32 | tr -d '\n' | "${SUDO[@]}" tee "$SEAL_KEY" >/dev/null
    "${SUDO[@]}" chown 100:1000 "$SEAL_KEY"
    "${SUDO[@]}" chmod 0600 "$SEAL_KEY"
    log "generated $SEAL_KEY"
fi

# The container user (100:1000) must traverse the mount and write the data dir.
"${SUDO[@]}" chmod 0755 "$CONF_DIR"
"${SUDO[@]}" chown -R 100:1000 "$DATA_DIR"
"${SUDO[@]}" chmod 0700 "$DATA_DIR"

# --- container -------------------------------------------------------------
if docker container inspect "$CONTAINER" >/dev/null 2>&1; then
    RUNNING=$(docker inspect -f '{{.State.Running}}' "$CONTAINER")
    if [ "$CONFIG_CHANGED" = 1 ] && [ "$RUNNING" = true ]; then
        log "config changed; restarting $CONTAINER"
        docker restart "$CONTAINER" >/dev/null
    elif [ "$RUNNING" != true ]; then
        log "starting existing $CONTAINER"
        docker start "$CONTAINER" >/dev/null
    fi
else
    log "creating $CONTAINER"
    # Bypass the image entrypoint: it forces -config=/openbao/config.
    docker run -d \
        --name "$CONTAINER" \
        --restart unless-stopped \
        --cap-add=IPC_LOCK \
        -p 127.0.0.1:8200:8200 \
        -v "$CONF_DIR:/openbao:Z" \
        --entrypoint /usr/bin/bao \
        "$IMAGE" server -config=/openbao/openbao.hcl >/dev/null
fi

# --- wait for the API ------------------------------------------------------
for _ in $(seq 1 60); do
    if curl -s -o /dev/null "$BAO_ADDR/v1/sys/health"; then
        break
    fi
    sleep 1
done
if ! curl -s -o /dev/null "$BAO_ADDR/v1/sys/health"; then
    echo "ERROR: OpenBao did not answer on $BAO_ADDR" >&2
    docker logs --tail 40 "$CONTAINER" >&2 || true
    exit 1
fi

status_json() {
    # `bao status` exits 2 while sealed but still prints JSON.
    bao status -format=json 2>/dev/null || true
}

INITIALIZED=$(status_json | jq -r '.initialized')
if [ "$INITIALIZED" != true ]; then
    log "initializing OpenBao (static seal, recovery keys unused)"
    init_json=$(bao operator init -format=json -recovery-shares=1 -recovery-threshold=1)
    root_token=$(jq -r '.root_token' <<<"$init_json")
    printf '%s' "$root_token" | "${SUDO[@]}" tee "$ROOT_TOKEN_FILE" >/dev/null
    "${SUDO[@]}" chmod 0600 "$ROOT_TOKEN_FILE"
fi

if "${SUDO[@]}" test -s "$ROOT_TOKEN_FILE"; then
    ROOT_TOKEN=$("${SUDO[@]}" cat "$ROOT_TOKEN_FILE")
else
    ROOT_TOKEN=${OPENBAO_ROOT_TOKEN:-}
fi
if [ -z "$ROOT_TOKEN" ]; then
    echo "ERROR: OpenBao is initialized but $ROOT_TOKEN_FILE is missing." >&2
    echo "       Re-run with OPENBAO_ROOT_TOKEN=<root token> to repair." >&2
    exit 1
fi
export BAO_TOKEN="$ROOT_TOKEN"

for _ in $(seq 1 30); do
    if status_json | jq -e '.sealed == false' >/dev/null; then
        break
    fi
    sleep 1
done
if ! status_json | jq -e '.sealed == false' >/dev/null; then
    echo "ERROR: OpenBao is still sealed" >&2
    exit 1
fi
log "OpenBao unsealed"

# --- KV v2 mount -----------------------------------------------------------
mounts=$(bao secrets list -format=json)
if ! jq -e 'has("secret/")' >/dev/null <<<"$mounts"; then
    log "enabling kv-v2 at secret/"
    bao secrets enable -path=secret -version=2 kv >/dev/null
else
    version=$(jq -r '."secret/".options.version // "1"' <<<"$mounts")
    if [ "$version" != 2 ]; then
        log "upgrading secret/ to kv-v2"
        bao kv enable-versioning secret/ >/dev/null
    fi
fi

# --- policy ----------------------------------------------------------------
log "applying policy talentos-ai"
bao policy write talentos-ai - >/dev/null <<'EOF'
path "secret/data/talentos/*" {
  capabilities = ["create", "read", "update", "delete"]
}

path "secret/metadata/talentos/*" {
  capabilities = ["create", "read", "update", "delete"]
}
EOF

# --- token -----------------------------------------------------------------
AI_TOKEN=""
if "${SUDO[@]}" test -s "$AI_TOKEN_FILE"; then
    AI_TOKEN=$("${SUDO[@]}" cat "$AI_TOKEN_FILE")
fi
if [ -z "$AI_TOKEN" ] || ! bao token lookup "$AI_TOKEN" >/dev/null 2>&1; then
    log "creating token talentos-ai-token"
    # shortcut: fixed one-year token, re-run to rotate; AppRole is the follow-up.
    token_json=$(bao token create \
        -display-name=talentos-ai-token \
        -policy=talentos-ai \
        -no-default-policy \
        -ttl=8760h \
        -format=json)
    AI_TOKEN=$(jq -r '.auth.client_token' <<<"$token_json")
    printf '%s' "$AI_TOKEN" | "${SUDO[@]}" tee "$AI_TOKEN_FILE" >/dev/null
    "${SUDO[@]}" chmod 0600 "$AI_TOKEN_FILE"
fi

echo
echo "OpenBao ready. Export these in the gateway environment:"
echo "export OPENBAO_ADDR=\"$OPENBAO_ADDR\""
echo "export OPENBAO_TOKEN=\"$AI_TOKEN\""
echo "export OPENBAO_ENABLED=true"

#!/usr/bin/env bash
# Start/verify the isolated E2E stack: gateway :18480 -> talentos_e2e,
# web :3100 -> TALENTOS_API_URL=http://127.0.0.1:18480.
#
# Ownership rule: port 3100 may only be held by the process whose cwd is
# web/.next/standalone and whose command is the Next standalone server.
# Any other owner is reported and left alone.
#
# Usage: scripts/e2e-stack.sh [up|status|down]
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WEB_DIR="$ROOT/web/.next/standalone"
GW_PORT="${E2E_GATEWAY_PORT:-18480}"
WEB_PORT="${E2E_WEB_PORT:-3100}"
EXPECTED_DB="talentos_e2e"
GW_LOG="/tmp/talentos-e2e-gateway.log"
WEB_LOG="/tmp/talentos-e2e-web.log"

NODE_BIN="${E2E_NODE_BIN:-$(command -v node)}"
GW_ENV="${E2E_GATEWAY_ENV:-/etc/talentos-gateway.env}"

gw_env_value() { # <key> from the gateway env file (sudo only if needed)
    local key="$1" content=""
    if [ -r "$GW_ENV" ]; then
        content="$(cat "$GW_ENV" 2>/dev/null)"
    elif command -v sudo >/dev/null 2>&1 && sudo -n test -r "$GW_ENV" 2>/dev/null; then
        content="$(sudo -n cat "$GW_ENV" 2>/dev/null)"
    fi
    printf '%s\n' "$content" | sed -n "s/^${key}=//p" | tail -1
}

listeners() {
    ss -lntpH "sport = :$1" 2>/dev/null | grep -o 'pid=[0-9]*' | cut -d= -f2 | sort -u
}

is_e2e_web() {
    local pid="$1" cwd cmd
    cwd="$(readlink "/proc/$pid/cwd" 2>/dev/null)" || return 1
    cmd="$(tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null)" || return 1
    [ "$cwd" = "$WEB_DIR" ] || return 1
    # Next rewrites the process title to "next-server (vX)"; the executable is
    # still the standalone server.js started from this cwd.
    case "$cmd" in
        *server.js*|*next-server*) return 0 ;;
        *) return 1 ;;
    esac
}

is_e2e_gateway() {
    local pid="$1" cwd cmd
    cwd="$(readlink "/proc/$pid/cwd" 2>/dev/null)" || return 1
    cmd="$(tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null)" || return 1
    [ "$cwd" = "$ROOT" ] || return 1
    case "$cmd" in
        *talentos-gateway*) return 0 ;;
        *) return 1 ;;
    esac
}

gw_env_ok() {
    local pid="$1" env
    env="$(tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null)" || return 1
    printf '%s\n' "$env" | grep -qx "DATABASE_URL=postgres://talentos_app:app_dev_secret@localhost:5432/$EXPECTED_DB" || return 1
    printf '%s\n' "$env" | grep -qx "GATEWAY_PORT=$GW_PORT" || return 1
    printf '%s\n' "$env" | grep -q '^PII_ENCRYPTION_KEY=..' || return 1
}

stop_pid() {
    local pid="$1" i
    kill -TERM "$pid" 2>/dev/null || return 0
    for i in $(seq 1 50); do
        kill -0 "$pid" 2>/dev/null || return 0
        sleep 0.2
    done
    kill -KILL "$pid" 2>/dev/null || true
}

stop_web_owners() {
    local pid
    for pid in $(listeners "$WEB_PORT"); do
        if is_e2e_web "$pid"; then
            echo "stopping E2E web pid $pid"
            stop_pid "$pid"
        else
            echo "REFUSING to stop pid $pid on :$WEB_PORT (cwd=$(readlink "/proc/$pid/cwd" 2>/dev/null), cmd=$(tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null))" >&2
            return 1
        fi
    done
}

start_web() {
    if [ ! -f "$WEB_DIR/server.js" ]; then
        echo "FAIL  web: $WEB_DIR/server.js not found (run 'next build')" >&2
        return 1
    fi
    (
        cd "$WEB_DIR" || exit 1
        PORT="$WEB_PORT" HOSTNAME=0.0.0.0 \
            TALENTOS_API_URL="http://127.0.0.1:$GW_PORT" \
            TALENTOS_COOKIE_SECURE=false NODE_ENV=production \
            nohup "$NODE_BIN" server.js >"$WEB_LOG" 2>&1 &
        echo $! >/tmp/talentos-e2e-web.pid
    )
}

start_gateway() {
    local token="" pii=""
    if [ -r /etc/openbao/ai.token ]; then
        token="$(cat /etc/openbao/ai.token)"
    elif command -v sudo >/dev/null 2>&1; then
        token="$(sudo -n cat /etc/openbao/ai.token 2>/dev/null || true)"
    fi
    pii="${PII_ENCRYPTION_KEY:-$(gw_env_value PII_ENCRYPTION_KEY)}"
    if [ -z "$pii" ]; then
        echo "FAIL  gateway: PII_ENCRYPTION_KEY is not configured (2FA/OIDC would 503)" >&2
        return 1
    fi
    (
        cd "$ROOT" || exit 1
        env \
            DATABASE_URL="postgres://talentos_app:app_dev_secret@localhost:5432/$EXPECTED_DB" \
            DATABASE_AUTH_URL="postgres://talentos_auth:auth_dev_secret@localhost:5432/$EXPECTED_DB" \
            DATABASE_ADMIN_URL="postgres://talentos:talentos_dev_secret@localhost:5432/$EXPECTED_DB" \
            REDIS_URL="redis://:redis_dev_secret@127.0.0.1:6379" \
            AUTH_JWT_SECRET="dev-secret-change-me-in-prod-minimum-32-bytes-okay!!" \
            GATEWAY_PORT="$GW_PORT" \
            SETUP_TOKEN=e2e-setup-token \
            TALENTOS_COOKIE_SECURE=false \
            OPENBAO_ENABLED=true \
            OPENBAO_ADDR=http://127.0.0.1:8200 \
            OPENBAO_TOKEN="$token" \
            PII_ENCRYPTION_KEY="$pii" \
            nohup ./target/debug/talentos-gateway >"$GW_LOG" 2>&1 &
        echo $! >/tmp/talentos-e2e-gateway.pid
    )
}

gw_health_ok() {
    curl -fsS "http://127.0.0.1:$GW_PORT/health" 2>/dev/null | grep -q '"database":"ok"'
}

web_health_ok() {
    [ "$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$WEB_PORT/login")" = "200" ]
}

wait_health() {
    local i
    for i in $(seq 1 40); do
        if gw_health_ok && web_health_ok; then return 0; fi
        sleep 1
    done
    return 1
}

up() {
    # --- gateway ---
    local pid
    pid="$(listeners "$GW_PORT" | head -1)"
    if [ -n "$pid" ] && is_e2e_gateway "$pid" && gw_env_ok "$pid" && gw_health_ok; then
        echo "PASS  gateway :$GW_PORT already healthy (pid $pid)"
    else
        if [ -n "$pid" ]; then
            if is_e2e_gateway "$pid"; then
                echo "stopping E2E gateway pid $pid"
                stop_pid "$pid"
            else
                echo "REFUSING to stop pid $pid on :$GW_PORT (not a talentos-gateway)" >&2
                return 1
            fi
        fi
        start_gateway
        echo "PASS  gateway :$GW_PORT started (pid $(cat /tmp/talentos-e2e-gateway.pid 2>/dev/null))"
    fi

    # --- web: one owner only ---
    stop_web_owners || return 1
    start_web
    echo "PASS  web :$WEB_PORT started (pid $(cat /tmp/talentos-e2e-web.pid 2>/dev/null))"

    if ! wait_health; then
        echo "FAIL  health: gateway or web did not become healthy" >&2
        echo "--- $GW_LOG ---"; tail -20 "$GW_LOG" 2>/dev/null
        echo "--- $WEB_LOG ---"; tail -20 "$WEB_LOG" 2>/dev/null
        return 1
    fi

    local web_owners gw_owners web_pid
    pid="$(listeners "$GW_PORT" | head -1)"
    web_owners="$(listeners "$WEB_PORT" | wc -l)"
    gw_owners="$(listeners "$GW_PORT" | wc -l)"
    [ "$web_owners" -eq 1 ] && echo "PASS  web port ownership (1 listener)" \
        || { echo "FAIL  web port ownership ($web_owners listeners)" >&2; return 1; }
    [ "$gw_owners" -eq 1 ] && echo "PASS  gateway port ownership (1 listener)" \
        || { echo "FAIL  gateway port ownership ($gw_owners listeners)" >&2; return 1; }

    web_pid="$(listeners "$WEB_PORT" | head -1)"
    is_e2e_web "$web_pid" && echo "PASS  web owner cwd+cmd match" \
        || { echo "FAIL  web owner does not match $WEB_DIR server.js" >&2; return 1; }
    tr '\0' '\n' <"/proc/$web_pid/environ" 2>/dev/null | grep -qx "TALENTOS_API_URL=http://127.0.0.1:$GW_PORT" \
        && echo "PASS  web TALENTOS_API_URL=http://127.0.0.1:$GW_PORT" \
        || { echo "FAIL  web TALENTOS_API_URL mismatch" >&2; return 1; }

    tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null | grep -qx "DATABASE_URL=postgres://talentos_app:app_dev_secret@localhost:5432/$EXPECTED_DB" \
        && echo "PASS  gateway DATABASE_URL -> $EXPECTED_DB" \
        || { echo "FAIL  gateway DATABASE_URL mismatch" >&2; return 1; }
    echo "PASS  gateway health database=ok"
    echo "PASS  web health /login=200"
}

status() {
    local pid
    for port in "$GW_PORT" "$WEB_PORT"; do
        for pid in $(listeners "$port"); do
            echo ":$port pid=$pid cwd=$(readlink "/proc/$pid/cwd" 2>/dev/null) cmd=$(tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null)"
        done
    done
    gw_health_ok && echo "gateway health ok" || echo "gateway health DOWN"
    web_health_ok && echo "web health ok" || echo "web health DOWN"
}

down() {
    local pid
    for pid in $(listeners "$WEB_PORT"); do
        is_e2e_web "$pid" && stop_pid "$pid" || echo "skipping unrelated pid $pid on :$WEB_PORT" >&2
    done
    for pid in $(listeners "$GW_PORT"); do
        is_e2e_gateway "$pid" && stop_pid "$pid" || echo "skipping unrelated pid $pid on :$GW_PORT" >&2
    done
    echo "E2E stack stopped"
}

case "${1:-up}" in
    up)     up ;;
    status) status ;;
    down)   down ;;
    *) echo "usage: $0 [up|status|down]" >&2; exit 2 ;;
esac

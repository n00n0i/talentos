#!/usr/bin/env bash
# Guarded destructive reset of the isolated E2E database.
#
# Usage:
#   E2E_ALLOW_DESTRUCTIVE=1 E2E_DB=talentos_e2e scripts/e2e-reset.sh
#
# Connection resolution (first match wins):
#   --dbname <dsn|name>  >  E2E_RESET_DATABASE_URL  >  DATABASE_ADMIN_URL
#   >  DATABASE_URL  >  gateway env file  >  local default
#
# Every resolved connection must be exactly the disposable database
# "talentos_e2e". Anything else is refused.
set -uo pipefail

EXPECTED_DB="talentos_e2e"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INIT_SQL="$ROOT/scripts/init-db.sql"
GW_ENV="${E2E_GATEWAY_ENV:-/etc/talentos-gateway.env}"

FAILED=0
pass() { printf 'PASS  %-24s %s\n' "$1" "${2:-}"; }
fail() { FAILED=1; printf 'FAIL  %-24s %s\n' "$1" "${2:-}"; }
finish() {
    if [ "$FAILED" -eq 0 ]; then
        echo "RESULT: PASS"
        exit 0
    fi
    echo "RESULT: FAIL"
    exit 1
}

# --- 1. Explicit consent -----------------------------------------------------
if [ "${E2E_ALLOW_DESTRUCTIVE:-}" != "1" ]; then
    fail destructive-flag "E2E_ALLOW_DESTRUCTIVE must be 1"
    echo "RESULT: FAIL"
    exit 2
fi
pass destructive-flag "E2E_ALLOW_DESTRUCTIVE=1"

# --- 2. Target database name is exact ---------------------------------------
if [ "${E2E_DB:-}" = "$EXPECTED_DB" ]; then
    pass e2e-db "E2E_DB=$EXPECTED_DB"
else
    fail e2e-db "E2E_DB must be exactly '$EXPECTED_DB' (got '${E2E_DB:-<unset>}')"
fi

# --- 3. Helpers --------------------------------------------------------------
# Only the exact disposable name is allowed. Everything else (including names
# we do not recognise) is refused; protected names get a clearer message.
classify_db() {
    case "$1" in
        "$EXPECTED_DB") echo disposable ;;
        talentos|postgres|template0|template1) echo protected ;;
        staging*|prod*|demo*) echo protected ;;
        "") echo unknown ;;
        *) echo unknown ;;
    esac
}

dsn_dbname() {
    local dsn="$1" rest after db
    case "$dsn" in
        postgres://*|postgresql://*)
            rest="${dsn#*://}"
            after="${rest#*/}"
            [ "$after" = "$rest" ] && { printf ''; return; }
            db="${after%%\?*}"
            db="${db%%/*}"
            printf '%s' "$db"
            ;;
        *dbname=*)
            db="${dsn##*dbname=}"
            db="${db%% *}"
            db="${db%%&*}"
            printf '%s' "$db"
            ;;
        *) printf '' ;;
    esac
}

check_dbname() { # <check-name> <source> <dbname>
    local name="$1" src="$2" db="$3"
    local cls
    cls="$(classify_db "$db")"
    if [ "$cls" = "disposable" ]; then
        pass "$name" "$src -> $db"
    elif [ "$cls" = "protected" ]; then
        fail "$name" "$src -> '$db' is protected, refusing"
    else
        fail "$name" "$src -> '${db:-<none>}' cannot be classified as disposable"
    fi
}

# --- 4. Optional CLI override -----------------------------------------------
OVERRIDE=""
while [ $# -gt 0 ]; do
    case "$1" in
        --dbname)   OVERRIDE="${2:-}"; shift 2 ;;
        --dbname=*) OVERRIDE="${1#*=}"; shift ;;
        *)          fail cli-args "unknown argument: $1"; shift ;;
    esac
done
if [ -n "$OVERRIDE" ]; then
    case "$OVERRIDE" in
        *://*|*dbname=*) OV_DB="$(dsn_dbname "$OVERRIDE")" ;;
        *)               OV_DB="$OVERRIDE" ;;
    esac
    check_dbname dbname-override "--dbname" "$OV_DB"
fi

# --- 5. Resolve the connection actually used --------------------------------
GW_CONTENT=""
if [ -r "$GW_ENV" ]; then
    GW_CONTENT="$(cat "$GW_ENV" 2>/dev/null)"
elif command -v sudo >/dev/null 2>&1 && sudo -n test -r "$GW_ENV" 2>/dev/null; then
    GW_CONTENT="$(sudo -n cat "$GW_ENV" 2>/dev/null)"
fi

RESOLVED_DSN=""
RESOLVED_SRC=""
if [ -n "$OVERRIDE" ]; then
    case "$OVERRIDE" in
        *://*|*dbname=*) RESOLVED_DSN="$OVERRIDE" ;;
        *) RESOLVED_DSN="postgres://talentos:talentos_dev_secret@localhost:5432/$OVERRIDE" ;;
    esac
    RESOLVED_SRC="--dbname override"
elif [ -n "${E2E_RESET_DATABASE_URL:-}" ]; then
    RESOLVED_DSN="$E2E_RESET_DATABASE_URL"
    RESOLVED_SRC="env E2E_RESET_DATABASE_URL"
elif [ -n "${DATABASE_ADMIN_URL:-}" ]; then
    RESOLVED_DSN="$DATABASE_ADMIN_URL"
    RESOLVED_SRC="env DATABASE_ADMIN_URL"
elif [ -n "${DATABASE_URL:-}" ]; then
    RESOLVED_DSN="$DATABASE_URL"
    RESOLVED_SRC="env DATABASE_URL"
else
    GW_DB_DSN=""
    if [ -n "$GW_CONTENT" ]; then
        GW_ADMIN="$(printf '%s\n' "$GW_CONTENT" | sed -n 's/^DATABASE_ADMIN_URL=//p' | tail -1)"
        GW_APP="$(printf '%s\n' "$GW_CONTENT" | sed -n 's/^DATABASE_URL=//p' | tail -1)"
        GW_DB_DSN="${GW_ADMIN:-$GW_APP}"
    fi
    if [ -n "$GW_DB_DSN" ]; then
        RESOLVED_DSN="$GW_DB_DSN"
        RESOLVED_SRC="gateway env $GW_ENV"
    else
        RESOLVED_DSN="postgres://talentos:talentos_dev_secret@localhost:5432/$EXPECTED_DB"
        RESOLVED_SRC="local default"
    fi
fi
check_dbname connection-target "$RESOLVED_SRC" "$(dsn_dbname "$RESOLVED_DSN")"

# --- 6. Every other configured connection that could be used by the stack ---
if [ -n "${DATABASE_AUTH_URL:-}" ]; then
    check_dbname auth-url-target "env DATABASE_AUTH_URL" "$(dsn_dbname "$DATABASE_AUTH_URL")"
else
    pass auth-url-target "env DATABASE_AUTH_URL not set"
fi

if [ -n "$GW_CONTENT" ]; then
    GW_DB=""
    for key in DATABASE_ADMIN_URL DATABASE_URL; do
        val="$(printf '%s\n' "$GW_CONTENT" | sed -n "s/^${key}=//p" | tail -1)"
        if [ -n "$val" ]; then GW_DB="$(dsn_dbname "$val")"; break; fi
    done
    if [ -z "$GW_DB" ]; then
        pass gateway-env "$GW_ENV has no database DSN"
    elif [ "$GW_DB" = "$EXPECTED_DB" ]; then
        pass gateway-env "$GW_ENV -> $GW_DB"
    elif [ "$RESOLVED_SRC" != "gateway env $GW_ENV" ]; then
        pass gateway-env "$GW_ENV -> $GW_DB (not used; overridden by $RESOLVED_SRC)"
    else
        fail gateway-env "$GW_ENV -> '$GW_DB' is not disposable"
    fi
else
    pass gateway-env "$GW_ENV not readable (not used)"
fi

[ "$FAILED" -eq 0 ] || finish

# --- 7. psql runner ----------------------------------------------------------
if [ -n "${E2E_PSQL:-}" ]; then
    read -r -a PSQL <<< "$E2E_PSQL"
elif command -v psql >/dev/null 2>&1; then
    PSQL=(psql)
elif command -v docker >/dev/null 2>&1 && docker inspect loans-postgres-1 >/dev/null 2>&1; then
    PSQL=(docker exec -i loans-postgres-1 psql)
else
    fail psql "no psql available (set E2E_PSQL)"
    finish
fi

run_sql() { # <sql> -> stdout, status preserved
    "${PSQL[@]}" "$RESOLVED_DSN" -v ON_ERROR_STOP=1 -Atq -c "$1" 2>&1
}

# --- 8. Server-side identity -------------------------------------------------
IDENTITY="$(run_sql 'select current_database()')"
if [ "$?" -ne 0 ]; then
    fail server-identity "query failed: $IDENTITY"
    finish
elif [ "$IDENTITY" = "$EXPECTED_DB" ]; then
    pass server-identity "current_database()=$IDENTITY"
else
    fail server-identity "current_database()='$IDENTITY', expected '$EXPECTED_DB'"
    finish
fi

# --- 9. Demo tenant must never live in the E2E database ---------------------
DEMO_COUNT="$(run_sql "select count(*) from tenants where slug = 'demo'")"
if [ "$?" -ne 0 ]; then
    fail demo-tenant "query failed: $DEMO_COUNT"
    finish
elif [ "$DEMO_COUNT" = "0" ]; then
    pass demo-tenant "no tenant with slug='demo'"
else
    fail demo-tenant "found $DEMO_COUNT tenant(s) with slug='demo', refusing"
    finish
fi

# --- 10. Grants, then the destructive delete --------------------------------
GRANT_OUT="$("${PSQL[@]}" "$RESOLVED_DSN" -v ON_ERROR_STOP=1 -q -f - < "$INIT_SQL" 2>&1)"
if [ "$?" -ne 0 ]; then
    fail init-db-grants "$GRANT_OUT"
    finish
fi
pass init-db-grants "applied scripts/init-db.sql"

DEL_OUT="$(run_sql 'delete from tenants')"
if [ "$?" -ne 0 ]; then
    fail delete-tenants "$DEL_OUT"
    finish
fi
pass delete-tenants "DELETE FROM tenants"

REMAINING="$(run_sql 'select count(*) from tenants')"
if [ "$REMAINING" = "0" ]; then
    pass tenants-empty "count=0"
else
    fail tenants-empty "count='$REMAINING', expected 0"
fi

finish

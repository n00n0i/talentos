#!/usr/bin/env bash
# Negative tests for scripts/e2e-reset.sh.
# Every case must be refused with a nonzero exit. Run from anywhere.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GUARD="$ROOT/scripts/e2e-reset.sh"
OUT="$(mktemp)"
FAILED=0
PASSED=0

cleanup() {
    # Remove the synthetic demo tenant if a case created it.
    if command -v docker >/dev/null 2>&1 && docker inspect loans-postgres-1 >/dev/null 2>&1; then
        docker exec -i loans-postgres-1 psql \
            "postgres://talentos:talentos_dev_secret@localhost:5432/talentos_e2e" \
            -v ON_ERROR_STOP=1 -Atq -c "delete from tenants where slug='demo'" >/dev/null 2>&1 || true
    fi
    rm -f "$OUT"
}
trap cleanup EXIT

# Run the guard with a clean environment so inherited DSNs cannot influence it.
run_guard() { # <extra env assignments...>
    env -i PATH="$PATH" HOME="$HOME" "$@" "$GUARD" >"$OUT" 2>&1
}

expect_refusal() { # <name> <expected-exit|any> <env assignments...>
    local name="$1" expected="$2"; shift 2
    local code
    run_guard "$@"
    code=$?
    if [ "$expected" = "any" ]; then
        if [ "$code" -ne 0 ]; then
            printf 'PASS  %-32s refused (exit %s)\n' "$name" "$code"
            PASSED=$((PASSED + 1))
            return
        fi
    elif [ "$code" -eq "$expected" ]; then
        printf 'PASS  %-32s refused (exit %s)\n' "$name" "$code"
        PASSED=$((PASSED + 1))
        return
    fi
    printf 'FAIL  %-32s expected refusal, got exit %s\n' "$name" "$code"
    sed 's/^/      /' "$OUT"
    FAILED=$((FAILED + 1))
}

# 1. Missing E2E_ALLOW_DESTRUCTIVE must exit 2.
expect_refusal "missing E2E_ALLOW_DESTRUCTIVE" 2 E2E_DB=talentos_e2e

# 2-3. E2E_DB must be exactly talentos_e2e.
expect_refusal "E2E_DB=talentos" any \
    E2E_ALLOW_DESTRUCTIVE=1 E2E_DB=talentos
expect_refusal "E2E_DB=prod_x" any \
    E2E_ALLOW_DESTRUCTIVE=1 E2E_DB=prod_x

# 4. DATABASE_URL pointing at the main database.
expect_refusal "DATABASE_URL -> talentos" any \
    E2E_ALLOW_DESTRUCTIVE=1 E2E_DB=talentos_e2e \
    DATABASE_URL="postgres://talentos:talentos_dev_secret@localhost:5432/talentos"

# 5. A demo tenant present in the E2E database.
docker exec -i loans-postgres-1 psql \
    "postgres://talentos:talentos_dev_secret@localhost:5432/talentos_e2e" \
    -v ON_ERROR_STOP=1 -Atq \
    -c "insert into tenants (name, slug) values ('Demo Guard Test', 'demo') on conflict (slug) do nothing" >/dev/null 2>&1
expect_refusal "demo tenant present" any \
    E2E_ALLOW_DESTRUCTIVE=1 E2E_DB=talentos_e2e \
    E2E_RESET_DATABASE_URL="postgres://talentos:talentos_dev_secret@localhost:5432/talentos_e2e" \
    DATABASE_AUTH_URL="postgres://talentos_auth:auth_dev_secret@localhost:5432/talentos_e2e"

echo
if [ "$FAILED" -eq 0 ]; then
    echo "RESULT: PASS ($PASSED/5 guard cases refused)"
    exit 0
fi
echo "RESULT: FAIL ($FAILED case(s) were not refused)"
exit 1

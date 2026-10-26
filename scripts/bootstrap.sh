#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

echo "==> Starting infrastructure..."
docker compose -f "$ROOT_DIR/docker-compose.yml" up -d postgres kafka temporal temporal-ui redis minio

echo "==> Waiting for PostgreSQL..."
until docker compose -f "$ROOT_DIR/docker-compose.yml" exec -T postgres pg_isready -U talentos -d talentos >/dev/null 2>&1; do
    sleep 1
done
echo "    PostgreSQL ready."

echo "==> Running SQLx migrations..."
export DATABASE_URL="postgres://talentos:${PG_PASSWORD:-talentos_dev_secret}@localhost:5432/talentos"
cd "$ROOT_DIR"
if command -v sqlx >/dev/null 2>&1; then
    sqlx migrate run
else
    echo "    WARNING: sqlx-cli not installed. Run: cargo install sqlx-cli --no-default-features --features postgres,rustls"
    echo "    Then re-run: sqlx migrate run"
fi

echo "==> Creating Kafka topics..."
TOPICS=("talentos.domain-events" "talentos.audit-log" "talentos.ai-agent-tasks")
for topic in "${TOPICS[@]}"; do
    docker compose -f "$ROOT_DIR/docker-compose.yml" exec -T kafka \
        /opt/kafka/bin/kafka-topics.sh --create --if-not-exists \
        --bootstrap-server localhost:9092 \
        --topic "$topic" \
        --partitions 6 \
        --replication-factor 1
done

echo "==> Setting up MinIO bucket..."
docker compose -f "$ROOT_DIR/docker-compose.yml" exec -T minio \
    mc mb --ignore-existing local/talentos-files 2>/dev/null || true

echo ""
echo "✅ Infrastructure ready:"
echo "   PostgreSQL:  localhost:5432"
echo "   Kafka:       localhost:9092"
echo "   Temporal:    localhost:7233 (UI: http://localhost:8080)"
echo "   Redis:       localhost:6379"
echo "   MinIO:       localhost:9000 (Console: http://localhost:9001)"
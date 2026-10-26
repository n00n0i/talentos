.DEFAULT_GOAL := help
SHELL := /bin/bash -o pipefail

# --- Configuration ---
COMPOSE_FILE := docker-compose.yml
CARGO_TARGET_DIR ?= target
SQLX_OFFLINE ?= false
# Owner role: migrations + sqlx prepare only.
export DATABASE_URL ?= postgres://talentos:$${PG_PASSWORD:-talentos_dev_secret}@localhost:5432/talentos
# Runtime role: NOBYPASSRLS, so RLS actually applies.
export DATABASE_APP_URL ?= postgres://talentos_app:app_dev_secret@localhost:5432/talentos
export AUTH_JWT_SECRET ?= dev-secret-change-me-in-prod-minimum-32-bytes-okay!!
export DATABASE_AUTH_URL ?= postgres://talentos_auth:auth_dev_secret@localhost:5432/talentos

.PHONY: help install infra-up infra-down db-reset sqlx-prepare \
        check fmt clippy test build release run \
        deploy deploy-rollback status logs clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-18s\033[0m %s\n", $$1, $$2}'

# ============================================================
# SETUP (run once per machine)
# ============================================================

install: ## Install Rust toolchain deps + sqlx-cli if missing
	rustup show active-toolchain >/dev/null 2>&1 || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --component rustfmt clippy
	cargo install sqlx-cli --no-default-features --features postgres,rustls --quiet 2>/dev/null || true
	cargo install cargo-nextest --locked --quiet 2>/dev/null || true
	@echo "✅ Dev tools ready."

infra-up: ## Start Docker Compose stack (postgres, kafka, temporal, redis, minio)
	docker compose -f $(COMPOSE_FILE) up -d postgres kafka temporal temporal-ui redis minio
	@echo "⏳ Waiting for PostgreSQL..."
	@until docker compose -f $(COMPOSE_FILE) exec -T postgres pg_isready -U talentos -d talentos >/dev/null 2>&1; do sleep 1; done
	@echo "✅ Infrastructure up."

db-reset: ## Drop and recreate database from scratch (DESTRUCTIVE)
	docker compose -f $(COMPOSE_FILE) down -v
	docker compose -f $(COMPOSE_FILE) up -d postgres
	@until docker compose -f $(COMPOSE_FILE) exec -T postgres pg_isready -U talentos -d talentos >/dev/null 2>&1; do sleep 1; done
	sqlx migrate run
	@echo "✅ Database reset and migrated."

sqlx-prepare: ## Generate .sqlx/ cache so builds work without live DB (CI/offline mode)
	$(MAKE) infra-up
	DATABASE_URL=$(DATABASE_URL) cargo sqlx prepare --workspace
	@echo "✅ SQLX offline cache generated in .sqlx/"

# ============================================================
# QUALITY GATES (used by CI and pre-commit locally)
# ============================================================

check: ## cargo check all crates
	SQLX_OFFLINE=$(SQLX_OFFLINE) cargo check --workspace --all-targets

fmt: ## Auto-format code
	cargo fmt --all

fmt-check: ## Verify formatting (fails if dirty)
	cargo fmt --all -- --check

clippy: ## Lint with warnings-as-errors
	SQLX_OFFLINE=$(SQLX_OFFLINE) cargo clippy --workspace --all-targets -- -D warnings

test: ## Run unit tests via nextest (as the RLS-enforcing app role)
	DATABASE_URL=$(DATABASE_APP_URL) SQLX_OFFLINE=$(SQLX_OFFLINE) cargo nextest run --workspace

lint: fmt-check clippy ## Combined quality gate

ci: lint test ## Full CI gate (what the pipeline runs)

# ============================================================
# BUILD & RUN
# ============================================================

build: ## Debug build of gateway binary
	SQLX_OFFLINE=$(SQLX_OFFLINE) cargo build -p talentos-gateway

release: ## Optimized release build
	SQLX_OFFLINE=$(SQLX_OFFLINE) cargo build --release -p talentos-gateway

run: build ## Run gateway locally against running infra (app role, RLS on)
	DATABASE_URL=$(DATABASE_APP_URL) RUST_LOG=$${RUST_LOG:-info,talentos=debug} ./target/debug/talentos-gateway

# ============================================================
# DEPLOYMENT (on-prem production)
# ============================================================

deploy: release ## Build image, migrate, swap containers with health check
	./scripts/deploy.sh

deploy-rollback: ## Roll back to previous tagged version
	./scripts/deploy.sh --rollback

status: ## Show container health summary
	docker compose -f $(COMPOSE_FILE) ps
	@curl -sf http://localhost:8080/health && echo "" || echo "❌ Gateway unhealthy"

logs: ## Tail gateway + postgres logs
	docker compose -f $(COMPOSE_FILE) logs -f --tail=100 api-gw postgres

clean: ## Remove build artifacts
	cargo clean
	rm -rf .sqlx/
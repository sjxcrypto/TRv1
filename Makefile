# ──────────────────────────────────────────────────────────────────────────────
# TRv1 Makefile
#
# Build targets for the TRv1 blockchain validator and tooling.
#
# Usage:
#   make build            Build all binaries (release)
#   make test             Run full workspace tests
#   make test-fast        Run only TRv1-specific crate tests
#   make validator        Build just the validator binary
#   make genesis          Generate test genesis ledger
#   make testnet          Start a local 3-node testnet
#   make docker-build     Build the Docker image
#   make docker-testnet   Start a 3-node testnet via Docker Compose
#   make clean            Cargo clean
#   make lint             Run clippy
#   make fmt              Run rustfmt
#   make help             Show this help
# ──────────────────────────────────────────────────────────────────────────────

.DEFAULT_GOAL := help
SHELL := /bin/bash

# ── Rust Environment ──────────────────────────────────────────────────────────
# Source cargo env if available (needed in some CI/container environments)
CARGO_ENV := source "$$HOME/.cargo/env" 2>/dev/null;

# ── Directories ───────────────────────────────────────────────────────────────
ROOT_DIR    := $(shell pwd)
BIN_DIR     := $(ROOT_DIR)/target/release
SCRIPTS_DIR := $(ROOT_DIR)/scripts/local-testnet
DOCKER_DIR  := $(ROOT_DIR)/docker
DOCS_DIR    := $(ROOT_DIR)/docs

# ── Docker ────────────────────────────────────────────────────────────────────
DOCKER_IMAGE := trv1-validator
DOCKER_TAG   := latest

# ── TRv1-specific crates ─────────────────────────────────────────────────────
TRV1_CRATES := \
	-p trv1-fee-market \
	-p trv1-test-validator \
	-p trv1-genesis \
	-p trv1-validator \
	-p solana-runtime

TRV1_PROGRAM_CRATES := \
	-p solana-passive-stake-program \
	-p solana-treasury-program \
	-p solana-governance-program \
	-p solana-developer-rewards-program

# ══════════════════════════════════════════════════════════════════════════════
# Build Targets
# ══════════════════════════════════════════════════════════════════════════════

.PHONY: build
build: ## Build all binaries in release mode
	@echo "═══ Building TRv1 (release) ═══"
	@$(CARGO_ENV) cargo build --release

.PHONY: build-debug
build-debug: ## Build all binaries in debug mode (faster compilation)
	@echo "═══ Building TRv1 (debug) ═══"
	@$(CARGO_ENV) cargo build

.PHONY: validator
validator: ## Build just the validator binary
	@echo "═══ Building trv1-validator ═══"
	@$(CARGO_ENV) cargo build --release --bin trv1-validator --bin solana-test-validator

.PHONY: genesis-bin
genesis-bin: ## Build just the genesis binary
	@echo "═══ Building trv1-genesis ═══"
	@$(CARGO_ENV) cargo build --release --bin trv1-genesis

.PHONY: programs
programs: ## Build TRv1 program crates
	@echo "═══ Building TRv1 programs ═══"
	@$(CARGO_ENV) cargo build --release $(TRV1_PROGRAM_CRATES) --features agave-unstable-api

# ══════════════════════════════════════════════════════════════════════════════
# Test Targets
# ══════════════════════════════════════════════════════════════════════════════

.PHONY: test
test: ## Run full workspace tests
	@echo "═══ Running all workspace tests ═══"
	@$(CARGO_ENV) cargo test --workspace

.PHONY: test-fast
test-fast: ## Run only TRv1-specific crate tests (fast)
	@echo "═══ Running TRv1-specific tests ═══"
	@$(CARGO_ENV) cargo test $(TRV1_CRATES)

.PHONY: test-programs
test-programs: ## Run TRv1 program tests
	@echo "═══ Running TRv1 program tests ═══"
	@$(CARGO_ENV) cargo test $(TRV1_PROGRAM_CRATES) --features agave-unstable-api

.PHONY: test-fee-market
test-fee-market: ## Run fee market crate tests
	@echo "═══ Running fee market tests ═══"
	@$(CARGO_ENV) cargo test -p trv1-fee-market

.PHONY: test-genesis
test-genesis: ## Run genesis configuration tests
	@echo "═══ Running genesis config tests ═══"
	@$(CARGO_ENV) cargo test -p trv1-test-validator trv1_genesis

# ══════════════════════════════════════════════════════════════════════════════
# Genesis & Testnet Targets
# ══════════════════════════════════════════════════════════════════════════════

.PHONY: genesis
genesis: validator ## Generate a test genesis ledger
	@echo "═══ Generating test genesis ═══"
	@$(BIN_DIR)/solana-test-validator \
		--ledger $(ROOT_DIR)/test-ledger \
		--slots-per-epoch 86400 \
		--inflation-fixed 0.05 \
		--reset \
		--quiet &
	@sleep 5
	@echo "Genesis created at $(ROOT_DIR)/test-ledger"
	@kill %1 2>/dev/null || true

.PHONY: testnet
testnet: validator ## Start a local 3-node testnet
	@echo "═══ Starting local testnet ═══"
	@bash $(SCRIPTS_DIR)/start-testnet.sh

.PHONY: testnet-reset
testnet-reset: validator ## Start a local testnet (reset ledger first)
	@echo "═══ Starting local testnet (reset) ═══"
	@bash $(SCRIPTS_DIR)/start-testnet.sh --reset

.PHONY: testnet-stop
testnet-stop: ## Stop the local testnet
	@echo "═══ Stopping local testnet ═══"
	@bash $(SCRIPTS_DIR)/stop-testnet.sh

.PHONY: fund
fund: ## Fund test accounts on the local testnet
	@echo "═══ Funding test accounts ═══"
	@bash $(SCRIPTS_DIR)/fund-test-accounts.sh

# ══════════════════════════════════════════════════════════════════════════════
# Docker Targets
# ══════════════════════════════════════════════════════════════════════════════

.PHONY: docker-build
docker-build: ## Build the Docker image
	@echo "═══ Building Docker image $(DOCKER_IMAGE):$(DOCKER_TAG) ═══"
	docker build -t $(DOCKER_IMAGE):$(DOCKER_TAG) -f $(DOCKER_DIR)/Dockerfile .

.PHONY: docker-testnet
docker-testnet: ## Start a 3-node testnet via Docker Compose
	@echo "═══ Starting Docker testnet ═══"
	cd $(DOCKER_DIR) && docker compose up --build -d
	@echo ""
	@echo "Testnet running:"
	@echo "  Validator 1: http://localhost:8899"
	@echo "  Validator 2: http://localhost:8999"
	@echo "  Validator 3: http://localhost:9099"
	@echo "  Faucet:      http://localhost:9900"
	@echo ""
	@echo "Stop with: make docker-testnet-stop"

.PHONY: docker-testnet-stop
docker-testnet-stop: ## Stop the Docker testnet
	@echo "═══ Stopping Docker testnet ═══"
	cd $(DOCKER_DIR) && docker compose down

.PHONY: docker-testnet-clean
docker-testnet-clean: ## Stop Docker testnet and remove volumes
	@echo "═══ Cleaning Docker testnet ═══"
	cd $(DOCKER_DIR) && docker compose down -v

.PHONY: docker-logs
docker-logs: ## Follow Docker testnet logs
	cd $(DOCKER_DIR) && docker compose logs -f

# ══════════════════════════════════════════════════════════════════════════════
# Code Quality Targets
# ══════════════════════════════════════════════════════════════════════════════

.PHONY: lint
lint: ## Run clippy on the workspace
	@echo "═══ Running clippy ═══"
	@$(CARGO_ENV) cargo clippy --workspace --all-targets -- -D warnings

.PHONY: lint-fix
lint-fix: ## Run clippy with auto-fix
	@echo "═══ Running clippy (fix) ═══"
	@$(CARGO_ENV) cargo clippy --workspace --all-targets --fix --allow-dirty

.PHONY: fmt
fmt: ## Format all Rust code
	@echo "═══ Running rustfmt ═══"
	@$(CARGO_ENV) cargo fmt --all

.PHONY: fmt-check
fmt-check: ## Check formatting without modifying files
	@echo "═══ Checking formatting ═══"
	@$(CARGO_ENV) cargo fmt --all -- --check

.PHONY: check
check: ## Run cargo check (fast compilation check)
	@echo "═══ Running cargo check ═══"
	@$(CARGO_ENV) cargo check --workspace

# ══════════════════════════════════════════════════════════════════════════════
# Cleanup Targets
# ══════════════════════════════════════════════════════════════════════════════

.PHONY: clean
clean: ## Cargo clean (remove target directory)
	@echo "═══ Cleaning build artifacts ═══"
	@$(CARGO_ENV) cargo clean

.PHONY: clean-testnet
clean-testnet: ## Remove testnet ledger data
	@echo "═══ Cleaning testnet data ═══"
	rm -rf $(ROOT_DIR)/test-ledger

.PHONY: clean-all
clean-all: clean clean-testnet docker-testnet-clean ## Clean everything
	@echo "═══ All clean ═══"

# ══════════════════════════════════════════════════════════════════════════════
# Documentation Targets
# ══════════════════════════════════════════════════════════════════════════════

.PHONY: docs
docs: ## Generate Rust documentation
	@echo "═══ Generating docs ═══"
	@$(CARGO_ENV) cargo doc --workspace --no-deps --open

.PHONY: docs-build
docs-build: ## Generate docs without opening browser
	@echo "═══ Generating docs ═══"
	@$(CARGO_ENV) cargo doc --workspace --no-deps

# ══════════════════════════════════════════════════════════════════════════════
# CI/CD Targets
# ══════════════════════════════════════════════════════════════════════════════

.PHONY: ci
ci: fmt-check lint test-fast ## Run CI checks (fmt + lint + tests)
	@echo "═══ CI checks passed ═══"

.PHONY: ci-full
ci-full: fmt-check lint test ## Run full CI checks (all workspace tests)
	@echo "═══ Full CI checks passed ═══"

# ══════════════════════════════════════════════════════════════════════════════
# Help
# ══════════════════════════════════════════════════════════════════════════════

.PHONY: help
help: ## Show this help message
	@echo ""
	@echo "TRv1 Blockchain — Build Targets"
	@echo "════════════════════════════════"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-22s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "Examples:"
	@echo "  make build              # Build everything"
	@echo "  make test-fast          # Quick TRv1 tests"
	@echo "  make testnet            # Start local 3-node testnet"
	@echo "  make docker-testnet     # Start Docker testnet"
	@echo ""

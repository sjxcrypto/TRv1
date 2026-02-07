# ──────────────────────────────────────────────────────────────────────────────
# TRv1 Validator — Multi-stage Docker Build
#
# Stage 1 (builder):   Compiles the validator binary from source.
# Stage 2 (runtime):   Minimal Ubuntu image with the validator binary.
#
# Build:
#   docker build -t trv1-validator:latest -f docker/Dockerfile .
#
# Run (single-node test validator):
#   docker run --rm -p 8899:8899 -p 8900:8900 trv1-validator:latest
#
# Run (custom args):
#   docker run --rm -p 8899:8899 trv1-validator:latest \
#     --rpc-port 8899 --slots-per-epoch 86400
# ──────────────────────────────────────────────────────────────────────────────

# ════════════════════════════════════════════════════════════════════════════════
# Stage 1: Builder
# ════════════════════════════════════════════════════════════════════════════════
FROM rust:1.87.0-bookworm AS builder

# Install system dependencies required by Solana/Agave
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    clang \
    cmake \
    libclang-dev \
    libudev-dev \
    libssl-dev \
    pkg-config \
    protobuf-compiler \
    zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy manifests first for better layer caching
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY .cargo/ .cargo/

# Copy the full source tree
COPY . .

# Build the validator and test-validator in release mode
RUN --mount=type=cache,target=/build/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release \
        --bin trv1-validator \
        --bin solana-test-validator \
        --bin trv1-genesis \
        --bin solana-keygen \
        --bin solana-faucet \
    && cp target/release/trv1-validator /usr/local/bin/ \
    && cp target/release/solana-test-validator /usr/local/bin/ \
    && cp target/release/trv1-genesis /usr/local/bin/ \
    && cp target/release/solana-keygen /usr/local/bin/ \
    && cp target/release/solana-faucet /usr/local/bin/

# ════════════════════════════════════════════════════════════════════════════════
# Stage 2: Runtime
# ════════════════════════════════════════════════════════════════════════════════
FROM ubuntu:22.04 AS runtime

LABEL maintainer="TRv1 Team"
LABEL description="TRv1 Validator Node"
LABEL version="0.1.0"

# Install minimal runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libssl3 \
    libudev1 \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user for running the validator
RUN useradd --create-home --shell /bin/bash trv1

# Copy binaries from the builder stage
COPY --from=builder /usr/local/bin/trv1-validator /usr/local/bin/
COPY --from=builder /usr/local/bin/solana-test-validator /usr/local/bin/
COPY --from=builder /usr/local/bin/trv1-genesis /usr/local/bin/
COPY --from=builder /usr/local/bin/solana-keygen /usr/local/bin/
COPY --from=builder /usr/local/bin/solana-faucet /usr/local/bin/

# Copy entrypoint script
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# Create data directories
RUN mkdir -p /data/ledger /data/accounts /data/snapshots /data/keys \
    && chown -R trv1:trv1 /data

# Switch to non-root user
USER trv1
WORKDIR /home/trv1

# ── Volumes ───────────────────────────────────────────────────────────────────
VOLUME ["/data/ledger", "/data/accounts", "/data/snapshots", "/data/keys"]

# ── Ports ─────────────────────────────────────────────────────────────────────
#   8899  — JSON-RPC
#   8900  — WebSocket (PubSub)
#   8001  — Gossip
#   8003  — TPU
#   8004  — TPU forwards
#   9900  — Faucet
EXPOSE 8899 8900 8001 8003 8004 9900

# ── Environment ───────────────────────────────────────────────────────────────
ENV TRV1_LEDGER_DIR=/data/ledger
ENV TRV1_ACCOUNTS_DIR=/data/accounts
ENV TRV1_SNAPSHOTS_DIR=/data/snapshots
ENV TRV1_KEYS_DIR=/data/keys
ENV TRV1_RPC_PORT=8899
ENV TRV1_GOSSIP_PORT=8001
ENV TRV1_FAUCET_PORT=9900
ENV TRV1_SLOTS_PER_EPOCH=86400
ENV TRV1_INFLATION=0.05
ENV RUST_LOG=info
ENV RUST_BACKTRACE=1

# ── Health check ──────────────────────────────────────────────────────────────
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD curl -sf http://localhost:${TRV1_RPC_PORT}/health || exit 1

ENTRYPOINT ["entrypoint.sh"]
CMD []

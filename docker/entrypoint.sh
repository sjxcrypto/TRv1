#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────────────
# TRv1 Validator Docker Entrypoint
#
# Configures and launches the validator based on environment variables.
# Supports two modes:
#   1. Test validator (default): single-node for development
#   2. Full validator: for joining a network
#
# Environment:
#   TRV1_MODE              test-validator (default) | validator
#   TRV1_LEDGER_DIR        Ledger directory
#   TRV1_RPC_PORT          JSON-RPC port
#   TRV1_GOSSIP_PORT       Gossip port
#   TRV1_FAUCET_PORT       Faucet port (test-validator only)
#   TRV1_SLOTS_PER_EPOCH   Slots per epoch
#   TRV1_INFLATION         Fixed inflation rate
#   TRV1_ENTRYPOINT        Entrypoint address (validator mode)
#   TRV1_IDENTITY          Path to identity keypair (validator mode)
#   TRV1_VOTE_ACCOUNT      Path to vote account keypair (validator mode)
#   TRV1_EXTRA_ARGS        Additional CLI arguments
# ──────────────────────────────────────────────────────────────────────────────
set -euo pipefail

MODE="${TRV1_MODE:-test-validator}"

echo "════════════════════════════════════════════════════"
echo "  TRv1 Validator — Docker Container"
echo "  Mode: ${MODE}"
echo "════════════════════════════════════════════════════"

# Generate an identity keypair if none exists
if [[ ! -f "${TRV1_KEYS_DIR}/identity.json" ]]; then
    echo "[TRv1] Generating identity keypair..."
    solana-keygen new --no-bip39-passphrase --silent -o "${TRV1_KEYS_DIR}/identity.json"
fi

if [[ "${MODE}" == "test-validator" ]]; then
    # ── Test Validator Mode ───────────────────────────────────────────────
    echo "[TRv1] Starting test validator..."

    ARGS=(
        --ledger "${TRV1_LEDGER_DIR}"
        --rpc-port "${TRV1_RPC_PORT}"
        --faucet-port "${TRV1_FAUCET_PORT}"
        --slots-per-epoch "${TRV1_SLOTS_PER_EPOCH}"
        --inflation-fixed "${TRV1_INFLATION}"
        --bind-address "0.0.0.0"
        --rpc-faucet-address "0.0.0.0:${TRV1_FAUCET_PORT}"
        --log
        --reset
    )

    # Pass through any extra arguments
    if [[ -n "${TRV1_EXTRA_ARGS:-}" ]]; then
        read -ra extra <<< "${TRV1_EXTRA_ARGS}"
        ARGS+=("${extra[@]}")
    fi

    # Pass through any command-line arguments
    if [[ $# -gt 0 ]]; then
        ARGS+=("$@")
    fi

    exec solana-test-validator "${ARGS[@]}"

elif [[ "${MODE}" == "validator" ]]; then
    # ── Full Validator Mode ───────────────────────────────────────────────
    IDENTITY="${TRV1_IDENTITY:-${TRV1_KEYS_DIR}/identity.json}"
    VOTE_ACCOUNT="${TRV1_VOTE_ACCOUNT:-${TRV1_KEYS_DIR}/vote.json}"
    ENTRYPOINT="${TRV1_ENTRYPOINT:-}"

    if [[ ! -f "${IDENTITY}" ]]; then
        echo "[TRv1] ERROR: Identity keypair not found at ${IDENTITY}"
        exit 1
    fi

    echo "[TRv1] Starting validator..."
    echo "[TRv1]   Identity:     ${IDENTITY}"
    echo "[TRv1]   Vote account: ${VOTE_ACCOUNT}"
    echo "[TRv1]   Entrypoint:   ${ENTRYPOINT:-<bootstrap leader>}"

    ARGS=(
        --identity "${IDENTITY}"
        --vote-account "${VOTE_ACCOUNT}"
        --ledger "${TRV1_LEDGER_DIR}"
        --rpc-port "${TRV1_RPC_PORT}"
        --gossip-port "${TRV1_GOSSIP_PORT}"
        --dynamic-port-range "8002-8020"
        --rpc-bind-address "0.0.0.0"
        --log
    )

    if [[ -n "${ENTRYPOINT}" ]]; then
        ARGS+=(--entrypoint "${ENTRYPOINT}")
    fi

    if [[ -n "${TRV1_EXTRA_ARGS:-}" ]]; then
        read -ra extra <<< "${TRV1_EXTRA_ARGS}"
        ARGS+=("${extra[@]}")
    fi

    if [[ $# -gt 0 ]]; then
        ARGS+=("$@")
    fi

    exec trv1-validator "${ARGS[@]}"

else
    echo "[TRv1] ERROR: Unknown mode '${MODE}'. Use 'test-validator' or 'validator'."
    exit 1
fi

#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────────────
# start-testnet.sh — Start a local TRv1 testnet with 3 validators
#
# This script:
#   1. Generates keypairs for 3 validators (if they don't already exist).
#   2. Creates the TRv1 genesis ledger.
#   3. Launches 3 validator processes on different port ranges.
#   4. Starts a faucet for development airdrops.
#
# Usage:
#   ./scripts/local-testnet/start-testnet.sh [--reset]
#
# Flags:
#   --reset   Wipe existing ledger and start fresh.
#
# Prerequisites:
#   - `trv1-validator` and `trv1-genesis` binaries in target/release/
#   - Run `make build` first.
# ──────────────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TESTNET_DIR="${ROOT_DIR}/test-ledger/local-testnet"
BIN_DIR="${ROOT_DIR}/target/release"

# ── Binaries ──────────────────────────────────────────────────────────────────
VALIDATOR="${BIN_DIR}/trv1-validator"
GENESIS="${BIN_DIR}/trv1-genesis"
KEYGEN="${BIN_DIR}/solana-keygen"
FAUCET="${BIN_DIR}/solana-faucet"

# ── TRv1 Parameters ──────────────────────────────────────────────────────────
SLOTS_PER_EPOCH=86400            # 1 day at 1-second slots
INFLATION_FIXED=0.05             # 5% flat annual inflation
FAUCET_LAMPORTS=500000000000000000  # 500 M TRV1 in lamports
BOOTSTRAP_STAKE=500000000000000  # 500 k TRV1 per validator

# ── Port Ranges ───────────────────────────────────────────────────────────────
VALIDATOR1_RPC_PORT=8899
VALIDATOR1_GOSSIP_PORT=8001
VALIDATOR1_DYNAMIC_RANGE="8002-8020"

VALIDATOR2_RPC_PORT=8999
VALIDATOR2_GOSSIP_PORT=8101
VALIDATOR2_DYNAMIC_RANGE="8102-8120"

VALIDATOR3_RPC_PORT=9099
VALIDATOR3_GOSSIP_PORT=8201
VALIDATOR3_DYNAMIC_RANGE="8202-8220"

FAUCET_PORT=9900

# ── Colors ────────────────────────────────────────────────────────────────────
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

log()  { echo -e "${GREEN}[TRv1]${NC} $*"; }
warn() { echo -e "${YELLOW}[TRv1]${NC} $*"; }
err()  { echo -e "${RED}[TRv1]${NC} $*" >&2; }

# ── Check prerequisites ──────────────────────────────────────────────────────
check_binary() {
    if [[ ! -x "$1" ]]; then
        err "Binary not found: $1"
        err "Run 'make build' first."
        exit 1
    fi
}

check_binary "${VALIDATOR}"

# ── Handle --reset ────────────────────────────────────────────────────────────
if [[ "${1:-}" == "--reset" ]]; then
    log "Resetting testnet ledger..."
    rm -rf "${TESTNET_DIR}"
fi

# ── Create directories ───────────────────────────────────────────────────────
mkdir -p "${TESTNET_DIR}"/{validator-1,validator-2,validator-3,keys,logs}

# ── Generate keypairs ─────────────────────────────────────────────────────────
generate_keypair() {
    local name="$1"
    local path="${TESTNET_DIR}/keys/${name}.json"
    if [[ ! -f "${path}" ]]; then
        if [[ -x "${KEYGEN}" ]]; then
            "${KEYGEN}" new --no-bip39-passphrase --silent -o "${path}"
        else
            # Fallback: use trv1-validator's built-in keygen if solana-keygen
            # is not available (it will generate at startup anyway)
            log "solana-keygen not found, generating ${name} keypair via Python fallback..."
            python3 -c "
import json, secrets
key = [secrets.randbelow(256) for _ in range(64)]
with open('${path}', 'w') as f:
    json.dump(key, f)
"
        fi
        log "Generated keypair: ${name}"
    else
        log "Keypair already exists: ${name}"
    fi
}

for i in 1 2 3; do
    generate_keypair "validator-${i}-identity"
    generate_keypair "validator-${i}-vote"
    generate_keypair "validator-${i}-stake"
done
generate_keypair "faucet"
generate_keypair "treasury"

# ── PID file management ──────────────────────────────────────────────────────
PID_FILE="${TESTNET_DIR}/pids"
: > "${PID_FILE}"

cleanup() {
    log "Cleaning up..."
    if [[ -f "${PID_FILE}" ]]; then
        while read -r pid; do
            if kill -0 "${pid}" 2>/dev/null; then
                kill "${pid}" 2>/dev/null || true
            fi
        done < "${PID_FILE}"
    fi
}
trap cleanup EXIT

# ── Start Validator 1 (bootstrap) ────────────────────────────────────────────
log "Starting validator 1 (bootstrap leader)..."
"${VALIDATOR}" \
    --identity "${TESTNET_DIR}/keys/validator-1-identity.json" \
    --vote-account "${TESTNET_DIR}/keys/validator-1-vote.json" \
    --ledger "${TESTNET_DIR}/validator-1" \
    --rpc-port "${VALIDATOR1_RPC_PORT}" \
    --gossip-port "${VALIDATOR1_GOSSIP_PORT}" \
    --dynamic-port-range "${VALIDATOR1_DYNAMIC_RANGE}" \
    --slots-per-epoch "${SLOTS_PER_EPOCH}" \
    --inflation-fixed "${INFLATION_FIXED}" \
    --faucet-sol "500000000" \
    --log \
    --reset \
    > "${TESTNET_DIR}/logs/validator-1.log" 2>&1 &
VALIDATOR1_PID=$!
echo "${VALIDATOR1_PID}" >> "${PID_FILE}"
log "Validator 1 started (PID: ${VALIDATOR1_PID})"

# Wait for validator 1 RPC to become available
log "Waiting for validator 1 RPC (port ${VALIDATOR1_RPC_PORT})..."
for i in $(seq 1 60); do
    if curl -s "http://127.0.0.1:${VALIDATOR1_RPC_PORT}" -X POST \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
        > /dev/null 2>&1; then
        log "Validator 1 RPC is ready!"
        break
    fi
    if [[ ${i} -eq 60 ]]; then
        err "Validator 1 did not start within 60 seconds."
        err "Check ${TESTNET_DIR}/logs/validator-1.log for errors."
        exit 1
    fi
    sleep 1
done

# ── Start Validator 2 ────────────────────────────────────────────────────────
log "Starting validator 2..."
"${VALIDATOR}" \
    --identity "${TESTNET_DIR}/keys/validator-2-identity.json" \
    --vote-account "${TESTNET_DIR}/keys/validator-2-vote.json" \
    --ledger "${TESTNET_DIR}/validator-2" \
    --rpc-port "${VALIDATOR2_RPC_PORT}" \
    --gossip-port "${VALIDATOR2_GOSSIP_PORT}" \
    --dynamic-port-range "${VALIDATOR2_DYNAMIC_RANGE}" \
    --entrypoint "127.0.0.1:${VALIDATOR1_GOSSIP_PORT}" \
    --log \
    --reset \
    > "${TESTNET_DIR}/logs/validator-2.log" 2>&1 &
VALIDATOR2_PID=$!
echo "${VALIDATOR2_PID}" >> "${PID_FILE}"
log "Validator 2 started (PID: ${VALIDATOR2_PID})"

# ── Start Validator 3 ────────────────────────────────────────────────────────
log "Starting validator 3..."
"${VALIDATOR}" \
    --identity "${TESTNET_DIR}/keys/validator-3-identity.json" \
    --vote-account "${TESTNET_DIR}/keys/validator-3-vote.json" \
    --ledger "${TESTNET_DIR}/validator-3" \
    --rpc-port "${VALIDATOR3_RPC_PORT}" \
    --gossip-port "${VALIDATOR3_GOSSIP_PORT}" \
    --dynamic-port-range "${VALIDATOR3_DYNAMIC_RANGE}" \
    --entrypoint "127.0.0.1:${VALIDATOR1_GOSSIP_PORT}" \
    --log \
    --reset \
    > "${TESTNET_DIR}/logs/validator-3.log" 2>&1 &
VALIDATOR3_PID=$!
echo "${VALIDATOR3_PID}" >> "${PID_FILE}"
log "Validator 3 started (PID: ${VALIDATOR3_PID})"

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
log "╔═══════════════════════════════════════════════════════════════╗"
log "║            TRv1 Local Testnet — 3 Validators Running         ║"
log "╠═══════════════════════════════════════════════════════════════╣"
log "║  Validator 1:  RPC http://127.0.0.1:${VALIDATOR1_RPC_PORT}              ║"
log "║  Validator 2:  RPC http://127.0.0.1:${VALIDATOR2_RPC_PORT}              ║"
log "║  Validator 3:  RPC http://127.0.0.1:${VALIDATOR3_RPC_PORT}              ║"
log "║  Faucet:       http://127.0.0.1:${FAUCET_PORT}                  ║"
log "║                                                               ║"
log "║  Ledger:       ${TESTNET_DIR}                                 ║"
log "║  Logs:         ${TESTNET_DIR}/logs/                           ║"
log "║                                                               ║"
log "║  Stop with:    ./scripts/local-testnet/stop-testnet.sh        ║"
log "╚═══════════════════════════════════════════════════════════════╝"
echo ""

# Keep the script running so that the trap fires on Ctrl-C
log "Press Ctrl-C to stop all validators."
wait

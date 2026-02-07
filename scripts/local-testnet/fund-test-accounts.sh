#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────────────
# fund-test-accounts.sh — Airdrop TRV1 tokens to test accounts
#
# Creates several test keypairs and funds them via the local faucet.
# Useful for development and integration testing.
#
# Usage:
#   ./scripts/local-testnet/fund-test-accounts.sh [RPC_URL]
#
# Arguments:
#   RPC_URL   Optional. Defaults to http://127.0.0.1:8899
#
# Environment:
#   TRV1_AIRDROP_AMOUNT   Amount per account in TRV1 (default: 1000)
#   TRV1_NUM_ACCOUNTS      Number of test accounts to create (default: 5)
# ──────────────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BIN_DIR="${ROOT_DIR}/target/release"
TESTNET_DIR="${ROOT_DIR}/test-ledger/local-testnet"
ACCOUNTS_DIR="${TESTNET_DIR}/test-accounts"

RPC_URL="${1:-http://127.0.0.1:8899}"
AIRDROP_AMOUNT="${TRV1_AIRDROP_AMOUNT:-1000}"
NUM_ACCOUNTS="${TRV1_NUM_ACCOUNTS:-5}"

# Prefer the TRv1 CLI if available, fall back to solana CLI
CLI=""
for candidate in "${BIN_DIR}/trv1" "${BIN_DIR}/solana" "$(command -v solana 2>/dev/null || true)"; do
    if [[ -n "${candidate}" && -x "${candidate}" ]]; then
        CLI="${candidate}"
        break
    fi
done

KEYGEN=""
for candidate in "${BIN_DIR}/solana-keygen" "$(command -v solana-keygen 2>/dev/null || true)"; do
    if [[ -n "${candidate}" && -x "${candidate}" ]]; then
        KEYGEN="${candidate}"
        break
    fi
done

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

log()  { echo -e "${GREEN}[TRv1]${NC} $*"; }
warn() { echo -e "${YELLOW}[TRv1]${NC} $*"; }
err()  { echo -e "${RED}[TRv1]${NC} $*" >&2; }
info() { echo -e "${CYAN}[TRv1]${NC} $*"; }

# ── Verify RPC is reachable ──────────────────────────────────────────────────
log "Checking RPC at ${RPC_URL}..."
if ! curl -s "${RPC_URL}" -X POST \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
    > /dev/null 2>&1; then
    err "RPC at ${RPC_URL} is not reachable."
    err "Is the testnet running? Try: ./scripts/local-testnet/start-testnet.sh"
    exit 1
fi
log "RPC is healthy."

# ── Create test accounts directory ───────────────────────────────────────────
mkdir -p "${ACCOUNTS_DIR}"

# ── Generate and fund accounts ───────────────────────────────────────────────
log "Creating and funding ${NUM_ACCOUNTS} test accounts (${AIRDROP_AMOUNT} TRV1 each)..."
echo ""

funded=0
for i in $(seq 1 "${NUM_ACCOUNTS}"); do
    account_name="test-account-${i}"
    keypair_path="${ACCOUNTS_DIR}/${account_name}.json"

    # Generate keypair if it doesn't exist
    if [[ ! -f "${keypair_path}" ]]; then
        if [[ -n "${KEYGEN}" ]]; then
            "${KEYGEN}" new --no-bip39-passphrase --silent -o "${keypair_path}"
        else
            # Fallback: generate via Python
            python3 -c "
import json, secrets
key = [secrets.randbelow(256) for _ in range(64)]
with open('${keypair_path}', 'w') as f:
    json.dump(key, f)
"
        fi
    fi

    # Extract pubkey
    if [[ -n "${KEYGEN}" ]]; then
        pubkey=$("${KEYGEN}" pubkey "${keypair_path}" 2>/dev/null)
    else
        pubkey="(keypair at ${keypair_path})"
    fi

    # Airdrop via CLI if available
    if [[ -n "${CLI}" ]]; then
        if "${CLI}" airdrop "${AIRDROP_AMOUNT}" "${pubkey}" \
            --url "${RPC_URL}" \
            > /dev/null 2>&1; then
            info "  ✓ ${account_name}: ${pubkey} — ${AIRDROP_AMOUNT} TRV1"
            ((funded++)) || true
        else
            warn "  ✗ ${account_name}: ${pubkey} — airdrop failed (faucet may be unavailable)"
        fi
    else
        # Fallback: use raw RPC
        if curl -s "${RPC_URL}" -X POST \
            -H "Content-Type: application/json" \
            -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"requestAirdrop\",\"params\":[\"${pubkey}\",$(echo "${AIRDROP_AMOUNT} * 1000000000" | bc)]}" \
            > /dev/null 2>&1; then
            info "  ✓ ${account_name}: ${pubkey} — ${AIRDROP_AMOUNT} TRV1 (via RPC)"
            ((funded++)) || true
        else
            warn "  ✗ ${account_name}: ${pubkey} — airdrop failed"
        fi
    fi
done

echo ""

# ── Named special accounts ───────────────────────────────────────────────────
log "Creating named test accounts..."

declare -A SPECIAL_ACCOUNTS=(
    ["deployer"]="10000"
    ["alice"]="5000"
    ["bob"]="5000"
    ["treasury-admin"]="1000"
    ["governance-proposer"]="1000"
)

for name in "${!SPECIAL_ACCOUNTS[@]}"; do
    amount="${SPECIAL_ACCOUNTS[${name}]}"
    keypair_path="${ACCOUNTS_DIR}/${name}.json"

    if [[ ! -f "${keypair_path}" ]]; then
        if [[ -n "${KEYGEN}" ]]; then
            "${KEYGEN}" new --no-bip39-passphrase --silent -o "${keypair_path}"
        else
            python3 -c "
import json, secrets
key = [secrets.randbelow(256) for _ in range(64)]
with open('${keypair_path}', 'w') as f:
    json.dump(key, f)
"
        fi
    fi

    if [[ -n "${KEYGEN}" ]]; then
        pubkey=$("${KEYGEN}" pubkey "${keypair_path}" 2>/dev/null)
    else
        pubkey="(keypair at ${keypair_path})"
    fi

    if [[ -n "${CLI}" ]]; then
        if "${CLI}" airdrop "${amount}" "${pubkey}" \
            --url "${RPC_URL}" \
            > /dev/null 2>&1; then
            info "  ✓ ${name}: ${pubkey} — ${amount} TRV1"
            ((funded++)) || true
        else
            warn "  ✗ ${name}: ${pubkey} — airdrop failed"
        fi
    fi
done

echo ""

# ── Summary ───────────────────────────────────────────────────────────────────
log "╔═══════════════════════════════════════════════════════════════╗"
log "║              Test Accounts Funded                             ║"
log "╠═══════════════════════════════════════════════════════════════╣"
log "║  Funded: ${funded} accounts                                  ║"
log "║  Keypairs saved in: ${ACCOUNTS_DIR}/                         ║"
log "║  RPC: ${RPC_URL}                                             ║"
log "╚═══════════════════════════════════════════════════════════════╝"
echo ""
log "Use keypairs with:"
log "  solana --keypair ${ACCOUNTS_DIR}/alice.json balance --url ${RPC_URL}"

#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────────────
# stop-testnet.sh — Stop all running TRv1 testnet validators
#
# Reads PIDs from the testnet PID file and sends SIGTERM, falling back to
# SIGKILL after a grace period.  Also kills any stray trv1-validator and
# solana-faucet processes.
#
# Usage:
#   ./scripts/local-testnet/stop-testnet.sh [--force]
#
# Flags:
#   --force   Skip graceful shutdown; send SIGKILL immediately.
# ──────────────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TESTNET_DIR="${ROOT_DIR}/test-ledger/local-testnet"
PID_FILE="${TESTNET_DIR}/pids"
GRACE_SECONDS=10
FORCE="${1:-}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

log()  { echo -e "${GREEN}[TRv1]${NC} $*"; }
warn() { echo -e "${YELLOW}[TRv1]${NC} $*"; }
err()  { echo -e "${RED}[TRv1]${NC} $*" >&2; }

killed=0

# ── Stop by PID file ─────────────────────────────────────────────────────────
if [[ -f "${PID_FILE}" ]]; then
    log "Reading PIDs from ${PID_FILE}..."
    while read -r pid; do
        [[ -z "${pid}" ]] && continue
        if kill -0 "${pid}" 2>/dev/null; then
            if [[ "${FORCE}" == "--force" ]]; then
                log "Killing PID ${pid} (SIGKILL)..."
                kill -9 "${pid}" 2>/dev/null || true
            else
                log "Stopping PID ${pid} (SIGTERM)..."
                kill "${pid}" 2>/dev/null || true
            fi
            ((killed++)) || true
        else
            warn "PID ${pid} is not running."
        fi
    done < "${PID_FILE}"
    rm -f "${PID_FILE}"
else
    warn "No PID file found at ${PID_FILE}."
fi

# ── Wait for graceful shutdown ────────────────────────────────────────────────
if [[ "${FORCE}" != "--force" && ${killed} -gt 0 ]]; then
    log "Waiting up to ${GRACE_SECONDS}s for graceful shutdown..."
    for i in $(seq 1 "${GRACE_SECONDS}"); do
        remaining=0
        # Re-check known processes
        if pgrep -f "trv1-validator.*local-testnet" > /dev/null 2>&1; then
            remaining=1
        fi
        if [[ ${remaining} -eq 0 ]]; then
            log "All validators stopped gracefully."
            break
        fi
        if [[ ${i} -eq ${GRACE_SECONDS} ]]; then
            warn "Grace period expired. Sending SIGKILL to stragglers..."
            pkill -9 -f "trv1-validator.*local-testnet" 2>/dev/null || true
        fi
        sleep 1
    done
fi

# ── Kill any stray processes ──────────────────────────────────────────────────
for proc_name in "trv1-validator" "trv1-test-validator" "solana-faucet"; do
    if pgrep -f "${proc_name}" > /dev/null 2>&1; then
        warn "Found stray ${proc_name} processes, killing..."
        pkill -f "${proc_name}" 2>/dev/null || true
        ((killed++)) || true
    fi
done

# ── Summary ───────────────────────────────────────────────────────────────────
if [[ ${killed} -gt 0 ]]; then
    log "Stopped ${killed} process(es)."
else
    log "No running testnet processes found."
fi

log "Testnet stopped. Ledger data preserved at ${TESTNET_DIR}/"
log "Use './scripts/local-testnet/start-testnet.sh --reset' to start fresh."

#!/usr/bin/env bash
# =============================================================================
# TRv1 Genesis Block Creation Script
# =============================================================================
#
# This is a TEMPLATE / REFERENCE script for generating the TRv1 genesis block.
# It documents the required steps and parameters.  Some commands reference
# tooling that may not yet exist (e.g. trv1-genesis).  Adapt paths and
# parameters as the codebase evolves.
#
# Prerequisites:
#   - Rust toolchain installed (source "$HOME/.cargo/env")
#   - TRv1 binaries built (cargo build --release)
#   - solana-keygen available (from the Agave/Solana SDK)
#
# Usage:
#   ./scripts/create-genesis.sh [--output-dir <path>]
#
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

# Where to write genesis artefacts
OUTPUT_DIR="${1:-./genesis-output}"

# TRv1 binary directory (adjust after build)
BIN_DIR="./target/release"

# solana-keygen binary (ships with Agave)
KEYGEN="${BIN_DIR}/solana-keygen"

# Genesis tool (agave-ledger-tool or custom trv1-genesis)
GENESIS_TOOL="${BIN_DIR}/solana-genesis"

# ---------------------------------------------------------------------------
# Step 0: Ensure output directory exists
# ---------------------------------------------------------------------------
echo "==> Creating output directory: ${OUTPUT_DIR}"
mkdir -p "${OUTPUT_DIR}/keypairs"

# ---------------------------------------------------------------------------
# Step 1: Generate keypairs
# ---------------------------------------------------------------------------
echo "==> Generating keypairs..."

# Bootstrap validator identity
if [ ! -f "${OUTPUT_DIR}/keypairs/bootstrap-validator-identity.json" ]; then
    "${KEYGEN}" new --no-bip39-passphrase \
        -o "${OUTPUT_DIR}/keypairs/bootstrap-validator-identity.json"
    echo "    Created bootstrap validator identity"
else
    echo "    Bootstrap validator identity already exists, skipping"
fi

# Bootstrap validator vote account
if [ ! -f "${OUTPUT_DIR}/keypairs/bootstrap-validator-vote.json" ]; then
    "${KEYGEN}" new --no-bip39-passphrase \
        -o "${OUTPUT_DIR}/keypairs/bootstrap-validator-vote.json"
    echo "    Created bootstrap validator vote account"
else
    echo "    Bootstrap validator vote account already exists, skipping"
fi

# Bootstrap validator stake account
if [ ! -f "${OUTPUT_DIR}/keypairs/bootstrap-validator-stake.json" ]; then
    "${KEYGEN}" new --no-bip39-passphrase \
        -o "${OUTPUT_DIR}/keypairs/bootstrap-validator-stake.json"
    echo "    Created bootstrap validator stake account"
else
    echo "    Bootstrap validator stake account already exists, skipping"
fi

# Treasury config account
if [ ! -f "${OUTPUT_DIR}/keypairs/treasury-config.json" ]; then
    "${KEYGEN}" new --no-bip39-passphrase \
        -o "${OUTPUT_DIR}/keypairs/treasury-config.json"
    echo "    Created treasury config account"
else
    echo "    Treasury config account already exists, skipping"
fi

# Treasury token account (where fees accumulate)
if [ ! -f "${OUTPUT_DIR}/keypairs/treasury-token-account.json" ]; then
    "${KEYGEN}" new --no-bip39-passphrase \
        -o "${OUTPUT_DIR}/keypairs/treasury-token-account.json"
    echo "    Created treasury token account"
else
    echo "    Treasury token account already exists, skipping"
fi

# Faucet keypair (for testnet/devnet only)
if [ ! -f "${OUTPUT_DIR}/keypairs/faucet.json" ]; then
    "${KEYGEN}" new --no-bip39-passphrase \
        -o "${OUTPUT_DIR}/keypairs/faucet.json"
    echo "    Created faucet keypair"
else
    echo "    Faucet keypair already exists, skipping"
fi

# ---------------------------------------------------------------------------
# Step 2: Extract public keys
# ---------------------------------------------------------------------------
echo "==> Extracting public keys..."

BOOTSTRAP_IDENTITY=$("${KEYGEN}" pubkey "${OUTPUT_DIR}/keypairs/bootstrap-validator-identity.json")
BOOTSTRAP_VOTE=$("${KEYGEN}" pubkey "${OUTPUT_DIR}/keypairs/bootstrap-validator-vote.json")
BOOTSTRAP_STAKE=$("${KEYGEN}" pubkey "${OUTPUT_DIR}/keypairs/bootstrap-validator-stake.json")
TREASURY_CONFIG=$("${KEYGEN}" pubkey "${OUTPUT_DIR}/keypairs/treasury-config.json")
TREASURY_TOKEN=$("${KEYGEN}" pubkey "${OUTPUT_DIR}/keypairs/treasury-token-account.json")
FAUCET_PUBKEY=$("${KEYGEN}" pubkey "${OUTPUT_DIR}/keypairs/faucet.json")

echo "    Bootstrap identity:  ${BOOTSTRAP_IDENTITY}"
echo "    Bootstrap vote:      ${BOOTSTRAP_VOTE}"
echo "    Bootstrap stake:     ${BOOTSTRAP_STAKE}"
echo "    Treasury config:     ${TREASURY_CONFIG}"
echo "    Treasury token:      ${TREASURY_TOKEN}"
echo "    Faucet:              ${FAUCET_PUBKEY}"

# ---------------------------------------------------------------------------
# Step 3: Define TRv1 economic parameters
# ---------------------------------------------------------------------------
echo "==> Configuring economic parameters..."

# Network timing
TICKS_PER_SLOT=64                    # Solana default
SLOTS_PER_EPOCH=86400                # 1-day epochs at 1-second slots

# Token supply (in lamports; 1 TRv1 = 1_000_000_000 lamports)
# Placeholder: 1 billion TRv1 = 1e18 lamports
TOTAL_SUPPLY_LAMPORTS=1000000000000000000

# Bootstrap validator stake (placeholder: 10M TRv1)
BOOTSTRAP_STAKE_LAMPORTS=10000000000000000

# Treasury initial balance (placeholder: 100M TRv1)
TREASURY_INITIAL_BALANCE=100000000000000000

# Faucet balance (testnet only)
FAUCET_BALANCE=500000000000000000

# Inflation: 5% annual, flat
INFLATION_INITIAL=0.05
INFLATION_TERMINAL=0.05
INFLATION_TAPER=1.0    # No taper (flat rate)

echo "    Ticks per slot:      ${TICKS_PER_SLOT}"
echo "    Slots per epoch:     ${SLOTS_PER_EPOCH}"
echo "    Total supply:        ${TOTAL_SUPPLY_LAMPORTS} lamports"
echo "    Bootstrap stake:     ${BOOTSTRAP_STAKE_LAMPORTS} lamports"
echo "    Treasury balance:    ${TREASURY_INITIAL_BALANCE} lamports"
echo "    Inflation:           ${INFLATION_INITIAL} (flat)"

# ---------------------------------------------------------------------------
# Step 4: Generate the genesis block
# ---------------------------------------------------------------------------
echo "==> Generating genesis block..."

# NOTE: The exact CLI flags depend on the TRv1 genesis tool implementation.
# Below is modelled on Solana's solana-genesis with TRv1 modifications.
# Adjust flags as the tooling evolves.

"${GENESIS_TOOL}" \
    --ledger "${OUTPUT_DIR}/ledger" \
    --bootstrap-validator \
        "${OUTPUT_DIR}/keypairs/bootstrap-validator-identity.json" \
        "${OUTPUT_DIR}/keypairs/bootstrap-validator-vote.json" \
        "${OUTPUT_DIR}/keypairs/bootstrap-validator-stake.json" \
    --bootstrap-stake-authorized-pubkey "${BOOTSTRAP_IDENTITY}" \
    --bootstrap-validator-lamports "${BOOTSTRAP_STAKE_LAMPORTS}" \
    --bootstrap-validator-stake-lamports "${BOOTSTRAP_STAKE_LAMPORTS}" \
    --ticks-per-slot "${TICKS_PER_SLOT}" \
    --slots-per-epoch "${SLOTS_PER_EPOCH}" \
    --faucet-pubkey "${FAUCET_PUBKEY}" \
    --faucet-lamports "${FAUCET_BALANCE}" \
    --inflation initial="${INFLATION_INITIAL}" \
    --inflation terminal="${INFLATION_TERMINAL}" \
    --inflation taper="${INFLATION_TAPER}" \
    --lamports-per-byte-year 3480 \
    --max-genesis-archive-unpacked-size 1073741824 \
    || {
        echo "WARNING: Genesis generation failed. This is expected if tooling"
        echo "         is not yet fully built.  Review the command above and"
        echo "         adapt once the genesis binary is available."
    }

# ---------------------------------------------------------------------------
# Step 5: Fund the treasury account at genesis
# ---------------------------------------------------------------------------
echo "==> Configuring treasury genesis accounts..."

# The treasury needs two accounts at genesis:
#   1. Treasury CONFIG account — owned by the treasury program, stores
#      TreasuryConfig state (authority, tracking, etc.)
#   2. Treasury TOKEN account — holds the actual lamports (fee accumulation).
#
# In a production genesis, these would be added as primordial accounts
# via the genesis tool's --primordial-accounts-file flag.

cat > "${OUTPUT_DIR}/treasury-primordial-accounts.yaml" <<EOF
# TRv1 Treasury Primordial Accounts
# ----------------------------------
# These accounts are created in the genesis block with the specified
# balances and ownership.

# Treasury token account — holds accumulated fees
- pubkey: "${TREASURY_TOKEN}"
  balance: ${TREASURY_INITIAL_BALANCE}
  owner: "11111111111111111111111111111111"
  data: ""
  executable: false

# Treasury config account — stores TreasuryConfig state
# Owner is the treasury program.  Data is written by InitializeTreasury
# instruction during the first epoch (or baked into genesis data).
- pubkey: "${TREASURY_CONFIG}"
  balance: 1000000  # Rent-exempt minimum
  owner: "Treasury11111111111111111111111111111111111"
  data: ""
  executable: false
EOF

echo "    Wrote treasury primordial accounts to:"
echo "    ${OUTPUT_DIR}/treasury-primordial-accounts.yaml"

# ---------------------------------------------------------------------------
# Step 6: Document the multisig configuration
# ---------------------------------------------------------------------------
echo "==> Documenting multisig configuration..."

cat > "${OUTPUT_DIR}/multisig-config.md" <<'EOF'
# Treasury Multisig Configuration

The treasury is controlled by a **5-of-7 multisig** at launch.

## Signers

| # | Role | Pubkey |
|---|------|--------|
| 1 | Core team lead | TBD |
| 2 | Core team engineer | TBD |
| 3 | Core team engineer | TBD |
| 4 | Advisor | TBD |
| 5 | Advisor | TBD |
| 6 | Community representative | TBD |
| 7 | Community representative | TBD |

## Threshold

- **5 of 7** signatures required for any treasury operation.
- The multisig pubkey is set as `TreasuryConfig.authority`.

## Transition to Governance

When governance is deployed:

1. Deploy governance program
2. Call `UpdateAuthority { new_authority: <governance_pda> }` (requires 5/7)
3. Call `ActivateGovernance` (requires new governance authority)

This is a one-way transition.
EOF

echo "    Wrote multisig configuration to:"
echo "    ${OUTPUT_DIR}/multisig-config.md"

# ---------------------------------------------------------------------------
# Step 7: Write a summary
# ---------------------------------------------------------------------------
echo ""
echo "============================================================"
echo "  TRv1 Genesis Configuration Complete"
echo "============================================================"
echo ""
echo "  Output directory:    ${OUTPUT_DIR}"
echo "  Keypairs:            ${OUTPUT_DIR}/keypairs/"
echo "  Primordial accounts: ${OUTPUT_DIR}/treasury-primordial-accounts.yaml"
echo "  Multisig config:     ${OUTPUT_DIR}/multisig-config.md"
echo ""
echo "  Bootstrap validator: ${BOOTSTRAP_IDENTITY}"
echo "  Treasury config:     ${TREASURY_CONFIG}"
echo "  Treasury token:      ${TREASURY_TOKEN}"
echo ""
echo "  Next steps:"
echo "    1. Fill in multisig signer pubkeys"
echo "    2. Finalize total token supply"
echo "    3. Build genesis tool (cargo build --release)"
echo "    4. Re-run this script with functional genesis binary"
echo "    5. Run InitializeTreasury instruction in first epoch"
echo ""
echo "============================================================"

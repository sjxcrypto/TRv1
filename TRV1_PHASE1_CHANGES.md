# TRv1 Phase 1: Rebrand & Economic Model Changes

## Summary of All Changes

### Task 1: Rebrand Constants

**Binary renames:**
- `genesis/Cargo.toml`: Package renamed from `solana-genesis` to `trv1-genesis`, binary from `solana-genesis` to `trv1-genesis`, lib name from `solana_genesis` to `trv1_genesis`
- `validator/Cargo.toml`: Package renamed from `agave-validator` to `trv1-validator`, default-run updated
- `validator/src/cli.rs`: Test validator app name changed from `solana-test-validator` to `trv1-test-validator`, about text updated
- `Cargo.toml` (workspace): Updated workspace dependency reference from `solana-genesis` to `trv1-genesis`
- `bench-tps/Cargo.toml`: Updated dependency reference
- `bench-tps/src/keypairs.rs` and `bench-tps/src/main.rs`: Updated `solana_genesis::` imports to `trv1_genesis::`
- `dev-bins/Cargo.toml`: Updated dependency reference
- `programs/sbf/Cargo.toml`: Updated `agave-validator` to `trv1-validator`
- `programs/sbf/tests/simulation.rs`: Updated `agave_validator::` import to `trv1_validator::`

**User-facing string changes:**
- `genesis/src/main.rs`: "URL for Solana's JSON RPC" → "URL for TRv1's JSON RPC"
- `validator/src/cli.rs`: "URL for Solana's JSON RPC" → "URL for TRv1's JSON RPC"
- `validator/src/bin/solana-test-validator.rs`: "`solana airdrop`" → "`trv1 airdrop`", `agave_validator::` → `trv1_validator::`
- `validator/src/main.rs`: `agave_validator::` → `trv1_validator::`
- `genesis/src/main.rs`: `solana_genesis::` → `trv1_genesis::` (import)

### Task 2: Inflation/Staking Reward Model

**File: `runtime/src/bank.rs`** — `calculate_epoch_inflation_rewards()` method

**Before (Solana model):**
- Used Inflation struct's declining schedule (8% → 1.5%)
- Applied inflation rate to total supply (capitalization)
- `rewards = validator_rate * capitalization * epoch_duration_in_years`

**After (TRv1 model):**
- Flat 5% annual staking rate (from `trv1_constants::STAKING_RATE`)
- Applied only to staked supply (from stake history's `effective` field)
- `rewards = staked_supply * 0.05 * epoch_duration_in_years`
- Foundation rate set to 0 (no foundation allocation from inflation)
- The `_capitalization` parameter is now ignored (kept for API compatibility)

**Key difference:** Total supply inflation is now proportional to staking participation. If 50% of tokens are staked, effective inflation is 2.5% on total supply. If 80% are staked, it's 4%.

### Task 3: Fee Distribution

**File: `runtime/src/bank/fee_distribution.rs`**

**Before (Solana model):**
- Static 50% burn / 50% to validator
- Priority fees go entirely to validator

**After (TRv1 model):**
- Four-way split: burn / validator / treasury / dApp developer
- Linear transition over 1825 epochs (~5 years at daily epochs):
  - Launch: 10% burn / 0% validator / 45% treasury / 45% dev
  - Maturity: 25% burn / 25% validator / 25% treasury / 25% dev
- `FeeDistribution` struct expanded with `treasury` and `dev` fields
- `calculate_reward_and_burn_fee_details()` uses `trv1_constants::fee_distribution_for_epoch()`
- Rounding remainder goes to burn
- **TODO markers** left for:
  - Treasury account delivery (currently treasury share goes to burn)
  - Per-program revenue recipient lookup (currently dev share goes to burn)
- Tests updated to reflect new epoch-0 distribution (0% to validator)

### Task 4: Configuration Constants

**File: `runtime/src/trv1_constants.rs`** (NEW)

Contains all TRv1-specific economic constants:
- `STAKING_RATE`: 5% annual flat rate
- `FEE_TRANSITION_EPOCHS`: 1825 (~5 years)
- Launch fee percentages: 10/0/45/45 (burn/validator/treasury/dev)
- Mature fee percentages: 25/25/25/25
- `PASSIVE_STAKE_TIERS`: 6 tiers from no-lock (5%) to permanent (120%)
- `EARLY_UNLOCK_PENALTY_MULTIPLIER`: 5x
- `ACTIVE_VALIDATOR_CAP`: 200
- Slashing parameters: 5%/10%/25% for double-sign/invalid-block/repeat
- `JAIL_OFFLINE_HOURS`: 24
- `fee_distribution_for_epoch()` helper function with unit tests

**Registered in:** `runtime/src/lib.rs` as `pub mod trv1_constants`

### Files Modified (complete list)
1. `genesis/Cargo.toml` — package/binary rename
2. `genesis/src/main.rs` — branding string
3. `validator/Cargo.toml` — package rename
4. `validator/src/cli.rs` — branding strings, app name
5. `validator/src/bin/solana-test-validator.rs` — branding string
6. `Cargo.toml` (workspace root) — dependency rename
7. `bench-tps/Cargo.toml` — dependency rename
8. `bench-tps/src/keypairs.rs` — import rename
9. `bench-tps/src/main.rs` — import rename
10. `dev-bins/Cargo.toml` — dependency rename
11. `programs/sbf/Cargo.toml` — dependency rename
12. `programs/sbf/tests/simulation.rs` — import rename
13. `runtime/src/lib.rs` — added trv1_constants module
14. `runtime/src/trv1_constants.rs` — NEW: all TRv1 constants
15. `runtime/src/bank.rs` — inflation rewards calculation
16. `runtime/src/bank/fee_distribution.rs` — fee distribution model + tests

### Known TODOs for Phase 2
1. **Treasury account delivery**: Actually send treasury fee share to a configured pubkey
2. **Per-program revenue recipients**: Look up program owners and send dev share to them
3. **Passive stake tiers**: Implement lock-duration-based reward multipliers
4. **Validator active set cap**: Enforce 200-validator limit in leader schedule
5. **Slashing**: Implement double-sign, invalid-block, and offline jailing
6. **Rename the file** `validator/src/bin/solana-test-validator.rs` to `trv1-test-validator.rs`

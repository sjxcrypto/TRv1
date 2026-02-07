# TRv1 Phase 1: Validator Set Management & Slashing

## Summary of Changes

### New Files

#### 1. `runtime/src/trv1_active_set.rs` — Active Validator Set Management
- **`ActiveValidatorSet`** struct with `compute()`, `is_active()`, `filter_active_vote_accounts()`, and ranking helpers.
- At each epoch boundary, vote accounts are aggregated by node identity, sorted by total stake descending, and the top 200 (configurable via `trv1_constants::ACTIVE_VALIDATOR_CAP`) form the active set.
- Jailed/banned validators are always placed in the standby set regardless of stake.
- `filter_active_vote_accounts()` produces a filtered `VoteAccountsHashMap` suitable for feeding directly to `LeaderSchedule::new()`.

#### 2. `runtime/src/slashing.rs` — Slashing & Jailing Infrastructure
- **`SlashingConfig`** with default penalties:
  - Double-sign: 5% of validator's own stake
  - Invalid block: 10% of validator's own stake
  - Repeat offense (3rd strike): 25% + permanent ban
- **`ValidatorJailStatus`** per-node tracking: `is_jailed`, `jail_until_epoch`, `offense_count`, `permanently_banned`, `last_seen_slot`.
- **`SlashingState`** runtime container with methods:
  - `slash_validator()` — computes penalty, increments offense count, applies jail
  - `jail_validator()` / `unjail_validator()` — manual jail/unjail; unjailing is free but respects time windows
  - `check_offline_validators()` — auto-jails nodes offline > 24h (216,000 slots)
  - `jailed_set()` — returns `HashSet<Pubkey>` of all currently jailed/banned nodes
- **Key design**: Only the validator's own stake is ever slashed. Delegators' stake accounts are **never** touched.
- 8 unit tests covering first offense, invalid block, 3-strike ban, unjail timing, unjail after permanent ban (fails), offline detection, and jailed set queries.

### Modified Files

#### 3. `runtime/src/lib.rs`
- Registered two new public modules: `slashing` and `trv1_active_set`.

#### 4. `runtime/src/bank.rs` — Bank Struct & Epoch Processing
- **New field**: `trv1_slashing: Arc<RwLock<SlashingState>>` added to the `Bank` struct.
- **Initialization**: Field initialized in all 3 Bank construction paths:
  - `new_from_genesis()` — fresh `SlashingState::new()`
  - `_new_from_parent()` — cloned from parent (inherits slashing state across slots)
  - Deserialization path — fresh `SlashingState::new()`
- **PartialEq**: `trv1_slashing` added to the destructuring pattern (ignored for equality, like other runtime caches).
- **Accessors**: `trv1_slashing_state()` (read) and `trv1_slashing_state_mut()` (write) added to Bank impl.
- **`process_new_epoch()`**: Added offline-validator detection at epoch boundary:
  - Scans all vote accounts for each node's latest voted slot
  - Calls `SlashingState::check_offline_validators()` to auto-jail nodes that haven't voted in 216,000+ slots
  - Logs newly jailed validators

#### 5. `runtime/src/leader_schedule_utils.rs` — Leader Schedule Filtering
- **`leader_schedule()`** now:
  1. Reads the jailed set from `bank.trv1_slashing_state()`
  2. Computes `ActiveValidatorSet` from vote accounts + jailed set
  3. Filters vote accounts to only active validators
  4. Passes the filtered set to `LeaderSchedule::new()`
- **Effect**: Only the top 200 non-jailed validators appear in the leader schedule. Standby validators never produce blocks and never receive transaction fee income.

#### 6. `runtime/src/bank/fee_distribution.rs` — Fee Distribution Comments
- Added documentation clarifying that the active set enforcement happens at the leader-schedule level, ensuring standby validators never receive transaction fee income.

#### 7. `runtime/src/bank/partitioned_epoch_rewards/calculation.rs` — Staking Rewards Filtering
- **`redeem_delegation_rewards()`**: Added early return (`None`) for delegations to jailed/banned validators.
- **Effect**: Jailed and permanently-banned validators earn 0 staking rewards. Their delegators also receive 0 for the jailed epoch, incentivizing redelegation away from bad validators.

### Reward Model Summary

| Validator Type | Staking Rewards | Transaction Fees |
|----------------|----------------|-----------------|
| Active (top 200, not jailed) | ✅ Full 5% APR | ✅ Yes (via leader schedule) |
| Standby (rank > 200, not jailed) | ✅ Full 5% APR | ❌ No (not in leader schedule) |
| Jailed/Offline | ❌ 0 | ❌ No |
| Permanently Banned | ❌ 0 | ❌ No |

### Slashing Summary

| Offense | Penalty | Jail Duration |
|---------|---------|---------------|
| Double-sign (1st) | 5% own stake | ~7 days (4 epochs) |
| Invalid block (1st) | 10% own stake | ~7 days (4 epochs) |
| Any offense (2nd) | Same as offense type | ~30 days (15 epochs) |
| 3rd offense (any type) | 25% own stake | Permanent ban |
| Offline > 24h | None (auto-jail) | ~7 days (4 epochs) |

### Architecture Notes
- The `SlashingState` is shared via `Arc<RwLock<>>` and inherited by child banks, so the jail state persists across the fork tree.
- The active set is recomputed dynamically each time the leader schedule is requested, ensuring automatic rotation when stake weights change.
- The `trv1_constants` module (previously created) already defines `ACTIVE_VALIDATOR_CAP = 200`, `STAKING_RATE = 0.05`, and all slashing percentages. The new modules reference these constants.

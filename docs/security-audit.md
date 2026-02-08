# TRv1 Security Audit Report

**Date:** 2026-02-08  
**Auditor:** TRv1 Internal Security Review (Phase 4)  
**Scope:** Consensus BFT, Fee Market, Slashing, Passive Staking, Developer Rewards  
**Codebase Commit:** Phase 3 HEAD  

---

## Executive Summary

This audit reviewed five security-critical components of TRv1: the Tendermint-style BFT consensus engine, EIP-1559 fee market, slashing infrastructure, passive staking program, and developer rewards program. The review identified **3 CRITICAL**, **5 HIGH**, **8 MEDIUM**, **4 LOW**, and **6 INFO** findings. The most severe issues relate to missing signature verification in consensus messages, floating-point arithmetic in slashing calculations, and potential manipulation of the developer rewards epoch fee cap.

---

## Table of Contents

1. [Consensus BFT Engine](#1-consensus-bft-engine)
2. [Fee Market Calculator](#2-fee-market-calculator)
3. [Slashing & Jailing](#3-slashing--jailing)
4. [Passive Staking](#4-passive-staking)
5. [Developer Rewards](#5-developer-rewards)
6. [Cross-Component Issues](#6-cross-component-issues)
7. [Remediation Summary](#7-remediation-summary)

---

## 1. Consensus BFT Engine

### C-1: Message Signatures Are Never Verified — **CRITICAL**

**File:** `consensus-bft/src/engine.rs` (all message handlers)

All consensus messages carry a `Signature` field, but the engine **never verifies any signature**. Every message handler (`on_proposal`, `on_prevote`, `on_precommit`) only checks that the sender is in the validator set — not that the message was actually signed by the claimed sender.

**Impact:** Any network participant can forge consensus messages (prevotes, precommits, proposals) from any validator by simply setting the `voter`/`proposer` field to a legitimate validator's pubkey. This completely breaks Byzantine fault tolerance — a single attacker can:
- Force commits on arbitrary blocks by forging 2/3+ precommits.
- Trigger false double-sign evidence against honest validators.
- Prevent consensus by sending conflicting forged votes.

**Remediation:**
1. Add `fn verify_signature(&self, msg: &ConsensusMessage) -> bool` that verifies the Ed25519 signature over the message payload.
2. Call `verify_signature()` at the top of every message handler, reject unsigned/invalid messages.
3. Use `solana_keypair::Keypair` to sign outgoing messages in `make_prevote()` and `make_precommit()` (currently they use `Signature::default()`).

---

### C-2: Commit Signatures Use `Signature::default()` — **CRITICAL**

**File:** `consensus-bft/src/engine.rs`, `try_commit()` line:

```rust
.map(|(k, _)| (*k, Signature::default())) // TODO: store actual signatures
```

When building a `CommittedBlock`, the engine collects precommit signatures but replaces all of them with `Signature::default()` (all zeros). The committed block therefore has **no cryptographic proof of validator commitment**.

**Impact:** The committed block cannot be verified by light clients or other nodes. Anyone can fabricate a `CommittedBlock` with arbitrary signatures. This invalidates the entire deterministic finality guarantee.

**Remediation:**
1. Store actual signatures when processing precommit messages (add a `signatures` field to the precommit tracking `HashMap`).
2. Include real signatures in the `CommittedBlock`.
3. Add verification of commit signatures when receiving committed blocks from peers.

---

### H-1: Future-Round Prevotes Are Silently Dropped — **HIGH**

**File:** `consensus-bft/src/engine.rs`, `on_prevote()`:

```rust
if *round > self.state.round {
    // TODO: track future-round votes and skip if quorum reached
    return EngineOutput::empty();
}
```

Prevotes and precommits for future rounds are silently discarded. In Tendermint, if a validator receives 2/3+ votes for a future round `r > current_round`, it should immediately advance to round `r`. Without this, a single slow node can get stuck in an earlier round while the rest of the network has moved on, creating a liveness failure.

**Impact:** If network conditions cause round desynchronization, validators without future-round vote tracking will not catch up, potentially stalling consensus for those validators. While the network as a whole can still commit (if enough validators are in sync), stragglers may never recover without external intervention.

**Remediation:**
1. Maintain a separate buffer for votes from rounds > `self.state.round`.
2. After each vote insertion, check if 2/3+ stake exists for any future round.
3. If a future round has quorum participation, advance to that round.

---

### H-2: `max_rounds_per_height` Only Logs a Warning — **HIGH**

**File:** `consensus-bft/src/engine.rs`, `on_timeout(ConsensusStep::Precommit)`:

```rust
if next_round >= self.config.max_rounds_per_height {
    warn!("Max rounds ({}) reached at height {}", ...);
    // Still advance but log the warning
}
```

When `max_rounds_per_height` (default: 5) is exceeded, the engine only logs a warning but continues advancing rounds. This means:
1. The round counter can overflow `u32::MAX` given enough time (though this would take billions of rounds).
2. The propose timeout grows linearly with round: `base + delta * round`, which for large round values will exceed reasonable time bounds.

**Impact:** Under sustained liveness failure, the propose timeout grows unbounded: at round 1000, timeout = 3000 + 500 * 1000 = 503 seconds. At round 100,000, it's 50,003 seconds (~14 hours). This effectively creates a denial-of-service on height progression.

**Remediation:**
1. Cap the propose timeout at a reasonable maximum (e.g., 30 seconds).
2. When `max_rounds_per_height` is reached, either (a) emit a special "stall" event for monitoring, or (b) request view change / validator set update.

---

### M-1: Proposer Selection Seed Is Too Simple — **MEDIUM**

**File:** `consensus-bft/src/proposer.rs`:

```rust
let seed = height.wrapping_add(round as u64);
let target = seed % total_stake;
```

The proposer selection seed is simply `height + round`. This is predictable and can be gamed:
- A validator with stake at a known position can compute which heights they will propose for.
- By manipulating the validator set (e.g., splitting stake across multiple identities), an attacker can increase their proposer frequency beyond their fair share.

**Impact:** While all validators agree on the same proposer (so safety is preserved), a sophisticated attacker with control over validator set composition could gain disproportionate block proposal opportunities. The linear relationship `height + round` also means height=1,round=5 produces the same proposer as height=6,round=0.

**Remediation:**
1. Use a hash-based seed: `sha256(height || round || prev_block_hash)` to break the linear relationship.
2. Consider adopting Tendermint's "incremental proposer priority" algorithm which rotates proportional to stake more evenly.

---

### M-2: No Proposal Block Validation — **MEDIUM**

**File:** `consensus-bft/src/engine.rs`, `determine_prevote()`:

```rust
// Rule 3: not locked, prevote for the proposal
// (In production, we'd validate the block here)
Some(*block_hash)
```

The engine unconditionally prevotes for any proposed block (when unlocked) without validating:
- Transaction validity
- State root correctness
- Parent hash continuity
- Timestamp sanity

**Impact:** A Byzantine proposer can propose a block with invalid transactions or an incorrect state root, and honest validators will prevote for it. This only causes damage if the invalid block is committed, at which point the chain state becomes inconsistent.

**Remediation:**
1. Add a `BlockValidator` trait that the engine caller implements.
2. Call `block_validator.validate(block)` in `determine_prevote()` before voting.
3. Nil-prevote if validation fails.

---

### M-3: Lock/Unlock Protocol Deviation from Tendermint — **MEDIUM**

**File:** `consensus-bft/src/engine.rs`, `determine_prevote()`:

In Tendermint, a locked validator should only unlock when it sees **proof** (a valid polka certificate) for the proposed value at `valid_round`. The current implementation trusts the proposer's `valid_round` claim without verification:

```rust
if vr >= lr {
    // The proposer claims a polka in valid_round.
    // In a full implementation, we'd verify this claim.
    // For now, trust the proposer's valid_round.
    return Some(*block_hash);
}
```

**Impact:** A Byzantine proposer can set `valid_round` to any value ≥ the target's `locked_round`, causing locked validators to unlock prematurely. This can lead to two conflicting blocks being committed at the same height, violating safety.

**Remediation:**
1. Require proposers to include a "polka certificate" (2/3+ prevotes for the block at `valid_round`).
2. Verify the polka certificate in `determine_prevote()` before unlocking.

---

### M-4: Finality Threshold Uses Floating Point — **MEDIUM**

**File:** `consensus-bft/src/config.rs`, `validator_set.rs`:

```rust
pub fn quorum_stake(&self, threshold: f64) -> u64 {
    let q = (self.total_stake as f64 * threshold).ceil() as u64;
    q.max(1)
}
```

The quorum calculation uses `f64` multiplication, which is subject to platform-dependent rounding behavior. While IEEE 754 is deterministic on a single platform, cross-platform behavior can differ for edge cases. Additionally, `0.667` is an approximation of `2/3` — the mathematically correct threshold is `2*total_stake/3 + 1`.

**Impact:** On extreme stake values (close to `u64::MAX`), the `f64` conversion loses precision (doubles only have 53 bits of mantissa vs 64 bits for u64). Two validators with different floating-point implementations could disagree on whether quorum was met.

**Remediation:**
1. Use pure integer arithmetic: `quorum = total_stake * 2 / 3 + 1`.
2. Remove the `finality_threshold` config parameter and hardcode the 2/3+1 rule.
3. If configurable thresholds are needed, use a numerator/denominator pair (e.g., `2/3`).

---

### L-1: Evidence Collector Never Bounds Memory on Evidence Vec — **LOW**

**File:** `consensus-bft/src/evidence.rs`

The `evidence` vector is never pruned — `prune()` only removes old votes, not old evidence. If there are many double-signs over time, the evidence vector grows unboundedly.

**Remediation:** Add a maximum evidence capacity or prune evidence older than N heights.

---

### I-1: `ConsensusState` Fields Are All `pub` — **INFO**

The `ConsensusState` struct has all fields `pub`, allowing external code to modify consensus state directly (e.g., setting `locked_value`, `locked_round`, `step`). While this is used in tests, production code should use accessor methods to prevent invalid state transitions.

---

### I-2: Block Hash Does Not Include Transactions — **INFO**

**File:** `consensus-bft/src/types.rs`, `ProposedBlock::hash()`:

The block hash is computed from `parent_hash`, `height`, `timestamp`, `state_root`, and `proposer` — but not the transactions themselves. The assumption is that `state_root` covers them, but this is only true if the state root is a Merkle root over the executed transactions. If two different transaction sets produce the same state root (unlikely but possible for empty/no-op transactions), the blocks would be indistinguishable.

---

## 2. Fee Market Calculator

### M-5: Validator Can Manipulate Base Fee via Block Stuffing — **MEDIUM**

**File:** `fee-market/src/calculator.rs`

A validator proposing a block controls `parent_gas_used` by choosing which transactions to include. By filling blocks with self-sent transactions (paying fees to themselves via priority fees), a validator can artificially inflate the base fee to force competing users to pay more.

**Impact:** While the validator pays the base fee (which is burned/distributed), they recoup priority fees. If the validator's total cost of block stuffing is less than the revenue from the increased base fees on subsequent blocks' transactions, this is profitable.

**Remediation:**
1. Consider a multi-block rolling average for utilization rather than single-parent.
2. Add a maximum base fee change per epoch.
3. Monitor for patterns of single-validator block stuffing.

---

### L-2: Integer Division Truncation in Fee Decrease — **LOW**

**File:** `fee-market/src/calculator.rs`:

When the block is just barely below target (e.g., 23,999,999 out of 24,000,000), the delta calculation rounds to 0 due to integer division, and the base fee doesn't change. This is by design (matching Ethereum's behavior), but it means the base fee has a "sticky" behavior near the target — very small deviations below target don't trigger any decrease. The inverse is not true: being 1 CU above target always increases the fee by at least 1 (due to the `max(delta, 1)` rule).

**Impact:** Minor asymmetry that biases the fee slightly upward over time. The test suite already verifies this behavior (`exact_value_just_below_target`).

**Remediation:** Accepted behavior per EIP-1559 spec. Document the asymmetry.

---

### I-3: `target_gas()` Can Return 0 — **INFO**

When `target_utilization_pct = 0`, the target gas is 0, causing the fee to jump to `max_base_fee` for any non-empty block. This edge case is handled correctly in `calculate_next_base_fee` but should be documented as a known configuration foot-gun.

---

### I-4: Saturating Arithmetic Preserves Correctness — **INFO**

All fee calculations use `saturating_mul`, `saturating_add`, `saturating_sub`, and `u128` intermediaries. No overflow/underflow vulnerabilities were found. The `calculate_transaction_fee` correctly saturates to `u64::MAX` for extreme inputs, and `calculate_next_base_fee` correctly clamps to `[min_base_fee, max_base_fee]`. The test suite (`transaction_fee_saturates`, `edge_case_very_large_base_fee`) verifies this.

---

## 3. Slashing & Jailing

### H-3: Penalty Calculation Uses Floating Point — **HIGH**

**File:** `runtime/src/slashing.rs`, `slash_validator()`:

```rust
let lamports_slashed = (own_stake_lamports as f64 * penalty_fraction).round() as u64;
```

The slash amount is computed using `f64` multiplication with `round()`. This has multiple problems:
1. **Precision loss**: For stakes > 2^53 lamports (~9 quadrillion, unlikely but not impossible with delegation), the conversion to `f64` loses precision.
2. **Non-determinism risk**: Different validators may compute different slash amounts for the same input due to floating-point rounding differences across platforms.
3. **Rounding attack**: A validator with stake exactly at a rounding boundary could receive a smaller slash than intended.

**Impact:** If different validators disagree on the exact slash amount, they will produce different state roots, causing a chain split.

**Remediation:**
1. Use basis-point integer arithmetic: `lamports_slashed = own_stake_lamports * penalty_bps / 10_000`.
2. Convert all penalty fractions from `f64` to `u64` basis points.
3. Example: `double_sign_penalty: 0.05` → `double_sign_penalty_bps: 500`.

---

### H-4: Slashing Calculates Penalty on Current Stake, Not At-Offense Stake — **HIGH**

**File:** `runtime/src/slashing.rs`, `slash_validator()`:

```rust
pub fn slash_validator(
    &mut self,
    validator: &Pubkey,
    offense: SlashOffense,
    own_stake_lamports: u64,  // caller provides current stake
    current_epoch: u64,
) -> Option<SlashResult> {
```

The caller passes the current stake, not the stake at the time of the offense. If a validator learns they are about to be slashed (e.g., from observing evidence in the mempool), they can quickly withdraw most of their stake, then the slash applies to the reduced amount.

**Impact:** A validator with 1M SOL stake who double-signs could withdraw to 1 SOL before the slash transaction processes, paying only 0.05 SOL penalty instead of 50,000 SOL.

**Remediation:**
1. Implement an "unbonding period" during which stake cannot be withdrawn.
2. Record the offense with the stake snapshot at evidence submission time.
3. Apply the slash to `max(current_stake, stake_at_offense_time)`.

---

### M-6: Jailed Validators May Still Receive Rewards — **MEDIUM**

**File:** `runtime/src/slashing.rs`

The `SlashingState` tracks jail status but does **not** integrate with the rewards distribution system. There is no function that filters jailed validators from reward eligibility. The `is_jailed_or_banned()` and `jailed_set()` methods exist for the caller to use, but there is no enforcement at the slashing module level.

**Impact:** If the rewards distribution code doesn't check the jailed set, jailed validators continue earning staking rewards during their sentence, weakening the economic penalty.

**Remediation:**
1. Add `is_eligible_for_rewards(&self, validator: &Pubkey) -> bool` that returns `false` for jailed/banned validators.
2. Document that the rewards module MUST call this check at distribution time.
3. Add an integration test verifying that jailed validators receive zero rewards.

---

### L-3: Offense Count Saturates at u8::MAX — **LOW**

**File:** `runtime/src/slashing.rs`:

```rust
status.offense_count = status.offense_count.saturating_add(1);
```

After permanent ban at offense 3, the offense count still increments (saturating at 255). Since `slash_validator` returns `None` for banned validators, this is harmless but wastes a tiny amount of storage.

**Remediation:** Return `None` before incrementing if already banned (already done — this is INFO-level).

---

### I-5: Jail Duration Epoch Conversion Is Approximate — **INFO**

```rust
let epochs = (self.config.jail_duration_first + 431_999) / 432_000;
```

The slot-to-epoch conversion assumes 432,000 slots/epoch. If epoch lengths change, jail durations will be wrong. Consider using the actual epoch schedule from the runtime.

---

## 4. Passive Staking

### H-5: `CalculateEpochRewards` Has No Access Control — **HIGH**

**File:** `programs/passive-stake/src/processor.rs`, `process_calculate_epoch_rewards`:

```rust
/// Accounts:
///   0. `[writable]` — Passive stake account.
```

The `CalculateEpochRewards` instruction requires no signer. The `current_epoch` and `validator_reward_rate` are passed as instruction data, not read from the chain state. This means:

1. **Anyone can call it** with arbitrary epoch and rate values.
2. An attacker can call it with `validator_reward_rate = 10_000` (100%) and `current_epoch = 1_000_000` to credit enormous rewards.
3. The only guard is `current_epoch > state.last_reward_epoch`, which is easily satisfied by passing a very large epoch.

**Impact:** Complete theft of the rewards pool. An attacker creates a passive stake account, then calls `CalculateEpochRewards` with inflated parameters to credit unlimited rewards, then calls `ClaimRewards` to drain the pool.

**Remediation:**
1. **Critical fix**: This instruction should only be callable by the runtime (via a privileged signer or CPI from the epoch rewards program).
2. Read `current_epoch` from the Clock sysvar instead of instruction data.
3. Read `validator_reward_rate` from the staking parameters sysvar instead of instruction data.
4. Add a signer requirement (e.g., the reward distribution authority or the runtime itself).

---

### M-7: Early Unlock Penalty Burn Has Accounting Gap — **MEDIUM**

**File:** `programs/passive-stake/src/processor.rs`, `process_early_unlock`:

```rust
// Remove all lamports from the stake account.
let total_lamports = stake_account.get_lamports();
stake_account.checked_sub_lamports(total_lamports)?;

// Credit only the post-penalty amount back to the authority.
let mut authority_account = instruction_context.try_borrow_instruction_account(0)?;
authority_account.checked_add_lamports(return_amount)?;
```

The code drains ALL lamports from the stake account (which includes rent-exempt reserve, not just the staked amount) but only credits `return_amount = amount - penalty`. The difference between `total_lamports` and `return_amount` is effectively burned.

**Issue:** If `total_lamports > amount` (which happens when the account holds rent-exempt minimum + staked amount), the burn includes the rent-exempt lamports that legitimately belong to the user. The penalty should only apply to the staked `amount`, not the rent reserve.

**Impact:** Users who early-unlock lose more than the intended penalty. The excess loss equals the rent-exempt reserve (~0.00089 SOL for an 84-byte account).

**Remediation:**
```rust
let rent_reserve = total_lamports.saturating_sub(state.amount);
let penalty_from_stake = state.amount * penalty_bps / BPS_DENOMINATOR;
let return_to_user = state.amount - penalty_from_stake + rent_reserve;
```

---

### M-8: Permanent Lock Can Be Bypassed via Account Closure — **MEDIUM**

**File:** `programs/passive-stake/src/processor.rs`

The `Unlock` instruction rejects permanent locks:
```rust
if state.is_permanent {
    return Err(PassiveStakeError::EarlyUnlockNotAllowed.into());
}
```

And `EarlyUnlock` also rejects them:
```rust
if state.is_permanent {
    return Err(PassiveStakeError::EarlyUnlockNotAllowed.into());
}
```

However, there is no protection against the **program owner** changing the program to allow permanent unlock in a future upgrade, or against a bug in account garbage collection that might return lamports from a zeroed account.

More immediately: the passive stake account is owned by the program. If the program's upgrade authority is not frozen, the deployer could push a malicious upgrade that drains permanent locks.

**Impact:** Permanent lockers (who receive the highest reward rate of 120% validator rate + 1.5x governance weight) are trusting the program's immutability. If the program can be upgraded, this trust is misplaced.

**Remediation:**
1. Freeze the program's upgrade authority before mainnet launch.
2. Alternatively, use a timelock + multisig for upgrade authority.
3. Document that permanent lock security depends on program immutability.

---

### L-4: Reward Calculation Assumes 365 Epochs/Year — **LOW**

**File:** `programs/passive-stake/src/processor.rs`:

```rust
.checked_mul(365)
```

The reward formula divides by 365, assuming one epoch per day. If TRv1 uses a different epoch length, rewards will be miscalculated.

**Remediation:** Pass the actual epochs-per-year as a parameter, or derive it from the epoch schedule.

---

## 5. Developer Rewards

### C-3: Epoch Fee Cap Can Be Bypassed via Multiple Transactions — **CRITICAL**

**File:** `programs/developer-rewards/src/processor.rs`, `process_credit`:

```rust
let projected_total = tracker.total_developer_fees.saturating_add(amount);
let max_allowed = (projected_total as u128)
    .saturating_mul(MAX_PROGRAM_FEE_SHARE_BPS as u128)
    .checked_div(TOTAL_BPS as u128)
    .unwrap_or(0) as u64;

if projected_program > max_allowed {
    return Err(DeveloperRewardsError::EpochFeeCapExceeded.into());
}
```

The cap check computes `max_allowed` based on `projected_total`, which **includes the current credit amount**. This means:

- If `total_developer_fees = 0` and `amount = 100`, then `projected_total = 100`, `max_allowed = 100 * 1000 / 10000 = 10`, and `projected_program = 100 > 10` → **rejected**.
- But if the program is the **only** program receiving fees, and it sends 10 transactions each crediting 10, the first transaction sets `total = 10`, `max_allowed = 1`, `projected_program = 10 > 1` → still rejected.

Actually, re-analyzing: the check works correctly when there are multiple programs. But when a single program dominates:
- Transaction 1: amount=10, projected_total=10, max_allowed=1, projected_program=10 → **rejected**

The real vulnerability is different: a wash-trading attacker creates 10+ programs and distributes fees across them, then consolidates the claimed lamports to a single wallet. The per-program cap doesn't prevent this because the attacker controls multiple programs.

**Impact:** The 10% per-program cap is trivially bypassed by creating multiple programs. A single entity deploying 10 programs can capture 100% of developer fees.

**Remediation:**
1. Add per-wallet caps: track total fees claimed by each recipient address.
2. Add a unique-user metric: only count fees from transactions signed by distinct fee payers.
3. Implement a reputation/scoring system that weights established programs higher.
4. Consider Sybil-resistance mechanisms (e.g., stake-weighted eligibility).

---

### H-6: ClaimDeveloperFees Sends All Funds to account[3] Ignoring Revenue Splits — **HIGH** (BUG)

**File:** `programs/developer-rewards/src/processor.rs`, `process_claim`:

```rust
// Account 3+: recipient(s)
{
    let mut recipient_account = instruction_context.try_borrow_instruction_account(3)?;
    recipient_account.checked_add_lamports(claim_amount)?;
}
```

The claim function sends 100% of `claim_amount` to account index 3, completely ignoring `config.revenue_splits`. Even if splits are configured, the entire amount goes to a single account.

**Impact:** Revenue splits are effectively non-functional. This is a correctness bug rather than a security vulnerability, but it means multi-recipient configurations don't work.

**Remediation:**
```rust
if config.revenue_splits.is_empty() {
    // Send to primary recipient (account 3)
    let mut recipient = instruction_context.try_borrow_instruction_account(3)?;
    recipient.checked_add_lamports(claim_amount)?;
} else {
    for (i, split) in config.revenue_splits.iter().enumerate() {
        let share = (claim_amount as u128 * split.share_bps as u128 / TOTAL_BPS as u128) as u64;
        let mut recipient = instruction_context.try_borrow_instruction_account(3 + i)?;
        recipient.checked_add_lamports(share)?;
    }
}
```

---

### M-9: `CreditDeveloperFees` Lacks Privileged Signer Check — **MEDIUM**

**File:** `programs/developer-rewards/src/processor.rs`, `process_credit`:

The instruction documentation states it is "Runtime-only" and "not by external users," but the actual code does not enforce this. Any user can craft a `CreditDeveloperFees` transaction to credit arbitrary amounts to any program's revenue config.

**Impact:** An attacker can credit fees to their own program without any actual fee-generating transactions occurring. Combined with the permissionless `ClaimDeveloperFees`, this allows draining the developer fee pool.

**Remediation:**
1. Require a privileged runtime signer (e.g., a well-known system program address).
2. Alternatively, verify the credit against actual fee collection records.

---

### M-10: Cooldown Can Be Circumvented by Pre-Registration — **MEDIUM**

**File:** `programs/developer-rewards/src/processor.rs`, `process_register`:

```rust
eligible_after_slot: current_slot.saturating_add(COOLDOWN_SLOTS),
```

The cooldown is 7 days from registration. An attacker planning wash-trading can register their programs 7 days in advance, then execute the attack on day 8 when all programs are eligible.

**Impact:** The cooldown only delays gaming, it doesn't prevent it. It stops completely opportunistic attacks but not planned ones.

**Remediation:**
1. Add a sustained activity requirement (e.g., minimum N unique callers over M days) in addition to the cooldown.
2. Implement a graduated fee share (e.g., 20% in week 2, 50% in week 3, 100% in week 4+).

---

### I-6: Revenue Config Has No Deactivation Mechanism — **INFO**

There is no instruction to deactivate a `ProgramRevenueConfig` once created. If a program is abandoned or malicious, its revenue config continues earning fees indefinitely. Consider adding an admin deactivation instruction.

---

## 6. Cross-Component Issues

### H-7: No Integration Between Consensus Evidence and Slashing — **HIGH**

The consensus BFT engine collects `DoubleSignEvidence` via `EvidenceCollector`, and the slashing module has `slash_validator()`. However, there is no code that connects them — no mechanism to submit evidence from consensus to the slashing module. The evidence collector has `drain_evidence()`, but nothing calls it in a context that feeds into `SlashingState::slash_validator()`.

**Impact:** Double-signing goes completely unpunished in the current codebase.

**Remediation:**
1. Add a runtime hook at epoch boundaries or block processing that:
   - Calls `evidence_collector.drain_evidence()`
   - For each evidence item, calls `slashing_state.slash_validator()`
2. Create an on-chain evidence submission instruction that any validator can call.

---

## 7. Remediation Summary

| ID | Severity | Component | Finding | Priority |
|----|----------|-----------|---------|----------|
| C-1 | CRITICAL | Consensus | Message signatures never verified | P0 — Block before mainnet |
| C-2 | CRITICAL | Consensus | Commit signatures are all zeros | P0 — Block before mainnet |
| C-3 | CRITICAL | Dev Rewards | Epoch fee cap bypassed via Sybil programs | P0 — Design change needed |
| H-1 | HIGH | Consensus | Future-round votes dropped | P1 — Liveness issue |
| H-2 | HIGH | Consensus | max_rounds unbounded timeout growth | P1 — DoS risk |
| H-3 | HIGH | Slashing | Float penalty calculation non-deterministic | P1 — Chain split risk |
| H-4 | HIGH | Slashing | Slash on current stake, not at-offense | P1 — Economic exploit |
| H-5 | HIGH | Passive Stake | Epoch rewards has no access control | P0 — Theft vector |
| H-6 | HIGH | Dev Rewards | Claim ignores revenue splits (bug) | P1 — Feature broken |
| H-7 | HIGH | Cross | No evidence→slashing integration | P1 — Slashing non-functional |
| M-1 | MEDIUM | Consensus | Predictable proposer selection seed | P2 |
| M-2 | MEDIUM | Consensus | No proposal block validation | P2 |
| M-3 | MEDIUM | Consensus | Lock/unlock trusts unverified valid_round | P1 — Safety issue |
| M-4 | MEDIUM | Consensus | Float quorum calculation | P1 — Consensus divergence |
| M-5 | MEDIUM | Fee Market | Validator block stuffing manipulation | P2 |
| M-6 | MEDIUM | Slashing | Jailed validators not filtered from rewards | P2 |
| M-7 | MEDIUM | Passive Stake | Early unlock burns rent reserve | P2 |
| M-8 | MEDIUM | Passive Stake | Permanent lock depends on program immutability | P2 |
| M-9 | MEDIUM | Dev Rewards | CreditDeveloperFees has no privileged signer | P1 |
| M-10 | MEDIUM | Dev Rewards | Cooldown circumvented by pre-registration | P2 |
| L-1 | LOW | Consensus | Evidence vec unbounded | P3 |
| L-2 | LOW | Fee Market | Asymmetric fee adjustment near target | P3 — By design |
| L-3 | LOW | Slashing | Offense count increments after ban | P3 |
| L-4 | LOW | Passive Stake | Assumes 365 epochs/year | P3 |
| I-1 | INFO | Consensus | Public fields on ConsensusState | P3 |
| I-2 | INFO | Consensus | Block hash excludes transactions | P3 |
| I-3 | INFO | Fee Market | target_gas() can return 0 | P3 |
| I-4 | INFO | Fee Market | Saturating arithmetic is correct | N/A — Positive finding |
| I-5 | INFO | Slashing | Approximate epoch conversion | P3 |
| I-6 | INFO | Dev Rewards | No config deactivation mechanism | P3 |

### Priority Legend
- **P0**: Must fix before any testnet/mainnet deployment
- **P1**: Must fix before mainnet, acceptable on testnet with monitoring
- **P2**: Should fix before mainnet, low risk on testnet
- **P3**: Nice to have, can be addressed post-launch

---

*End of Security Audit Report*

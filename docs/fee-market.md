# TRv1 Dynamic Fee Market (EIP-1559)

## Overview

TRv1 replaces Solana's fixed `lamports_per_signature` fee model with an **EIP-1559-style dynamic base fee** that adjusts every block based on network utilization.  Every transaction pays:

```
total_fee = (base_fee_per_cu + priority_fee_per_cu) × compute_units_used
```

- **Base fee** — set by the protocol; rises when blocks are congested, falls when they're underutilized.  This component is *burned* (removed from circulation).
- **Priority fee (tip)** — set by the user; goes to the block producer as an incentive.  Higher tips get better ordering.

---

## How the Dynamic Base Fee Works

### Per-block adjustment

At the start of each block, the validator computes the new base fee from the **parent block's utilization**:

```
target = max_block_compute_units × target_utilization_pct / 100
```

| Condition | Formula |
|---|---|
| Parent CU = target | `next_base_fee = current_base_fee` (no change) |
| Parent CU > target | `next_base_fee = current_base_fee + current_base_fee × (parent_cu − target) / target / denominator` |
| Parent CU < target | `next_base_fee = current_base_fee − current_base_fee × (target − parent_cu) / target / denominator` |

The result is always clamped to **[min_base_fee, max_base_fee]**.

When above target, the increase is at least **+1 lamport/CU** (matching go-ethereum's implementation) to guarantee convergence even at very low base fees.

### Default parameters

| Parameter | Value | Rationale |
|---|---|---|
| `min_base_fee` | 5 000 lamports/CU | Same order as Solana's 5 000 lamports/signature |
| `max_base_fee` | 50 000 000 lamports/CU | Hard ceiling to prevent runaway costs |
| `target_utilization_pct` | 50 % | Blocks should be half-full on average |
| `max_block_compute_units` | 48 000 000 CU | Matches Solana mainnet block limit |
| `base_fee_change_denominator` | 8 | ±12.5 % max change per block |
| `min_priority_fee` | 0 | No forced tip |

### Intuition

- **Sustained congestion** → base fee climbs exponentially (12.5 % per block) until demand subsides or the ceiling is hit.
- **Sustained emptiness** → base fee decays exponentially toward the floor.
- **At steady state** (50 % utilization) → base fee is constant — the "sweet spot" the mechanism targets.

---

## Comparison with Ethereum's EIP-1559

| Aspect | Ethereum EIP-1559 | TRv1 |
|---|---|---|
| Resource unit | Gas | Compute units (CU) |
| Target block size | 15 M gas (50 % of 30 M) | 24 M CU (50 % of 48 M) |
| Adjustment denominator | 8 (±12.5 %) | 8 (±12.5 %) |
| Base fee floor | 0 (no floor) | 5 000 lamports/CU |
| Base fee ceiling | None | 50 000 000 lamports/CU |
| Base fee burned? | Yes | Yes (via TRv1's 4-way split: burn portion) |
| Priority fee recipient | Block proposer | Block producer (validator) |
| `maxFeePerGas` / `maxPriorityFeePerGas` | Per-transaction user fields | Equivalent: `offered_lamports` and `priority_fee_per_cu` |
| Block elasticity | 2× (target = ½ max) | Same — target = 50 % of max |

**Key difference:** TRv1 adds a **min/max clamp** so the base fee can never drop to zero (preventing free spam) or spike to infinity.

---

## Comparison with Solana's Priority Fees

| Aspect | Solana (current) | TRv1 |
|---|---|---|
| Base fee model | Fixed `lamports_per_signature` (currently 5 000) | Dynamic `base_fee_per_cu`, adjusted every block |
| Priority fee | Optional micro-lamports per CU (auction) | `priority_fee_per_cu` (tip to producer) |
| Congestion signal | None in the fee itself; validators may drop txs | Base fee rises automatically |
| Anti-spam | Minimal — fixed cost even under congestion | Self-regulating — spam gets exponentially expensive |
| Fee predictability | Perfectly predictable (fixed) | Predictable within one block (base fee known ahead of time) |
| Fee destination | 50 % burn / 50 % validator | Phase 1: epoch-dependent 4-way split (burn / validator / treasury / dev) |

**Why the change?**  Solana's fixed fee means that during high demand the fee doesn't reflect scarcity; users compete only through priority fees in an opaque auction.  A dynamic base fee makes congestion pricing transparent and automatic.

---

## How Base Fee + Priority Fee Interact

```
┌────────────────────────────────────────────┐
│              Transaction Fee               │
│                                            │
│   base_fee_per_cu × CU   = base fee       │  ← set by protocol, burned
│ + priority_fee_per_cu × CU = priority fee  │  ← set by user, to producer
│ ──────────────────────────────────────────  │
│ = total_fee                                │
└────────────────────────────────────────────┘
```

1. **Users see the current base fee** (published by the RPC / block header).
2. They choose a **priority fee** based on how urgently they want inclusion.
3. The **total fee must be ≤ `offered_lamports`** (balance check).
4. The validator orders transactions by **priority fee** (highest-tip-first).

### Example

Current base fee: **10 000 lamports/CU**.  Alice sends a transaction using **200 000 CU** with a **500 lamports/CU** tip:

```
base_fee   = 10 000 × 200 000 = 2 000 000 000 lamports (2 SOL)
priority   =    500 × 200 000 =   100 000 000 lamports (0.1 SOL)
total_fee  =                    2 100 000 000 lamports (2.1 SOL)
```

---

## Fee Estimation for Wallets

Wallets should:

1. **Query the latest `base_fee_per_cu`** from the RPC (included in the block header / fee-market state).
2. **Add a small buffer** — since the base fee can change by up to 12.5 % per block, a safe `maxFee` is:

   ```
   safe_max_base = current_base_fee × 1.125^N
   ```

   where *N* is the number of blocks you're willing to wait.  For immediate inclusion, *N = 1* → multiply by **1.125**.

3. **Set `priority_fee_per_cu`** based on urgency:
   - Low urgency: 0 (or the network minimum)
   - Normal: recent median priority fee
   - High: recent 75th-percentile priority fee

4. **Pre-flight check:** call `validate_transaction_fee(offered, priority, base_fee, cu, config)` client-side to guarantee the transaction won't be rejected.

### Estimator pseudocode

```python
def estimate_fee(current_base_fee, cu, priority_per_cu, blocks_ahead=1):
    max_base = current_base_fee * (1.125 ** blocks_ahead)
    return int(max_base * cu + priority_per_cu * cu)
```

---

## Anti-Spam Properties

1. **Exponential cost increase under attack.**  A spammer filling 100 % of blocks causes the base fee to grow by 12.5 % per block.  After 16 full blocks the base fee has *tripled*; after 50 blocks it is ~340× higher.  This makes sustained spam economically infeasible.

2. **Floor prevents free-ride.**  The `min_base_fee` of 5 000 lamports/CU means even empty-chain transactions aren't free, preventing dust attacks.

3. **Ceiling protects legitimate users.**  The `max_base_fee` cap ensures that even in worst-case congestion the fee cannot exceed a known bound, so wallets can always display a maximum possible cost.

4. **Base fee is burned.**  Because the base fee doesn't go to validators, block producers have no incentive to *create* artificial congestion to inflate fees (unlike a pure-auction model).

5. **Priority fee creates honest ordering.**  Users who genuinely need faster inclusion can outbid others with tips, creating a transparent market instead of opaque off-chain deals.

---

## Crate Structure

```
fee-market/
├── Cargo.toml
└── src/
    ├── lib.rs          — crate root, re-exports
    ├── config.rs       — FeeMarketConfig (tunables)
    ├── calculator.rs   — calculate_next_base_fee, calculate_transaction_fee, validate_*
    ├── state.rs        — BlockFeeState, TransactionFee
    ├── error.rs        — FeeError enum
    └── tests.rs        — 57 unit tests covering all edge cases
```

## Future Work

- **Integration into `Bank`**: the `BlockFeeState` will be stored in the bank and updated at block boundaries; `calculate_next_base_fee` will be called in `new_from_parent`.
- **RPC exposure**: expose `base_fee_per_cu` in `getBlock` / `getFeeForMessage` RPCs.
- **Fee distribution**: base fee flows into TRv1's 4-way split (Phase 1); priority fee goes to the validator.
- **Compute-budget instruction**: the existing `SetComputeUnitPrice` will map to `priority_fee_per_cu`.

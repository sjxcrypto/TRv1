# TRv1 Consensus: Tendermint-style BFT

## Overview

TRv1 replaces Solana's Proof of History (PoH) clock-based consensus with a Tendermint-inspired Byzantine Fault Tolerant (BFT) consensus protocol. This gives TRv1 **deterministic finality** — once a block is committed, it can never be reverted — while targeting **1-second block time** and **~6-second finality** under normal network conditions.

## Why Replace PoH?

| Property | Solana PoH + Tower BFT | TRv1 BFT |
|---|---|---|
| **Finality** | Probabilistic (~12s optimistic, ~32 slots for safe) | Deterministic (~6s, 1 block + 2/3 signatures) |
| **Reorgs** | Possible (fork choice required) | Impossible (safety proven under <1/3 Byzantine) |
| **Clock dependency** | SHA-256 PoH clock required | No special clock; standard wall-clock + timeouts |
| **Leader schedule** | Epoch-based, complex | Round-robin weighted by stake, simple |
| **Block time** | ~400ms | ~1000ms (configurable) |
| **Liveness threshold** | Degrades gracefully | Halts if <2/3 online (expected for BFT) |
| **Hardware requirement** | Dedicated CPU core for PoH hashing | No special hardware |

## Three-Phase Commit Protocol

```
Height H, Round R:

  ┌──────────┐     ┌──────────┐     ┌───────────┐     ┌──────────┐
  │ NewRound │────▶│ Propose  │────▶│  Prevote  │────▶│Precommit │
  └──────────┘     └──────────┘     └───────────┘     └──────────┘
       ▲                                                     │
       │                                                     │
       │  ┌─────────────────────────────────────────────┐    │
       │  │           Timeout? Next Round               │◀───┘ (no quorum)
       │  └─────────────────────────────────────────────┘
       │                                                     │
       │                                               ┌─────▼─────┐
       └───────────────────────────────────────────────│  Commit   │
                    (height++)                          └───────────┘
                                                        (2/3+ precommits)
```

### Phase 1: Propose

1. The designated **proposer** (selected by stake-weighted round-robin) assembles a block from the transaction mempool.
2. The proposer broadcasts a `Proposal` message containing:
   - The proposed block (parent hash, height, timestamp, transactions, state root)
   - The round number
   - An optional `valid_round` if the proposer observed a "polka" (2/3+ prevotes) for this value in a prior round

### Phase 2: Prevote

Upon receiving a proposal, each validator evaluates it:

- **If not locked**: Prevote for the proposed block hash.
- **If locked on the same value**: Prevote for it.
- **If locked on a different value** AND the proposal has `valid_round >= locked_round`: Unlock and prevote for the new value (Tendermint polka rule).
- **If locked on a different value** with no valid unlock: Send a **nil prevote**.
- **If propose timeout expires**: Send a nil prevote.

### Phase 3: Precommit

After collecting prevotes:

- **If 2/3+ prevotes for a specific block hash** (a "polka"):
  - Lock on that value
  - Send a precommit for that hash
- **If 2/3+ total prevotes but no single hash has a quorum**:
  - Send a nil precommit
- **If prevote timeout expires without quorum**:
  - Send a nil precommit

### Commit

- **If 2/3+ precommits for a specific block hash**:
  - The block is **committed with deterministic finality**
  - Advance to the next height
- **If precommit timeout expires**:
  - Advance to the next round (R+1) at the same height
  - The propose timeout increases linearly: `base + delta * round`

## Finality Guarantees

### Deterministic Finality

Once a block receives 2/3+ precommits, it is finalized. No reorganization is possible because:

1. **Safety**: Two conflicting blocks at the same height would require >1/3 of stake to double-sign (impossible if <1/3 is Byzantine).
2. **Lock mechanism**: Validators lock on values they precommit for, preventing them from voting for a different value in the same height.
3. **Polka rule**: The `valid_round` mechanism allows safe unlocking when a newer polka is observed.

### Finality Timeline (Normal Operation)

```
T+0.0s  Block proposed
T+0.5s  Prevotes collected (network propagation)
T+1.0s  Precommits collected
T+1.0s  Block committed ← DETERMINISTIC FINALITY
```

Under normal conditions (low latency, honest supermajority), finality is reached in ~1 second. Worst case with full timeouts:

```
Propose timeout:  3000 + 500 * round  ms
Prevote timeout:  1000 ms
Precommit timeout: 1000 ms
─────────────────────────────────
Max round 0:      ~5000 ms (5 seconds)
```

With timeout escalation across rounds, finality is typically reached within 6 seconds.

## Timeout Schedule

| Step | Timeout | Notes |
|---|---|---|
| Propose | 3000 + 500 × round ms | Increases with round to accommodate slow proposers |
| Prevote | 1000 ms | Fixed; starts after 2/3+ total prevotes seen |
| Precommit | 1000 ms | Fixed; starts after 2/3+ total precommits seen |
| Commit | None | No timeout in commit state |

### Timeout Escalation

When a round fails (precommit timeout), the next round begins with a larger propose timeout. This ensures liveness: even if the network is slow, eventually the propose timeout will be large enough for the proposal to propagate.

```
Round 0: 3000ms propose timeout
Round 1: 3500ms propose timeout
Round 2: 4000ms propose timeout
Round 3: 4500ms propose timeout
Round 4: 5000ms propose timeout (max_rounds_per_height = 5)
```

## Leader Selection Algorithm

Proposers are selected deterministically using stake-weighted round-robin:

```
fn proposer_for_round(validators, height, round) -> Pubkey:
    seed = height + round
    target = seed % total_stake
    
    accumulated = 0
    for validator in validators (sorted by stake desc, pubkey asc):
        accumulated += validator.stake
        if accumulated > target:
            return validator.pubkey
```

### Properties

- **Deterministic**: All validators agree on the proposer for any (height, round).
- **Stake-weighted**: Validators with more stake propose proportionally more often.
- **Input-order independent**: Validators are sorted canonically (by stake desc, pubkey asc).
- **Rotation**: Different heights/rounds select different proposers.

## Locking and Unlocking

The Tendermint lock mechanism prevents conflicting commits:

### Lock Rules

1. **Lock on polka**: When a validator sees 2/3+ prevotes for a hash, it locks on that hash.
2. **Locked prevote**: A locked validator only prevotes for its locked value (or nil).
3. **Unlock via polka**: A locked validator can unlock if a proposal carries `valid_round >= locked_round`, indicating a newer polka exists.
4. **Lock preserved across rounds**: When advancing to a new round, the lock is preserved; only votes are cleared.

### State Variables

```
locked_value: Option<Hash>    — hash the validator is locked on
locked_round: Option<u32>     — round in which the lock was acquired
valid_value:  Option<Hash>    — hash that received a valid polka
valid_round:  Option<u32>     — round of the valid polka
```

## Double-Sign Detection and Evidence

Validators must not cast conflicting votes at the same (height, round):
- Two different prevotes
- Two different precommits
- A nil vote and a value vote

### Evidence Collection

The `EvidenceCollector` monitors all incoming votes:

1. For each (height, round, voter, vote_type), it stores the first vote seen.
2. If a second, conflicting vote arrives, it records `DoubleSignEvidence`.
3. Evidence includes both conflicting votes with their signatures.
4. Evidence is preserved for submission to the slashing module.

### Evidence Types

| Type | Description |
|---|---|
| `ConflictingPrevote` | Two different prevotes in the same round |
| `ConflictingPrecommit` | Two different precommits in the same round |

### Memory Management

Old votes are pruned after 100 heights to bound memory. Evidence is retained until explicitly drained for slashing.

## Configuration

```rust
BftConfig {
    block_time_ms: 1000,           // Target 1-second blocks
    prevote_timeout_ms: 1000,      // 1s prevote timeout
    precommit_timeout_ms: 1000,    // 1s precommit timeout  
    finality_threshold: 0.667,     // 2/3+1 quorum
    max_rounds_per_height: 5,      // Max rounds before warning
    propose_timeout_base_ms: 3000, // 3s base propose timeout
    propose_timeout_delta_ms: 500, // +500ms per round
}
```

## Module Architecture

```
consensus-bft/
├── src/
│   ├── lib.rs              — Public API and re-exports
│   ├── config.rs           — BftConfig with validation
│   ├── types.rs            — ConsensusMessage, ProposedBlock, CommittedBlock, ConsensusState
│   ├── engine.rs           — Core state machine (ConsensusEngine)
│   ├── proposer.rs         — Deterministic stake-weighted leader selection
│   ├── validator_set.rs    — Weighted validator set management
│   ├── timeout.rs          — TimeoutScheduler for step timeouts
│   └── evidence.rs         — Double-sign detection (EvidenceCollector)
└── Cargo.toml
```

### Key Design Decisions

1. **Pure state machine**: The `ConsensusEngine` is deterministic and has no I/O. Networking, disk, and timers are handled externally. This makes the engine testable and auditable.

2. **Event-driven**: The engine processes events (`on_proposal`, `on_prevote`, `on_precommit`, `on_timeout`) and returns output messages. No internal threads or async.

3. **Separated concerns**: Proposer selection, timeout management, evidence collection, and validator set management are independent modules.

## Comparison with Solana's Tower BFT

### Solana Tower BFT
- Validators vote on slots using a **vote tower** (stack of votes with exponential lockout).
- Votes deeper in the tower take longer to expire, creating **probabilistic finality**.
- The PoH clock provides a **verifiable passage of time** between votes.
- Fork choice uses the **heaviest fork** weighted by stake and lockout.
- Finality is ~12 seconds for optimistic confirmation, ~32 slots for max lockout.

### TRv1 Tendermint BFT
- Validators participate in **explicit three-phase voting** per block.
- Locks and polka rules provide **deterministic finality** in a single round.
- No PoH clock needed — timeouts are wall-clock based.
- No fork choice needed — blocks are either committed or rejected.
- Finality is ~1-6 seconds (1 round under normal conditions).

### Trade-offs

| | Tower BFT | Tendermint BFT |
|---|---|---|
| **Throughput** | Higher (pipelining with PoH) | Lower (sequential blocks) |
| **Finality** | ~12-32s probabilistic | ~1-6s deterministic |
| **Partition tolerance** | Graceful degradation | Halts if <2/3 online |
| **Complexity** | High (PoH + tower + fork choice) | Moderate (3-phase + locks) |
| **Reorg safety** | Possible but rare | Impossible |

## Future Work

- **Message signing**: Currently uses placeholder signatures; integrate ed25519 signing.
- **Block validation**: Add transaction execution and state-root verification in prevote phase.
- **Network layer**: Integrate with gossip/p2p for message dissemination.
- **Slashing**: Connect evidence collector to stake slashing mechanism.
- **View change optimization**: Implement aggregate signatures for bandwidth efficiency.
- **Pipelining**: Explore starting execution of next height while current is in precommit phase.

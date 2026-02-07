# TRv1 Configuration Reference

> Comprehensive reference for all TRv1 validator, genesis, and network
> configuration parameters.  Values marked **adjustable** can be changed via
> CLI flags or environment variables.  Values marked **genesis** are baked
> into the genesis block and require a hard fork to change.

---

## Table of Contents

1. [Validator CLI Flags](#1-validator-cli-flags)
2. [Genesis Configuration](#2-genesis-configuration)
3. [Fee Market Parameters](#3-fee-market-parameters)
4. [Staking & Inflation Parameters](#4-staking--inflation-parameters)
5. [Passive Staking Tiers](#5-passive-staking-tiers)
6. [Slashing Parameters](#6-slashing-parameters)
7. [Network Parameters](#7-network-parameters)
8. [Storage Configuration](#8-storage-configuration)
9. [RPC Configuration](#9-rpc-configuration)
10. [Environment Variables](#10-environment-variables)
11. [Docker Configuration](#11-docker-configuration)

---

## 1. Validator CLI Flags

### Core Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--identity <KEYPAIR>` | *required* | Path to the validator's identity keypair (Ed25519) |
| `--vote-account <KEYPAIR>` | *required* | Path to the vote account keypair |
| `--ledger <PATH>` | `./ledger` | Directory for the ledger database |
| `--log` | off | Enable file-based logging (validator.log in ledger dir) |
| `--quiet` | off | Suppress all output |
| `--reset` | off | Wipe ledger and start fresh |

### Network Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--entrypoint <HOST:PORT>` | — | Gossip entrypoint for joining an existing network |
| `--gossip-port <PORT>` | 8001 | Port for gossip protocol |
| `--rpc-port <PORT>` | 8899 | Port for JSON-RPC endpoint |
| `--dynamic-port-range <LO-HI>` | `8002-8020` | Range for dynamically assigned ports (TPU, repair, etc.) |
| `--bind-address <IP>` | `0.0.0.0` | IP address to bind all services |
| `--gossip-host <IP>` | `127.0.0.1` | Advertised gossip IP address |

### Genesis / Epoch Flags (test-validator only)

| Flag | Default | Description |
|------|---------|-------------|
| `--slots-per-epoch <N>` | 86400 | Number of slots per epoch |
| `--ticks-per-slot <N>` | 64 | Number of ticks per slot |
| `--inflation-fixed <RATE>` | 0.05 | Fixed annual inflation rate |
| `--faucet-sol <AMOUNT>` | 500000000 | Initial faucet balance in TRV1 |
| `--mint <PUBKEY>` | random | Mint authority pubkey |

### Performance Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--compute-unit-limit <N>` | 48000000 | Maximum compute units per block |
| `--limit-ledger-size <SHREDS>` | unlimited | Maximum number of shreds in the ledger |
| `--log-messages-bytes-limit <N>` | unlimited | Maximum size of program log messages |
| `--transaction-account-lock-limit <N>` | 128 | Maximum number of accounts a transaction can lock |

### Geyser Plugin Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--geyser-plugin-config <PATH>` | — | Path to Geyser plugin configuration file (repeatable) |

### Bigtable Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--enable-rpc-bigtable-ledger-storage` | off | Enable Bigtable for historical block storage |
| `--enable-bigtable-ledger-upload` | off | Upload ledger data to Bigtable |
| `--rpc-bigtable-instance <NAME>` | — | Bigtable instance name |

---

## 2. Genesis Configuration

These parameters are set in the genesis block and define the fundamental
properties of the chain.

### Network Identity

| Parameter | Value | Adjustable |
|-----------|-------|:----------:|
| Chain name | TRv1 | ✗ (genesis) |
| Cluster type | `Development` / `Devnet` / `Testnet` / `MainnetBeta` | ✗ (genesis) |
| Genesis hash | SHA-256 of genesis block | ✗ |

### Timing

| Parameter | Value | Notes |
|-----------|-------|-------|
| Ticks per slot | 64 | Each tick ≈ 15.6 ms |
| Slots per epoch | 86,400 | 1 day at 1-second slot times |
| Target slot time | ~1 second | |
| Hashes per tick | Calibrated at launch | Hardware-dependent |
| Warmup epochs | Disabled | Epochs are fixed-length from genesis |

### Token Supply

| Parameter | Value | Notes |
|-----------|-------|-------|
| Total supply at genesis | TBD | Set in genesis config |
| Denomination | lamports | 1 TRV1 = 1,000,000,000 lamports |
| Native token name | TRV1 | |
| `LAMPORTS_PER_SOL` constant | 1,000,000,000 | Inherited from Solana (constant name unchanged) |

### Built-in Programs

| Program | ID | Status |
|---------|----|--------|
| System Program | `11111111111111111111111111111111` | Active |
| Vote Program | (Solana default) | Active |
| Stake Program | (Solana default) | Active |
| BPF Loader Upgradeable | `BPFLoaderUpgradeab1e11111111111111111111111` | Active |
| Passive Stake | `Pass1veStake1111111111111111111111111111111` | Active |
| Treasury | `Treasury11111111111111111111111111111111111` | Active |
| Governance | `Governance1111111111111111111111111111111111` | Active (disabled mode) |
| Developer Rewards | `DevRew11111111111111111111111111111111111111` | Active |

---

## 3. Fee Market Parameters

TRv1 uses an **EIP-1559-style dynamic base fee** mechanism instead of Solana's
static fee model.

### Base Fee Configuration

| Parameter | Default | Min | Max | Notes |
|-----------|---------|-----|-----|-------|
| `min_base_fee` | 5,000 lamports/CU | 1 | — | Floor to prevent zero-cost spam |
| `max_base_fee` | 50,000,000 lamports/CU | — | — | Ceiling during extreme congestion |
| `target_utilization_pct` | 50% | 1 | 100 | Target block utilization (sweet spot) |
| `max_block_compute_units` | 48,000,000 CU | — | — | Hard cap per block |
| `base_fee_change_denominator` | 8 | 1 | — | ±12.5% max change per block |
| `min_priority_fee` | 0 lamports/CU | 0 | — | Minimum tip (0 = optional tips) |

### Fee Calculation

```
total_fee = (base_fee_per_cu + priority_fee_per_cu) × compute_units_used
```

- **Base fee**: Protocol-set; rises above target, falls below. This portion is
  subject to the 4-way fee split (burn/validator/treasury/developer).
- **Priority fee (tip)**: User-set; goes to the block producer for ordering.

### Base Fee Adjustment Formula

```
target = max_block_compute_units × target_utilization_pct / 100

If parent_cu > target:
  next_base_fee = base_fee + base_fee × (parent_cu − target) / target / denominator
  next_base_fee = max(next_base_fee, base_fee + 1)  // minimum +1 for convergence

If parent_cu < target:
  next_base_fee = base_fee − base_fee × (target − parent_cu) / target / denominator

Clamp to [min_base_fee, max_base_fee]
```

### Fee Distribution Schedule

Fees are split four ways with a linear transition over 1,825 epochs (~5 years):

| Recipient | Epoch 0 (Launch) | Epoch 1825 (Maturity) | Transition |
|-----------|:----------------:|:---------------------:|:----------:|
| **Burn** | 10% | 25% | Linear increase |
| **Validator** | 0% | 25% | Linear increase |
| **Treasury** | 45% | 25% | Linear decrease |
| **Developer** | 45% | 25% | Linear decrease |

**Formula:**
```
progress = min(current_epoch / 1825, 1.0)
burn_pct      = 0.10 + progress × (0.25 − 0.10)
validator_pct = 0.00 + progress × (0.25 − 0.00)
treasury_pct  = 0.45 + progress × (0.25 − 0.45)
dev_pct       = 0.45 + progress × (0.25 − 0.45)
```

All percentages sum to 1.0 at every epoch. Rounding remainder goes to burn.

---

## 4. Staking & Inflation Parameters

### Inflation Model

| Parameter | Value | Notes |
|-----------|-------|-------|
| Annual staking rate | 5% (flat) | No declining curve — constant forever |
| Inflation source | Newly minted tokens | Applied to staked supply only |
| Foundation allocation | 0% | No foundation take from inflation |
| Taper rate | 1.0 (no taper) | Rate never changes |

**Effective supply inflation** depends on staking participation:

| Staking Participation | Effective Total Supply Inflation |
|:---------------------:|:-------------------------------:|
| 30% | 1.5% |
| 50% | 2.5% |
| 70% | 3.5% |
| 80% | 4.0% |
| 90% | 4.5% |
| 100% | 5.0% |

### Epoch Rewards Distribution

At each epoch boundary:
1. Calculate `rewards = staked_supply × 0.05 × (epoch_duration / 1_year)`
2. Distribute proportionally to all validators based on their effective stake
3. Validator commission applies before delegator distribution

### Validator Set

| Parameter | Value | Notes |
|-----------|-------|-------|
| Active validator cap | 200 | Top 200 by stake weight |
| Standby validators | Unlimited | Earn no rewards |
| Minimum stake | TBD | Governance-adjustable |

---

## 5. Passive Staking Tiers

Non-validator token holders earn yield by locking tokens. Rewards are a
percentage of the validator staking rate (5% APY).

| Lock Period | Rate (% of validator rate) | Approx APY | Governance Weight |
|:-----------:|:--------------------------:|:----------:|:-----------------:|
| No lock | 5% | 0.25% | 0× |
| 30 days | 10% | 0.50% | 0.10× |
| 90 days | 20% | 1.00% | 0.20× |
| 180 days | 30% | 1.50% | 0.30× |
| 360 days | 50% | 2.50% | 0.50× |
| Permanent | 120% | 6.00% | 1.50× |

### Early Unlock Penalties

| Lock Period | Penalty (% of principal) | Applied To |
|:-----------:|:------------------------:|:----------:|
| No lock | 0% | — |
| 30 days | 2.5% | Principal burned |
| 90 days | 5.0% | Principal burned |
| 180 days | 7.5% | Principal burned |
| 360 days | 12.5% | Principal burned |
| Permanent | **Cannot unlock** | — |

Penalty multiplier: `EARLY_UNLOCK_PENALTY_MULTIPLIER = 5.0` (5× the reward rate).

---

## 6. Slashing Parameters

Slashing applies **only to validators' own stake**. Delegator principal is
always 100% protected.

### Offence Types

| Offence | First Slash | Second (within 30 epochs) | Third+ |
|---------|:-----------:|:-------------------------:|:------:|
| Double-signing | 5% | 10% | 25% |
| Invalid block | 10% | 10% | 25% |
| Repeated offence | 25% | 25% | 25% |

### Jailing

| Parameter | Value | Notes |
|-----------|-------|-------|
| Jail trigger | Missing 86,400 consecutive slots (24 h) | |
| Jail duration | 1 epoch (24 h) | Jailed validator earns nothing |
| Unjail | Automatic after jail period | Validator must re-enter active set |

### Slashing Destinations

Slashed tokens are **burned** (removed from total supply), creating
deflationary pressure alongside the fee burn.

---

## 7. Network Parameters

### Consensus (Tendermint-style BFT)

| Parameter | Value | Notes |
|-----------|-------|-------|
| Consensus algorithm | Tendermint-style BFT | Replaces PoH + Tower BFT |
| Finality | ~6 seconds (deterministic) | After 6 confirmed blocks |
| Block time | ~1 second | 1 slot = 1 block |
| BFT threshold | ⅔ + 1 of stake weight | Prevote → Precommit → Commit |
| Leader selection | RANDAO-based | Weighted by stake |

### Gossip & Propagation

| Parameter | Value | Notes |
|-----------|-------|-------|
| Gossip protocol | Modified Solana gossip | Peer discovery and metadata |
| Block propagation | Simplified Turbine | Tree-based erasure-coded propagation |
| Propagation hops | ~8 | Full network coverage |
| Max gossip peers | Dynamic | Based on network size |

### Transaction Processing

| Parameter | Value | Notes |
|-----------|-------|-------|
| Execution engine | SVM/Sealevel | Parallel execution |
| Execution threads | 8–16 | Reduced from Solana's 32+ |
| Max block CU | 48,000,000 | Matches Solana mainnet |
| Max transaction CU | 1,400,000 | Per-transaction limit |
| Account lock limit | 128 | Max accounts per transaction |
| Program runtime | SBF (Solana Bytecode Format) | Anchor-compatible |

---

## 8. Storage Configuration

### Tiered Storage Architecture

TRv1 uses a 3-tier storage model to keep RAM requirements low while
maintaining high throughput:

```
┌──────────────────────────────────────────────────────────────────┐
│  HOT TIER (RAM)     │  WARM TIER (NVMe)   │  COLD TIER (Disk)  │
│  LRU Cache          │  RocksDB            │  Archive           │
│  16–32 GB           │  500 GB – 2 TB      │  Unbounded         │
│  ~1 μs access       │  ~100 μs access     │  ~10 ms access     │
│  Active accounts    │  Recent state       │  Historical data   │
└──────────────────────────────────────────────────────────────────┘
```

### Configuration Parameters

| Parameter | Default | Notes |
|-----------|---------|-------|
| **Hot tier size** | 16 GB | LRU cache in RAM; configurable via `--accounts-db-cache-limit-mb` |
| **Hot tier path** | `<ledger>/accounts/cache` | In-memory, no separate path |
| **Warm tier path** | `<ledger>/accounts/run` | NVMe recommended |
| **Cold tier path** | `<ledger>/accounts/archive` | HDD acceptable |
| **Snapshot interval** | 100 slots | Full snapshot for fast catch-up |
| **Incremental snapshot interval** | 10 slots | Reduced data transfer for syncing |
| **Max shreds in ledger** | Unlimited | Use `--limit-ledger-size` to cap |

### Accounts DB Configuration

| Parameter | CLI Flag | Default | Notes |
|-----------|----------|---------|-------|
| Cache limit | `--accounts-db-cache-limit-mb` | 16384 | MB of RAM for account cache |
| Index memory limit | — | Auto | Based on total account count |
| Compaction style | — | Universal | RocksDB compaction |
| Write buffer size | — | 64 MB | RocksDB write buffer |

### Recommended Hardware by Role

| Role | RAM | NVMe | HDD | CPU | Monthly Cost |
|------|-----|------|-----|-----|:------------:|
| **Full Validator** | 32 GB | 1 TB | Optional | 8-core | ~$50–100 |
| **RPC Node** | 64 GB | 2 TB | 4 TB | 16-core | ~$150–300 |
| **Archive Node** | 64 GB | 2 TB | 10+ TB | 16-core | ~$200–400 |
| **Light Client** | 4 GB | — | — | 2-core | ~$10–20 |

---

## 9. RPC Configuration

### JSON-RPC Configuration

| Parameter | Default | Notes |
|-----------|---------|-------|
| RPC port | 8899 | |
| WebSocket port | 8900 | For PubSub subscriptions |
| Max request body | 50 MB | |
| Transaction history | Enabled (test) | Historical transaction lookup |
| Extended metadata | Enabled (test) | Additional tx metadata |
| Health check endpoint | `/health` | Returns 200 when healthy |

### PubSub Configuration

| Parameter | Default | Notes |
|-----------|---------|-------|
| Max active subscriptions | 100,000 | Per-connection limit |
| Notification queue size | 100 | Queued before backpressure |
| Max log subscriptions | 100 | `logsSubscribe` limit |

### Rate Limiting (Production)

| Parameter | Recommended | Notes |
|-----------|-------------|-------|
| Requests/second | 100 | Per IP address |
| WebSocket connections | 10 | Per IP address |
| Burst allowance | 200 | Short-term burst |

### Supported RPC Methods

All standard Solana RPC methods are supported, plus TRv1 extensions:

- `getHealth` — Node health check
- `getSlot` — Current slot
- `getBlockHeight` — Current block height
- `getBalance` — Account balance
- `getTransaction` — Transaction details
- `sendTransaction` — Submit a transaction
- `requestAirdrop` — Request test tokens (testnet/devnet only)
- `getFeeForMessage` — Estimate fee (with dynamic base fee)
- `getRecentBlockhash` — Latest blockhash for transaction signing
- *(Future)* `getBaseFee` — Current EIP-1559 base fee
- *(Future)* `getFeeDistribution` — Current fee split percentages

---

## 10. Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Log level (`error`, `warn`, `info`, `debug`, `trace`) |
| `RUST_BACKTRACE` | `0` | Enable backtraces (`1` or `full`) |
| `TRV1_LEDGER_DIR` | `./ledger` | Override ledger directory |
| `TRV1_ACCOUNTS_DIR` | — | Override accounts storage path |
| `TRV1_SNAPSHOTS_DIR` | — | Override snapshot storage path |
| `TRV1_RPC_PORT` | `8899` | Override RPC port |
| `TRV1_GOSSIP_PORT` | `8001` | Override gossip port |
| `TRV1_FAUCET_PORT` | `9900` | Override faucet port |
| `TRV1_SLOTS_PER_EPOCH` | `86400` | Override slots per epoch |
| `TRV1_INFLATION` | `0.05` | Override inflation rate |
| `TRV1_MODE` | `test-validator` | Docker mode (`test-validator` or `validator`) |
| `TRV1_ENTRYPOINT` | — | Network entrypoint (Docker validator mode) |
| `TRV1_EXTRA_ARGS` | — | Additional CLI arguments (Docker) |

---

## 11. Docker Configuration

### Image Tags

| Tag | Description |
|-----|-------------|
| `trv1-validator:latest` | Latest build |
| `trv1-validator:<git-sha>` | Specific commit |
| `trv1-validator:v0.1.0` | Release version |

### Volumes

| Mount Point | Purpose |
|-------------|---------|
| `/data/ledger` | Ledger database |
| `/data/accounts` | Account storage |
| `/data/snapshots` | Snapshot storage |
| `/data/keys` | Validator keypairs |

### Ports

| Port | Protocol | Service |
|------|----------|---------|
| 8899 | TCP | JSON-RPC |
| 8900 | TCP | WebSocket (PubSub) |
| 8001 | UDP+TCP | Gossip |
| 8003 | UDP | TPU |
| 8004 | UDP | TPU Forwards |
| 9900 | TCP | Faucet |

### Docker Compose Profiles

The provided `docker-compose.yml` supports a 3-validator testnet:

| Service | Role | Host RPC Port |
|---------|------|:-------------:|
| `validator-1` | Bootstrap leader + test-validator | 8899 |
| `validator-2` | Follower | 8999 |
| `validator-3` | Follower | 9099 |

```bash
# Start the 3-node testnet
cd docker && docker compose up --build -d

# Check health
curl http://localhost:8899 -X POST \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}'

# View logs
docker compose logs -f validator-1

# Stop
docker compose down -v
```

---

## Appendix: Configuration File Locations

| File | Purpose |
|------|---------|
| `runtime/src/trv1_constants.rs` | Core economic constants (Rust source of truth) |
| `fee-market/src/config.rs` | Fee market configuration defaults |
| `fee-market/src/state.rs` | Fee market state structures |
| `test-validator/src/trv1_genesis.rs` | Test validator genesis setup |
| `docs/genesis-config.md` | Genesis parameter reference |
| `docs/fee-market.md` | Fee market deep-dive |
| `docs/configuration.md` | This file |

# TRv1 Tiered Storage Architecture

## Overview

TRv1 replaces Solana's memory-mapped Cloudbreak accounts database with a three-tier storage architecture that reduces hardware requirements from **512GB RAM** to as low as **32GB RAM** while maintaining competitive transaction throughput.

Solana's Cloudbreak memory-maps the entire accounts database into RAM. With over 400 million accounts averaging ~200 bytes each, this requires hundreds of gigabytes of RAM just for account storage. TRv1 recognizes that the vast majority of these accounts are inactive — the Pareto principle applies heavily, with a small fraction of accounts handling most transaction volume.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Transaction Runtime                       │
│                     (load_account / store_account)               │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                     AccountCache (Hot Tier)                       │
│                                                                   │
│  ┌─────────────┐    LRU Linked List                              │
│  │  HashMap     │    ┌──────┐──▶┌──────┐──▶┌──────┐──▶┌──────┐  │
│  │ Pubkey→Node  │    │ HEAD │   │      │   │      │   │ TAIL │  │
│  │  O(1) lookup │    │newest│◀──│      │◀──│      │◀──│oldest│  │
│  └─────────────┘    └──────┘   └──────┘   └──────┘   └──────┘  │
│                                                      ↑ evict    │
│  RAM: 16-48 GiB (configurable)                                   │
│  Latency: ~100ns                                                 │
└───────────────────────────────┬─────────────────────────────────┘
                    evict ↓           ↑ promote (cache miss)
┌─────────────────────────────────────────────────────────────────┐
│                     Warm Storage (NVMe SSD)                      │
│                                                                   │
│  Serialized accounts on fast NVMe storage                        │
│  Accounts inactive for >warm_threshold_slots (~4.6 days)         │
│  Latency: ~100μs random read                                    │
│  Capacity: 1-4 TB typical                                        │
└───────────────────────────────┬─────────────────────────────────┘
                    archive ↓         ↑ revive (with rent deposit)
┌─────────────────────────────────────────────────────────────────┐
│                     Cold Storage (Archive)                        │
│                                                                   │
│  Accounts inactive for >365 days                                 │
│  Merkle proofs for trustless verification                        │
│  Optional state rent expiry                                      │
│  Latency: ~10ms (HDD) or cloud storage                          │
│  Capacity: unlimited                                             │
└─────────────────────────────────────────────────────────────────┘
```

## Account Lifecycle

### 1. Active → Hot Tier (RAM)

When an account is accessed (read or write) during transaction processing, it is loaded into the hot cache. The cache uses an LRU (Least Recently Used) eviction policy by default, keeping the most recently accessed accounts in RAM.

```
Transaction accesses account "Alice"
  → AccountCache::get("Alice")
    → Cache HIT:  Return from RAM (~100ns)
    → Cache MISS: Load from warm/cold, insert into cache, return
```

### 2. Hot → Warm Tier (Eviction to SSD)

When the hot cache exceeds its configured size (`hot_cache_size * target_utilization`), background eviction kicks in. The least-recently-used accounts are serialized and written to NVMe SSD storage.

**Eviction triggers:**
- Cache size exceeds `hot_cache_size × target_utilization` (default 90%)
- Background eviction runs in batches of `eviction_batch_size` (default 4096)

**Eviction policies:**
- **LRU** (default): Evicts the least recently accessed accounts. Best for workloads with temporal locality.
- **LFU**: Evicts accounts with the fewest total accesses. Better when some accounts are accessed in bursts.
- **ARC**: Adaptively balances LRU and LFU using ghost lists. Best overall hit rate but higher overhead.

### 3. Warm → Cold Tier (Archival)

Accounts that have been inactive for longer than `cold_threshold_days` (default: 365 days) are candidates for archival to cold storage. This is controlled by the State Rent Expiry system.

**Archival process:**
1. `check_rent_expiry()` identifies eligible accounts
2. `archive_account()` serializes the account to cold storage
3. A Merkle proof is generated for trustless verification
4. The account's metadata is stored in the `ArchiveIndex`
5. The account is removed from the accounts database

### 4. Cold → Active (Revival)

Archived accounts can be revived by providing a rent deposit that covers at least 2 years of storage rent.

**Revival process:**
1. User submits a revival transaction with the account pubkey and rent deposit
2. `revive_account()` loads the data from cold storage
3. Integrity is verified using the stored hash
4. Merkle proof is verified (if available)
5. Account is restored to the accounts database with the rent deposit added

## Configuration

### TieredStorageConfig

| Parameter | Default | Description |
|-----------|---------|-------------|
| `hot_cache_size` | 16 GiB | Maximum RAM for the hot cache |
| `eviction_policy` | LRU | Cache eviction strategy |
| `warm_storage_path` | `trv1-warm-storage` | Path for NVMe SSD storage |
| `cold_storage_path` | `trv1-cold-storage` | Path for archive storage |
| `warm_threshold_slots` | 1,000,000 (~4.6 days) | Slots of inactivity before warm demotion |
| `cold_threshold_days` | 365 | Days before cold archival |
| `enable_state_rent_expiry` | false | Enable automatic archival |
| `eviction_batch_size` | 4,096 | Max accounts per eviction batch |
| `target_utilization` | 0.90 | Cache fill ratio that triggers eviction |

### Preset Configurations

```rust
// For validators with 32GB RAM (leaves 8GB for OS + runtime)
let config = TieredStorageConfig::for_32gb_validator();
// hot_cache_size = 24 GiB, LRU eviction

// For validators with 64GB+ RAM
let config = TieredStorageConfig::for_64gb_validator();
// hot_cache_size = 48 GiB, ARC eviction

// For testing
let config = TieredStorageConfig::for_testing();
// hot_cache_size = 64 MiB, short thresholds
```

### Cache Tuning Guide

**Cache size selection:**
- Reserve at least 8 GiB for OS, runtime, and other validator processes
- The hot cache should be sized to achieve >95% hit rate for your workload
- Monitor `TierStats::cache_hit_rate` and adjust accordingly

**Eviction policy selection:**
- **LRU**: Best default. Works well when recently-accessed accounts are likely to be accessed again.
- **LFU**: Better for DeFi workloads where popular AMM pools are accessed repeatedly but not continuously.
- **ARC**: Best for mixed workloads but adds ~10% overhead per operation.

**Monitoring:**
```rust
let stats = cache.stats();
println!("{}", stats.summary());
// Hot: 1234567 accts (12.3 GB) | Warm: 89012345 accts (890 GB) | Cold: 300000000 accts (2.4 TB) | Hit rate: 97.3%
```

## State Rent Expiry

### Overview

Unlike Solana where rent-exempt accounts live forever for free, TRv1 introduces meaningful storage costs. Accounts that are inactive for extended periods are archived to reduce active state size.

### StateRentConfig

| Parameter | Default | Description |
|-----------|---------|-------------|
| `lamports_per_byte_year` | 3,480 | Annual rent per byte |
| `archive_after_days` | 365 | Inactivity threshold |
| `allow_revival` | true | Whether archived accounts can be restored |
| `exempt_system_programs` | true | Skip stake/vote/system program accounts |

### Eligibility Criteria

An account is eligible for archival when ALL of these are true:
1. Inactive for more than `archive_after_days` (no writes; reads don't count)
2. Data length ≥ 128 bytes (tiny accounts aren't worth archiving)
3. Non-zero lamport balance (zero-lamport accounts are handled by normal GC)
4. Not owned by an exempt system program (if `exempt_system_programs` is true)

### Revival Mechanism

```
User sends ReviveAccount instruction:
  - account_pubkey: the archived account
  - rent_deposit: lamports to cover ≥2 years of rent

Validator processes:
  1. Look up ArchivedAccount in ArchiveIndex
  2. Load serialized data from cold storage
  3. Verify SHA-256 hash matches archived hash
  4. Verify Merkle proof (if available)
  5. Restore AccountSharedData to accounts DB
  6. Remove from ArchiveIndex
  7. Account is now active with deposited rent
```

### Merkle Proofs

Each archived account includes a Merkle proof demonstrating it existed in the accounts hash tree at a specific slot. This enables:
- **Trustless verification**: Anyone can verify the account's existence without trusting the archiver
- **Light client support**: SPV-style proofs for archived account state
- **Dispute resolution**: Proof that an account was archived with specific data

## Hardware Requirements

### Comparison

| Resource | Solana (Cloudbreak) | TRv1 (Tiered Storage) |
|----------|--------------------|-----------------------|
| RAM | 512 GB | 32 GB (min) / 64 GB (recommended) |
| NVMe SSD | 2 TB | 2-4 TB |
| HDD | - | 4+ TB (cold storage) |
| CPU | 24+ cores | 24+ cores |
| Network | 10 Gbps | 10 Gbps |

### Cost Impact

| Component | Solana Monthly | TRv1 Monthly | Savings |
|-----------|---------------|-------------|---------|
| RAM | ~$800 (512 GB) | ~$100 (64 GB) | 87% |
| Storage | ~$200 (2 TB NVMe) | ~$300 (2 TB NVMe + 4 TB HDD) | -50% |
| **Total** | **~$1,000** | **~$400** | **60%** |

*Estimates based on typical cloud provider pricing (2024)*

### Recommended Configurations

**Entry-level validator (32 GB RAM):**
```
RAM: 32 GB DDR5
NVMe: 2 TB PCIe 4.0
HDD: 4 TB (cold storage)
Hot cache: 24 GiB
```

**Performance validator (64 GB RAM):**
```
RAM: 64 GB DDR5
NVMe: 4 TB PCIe 5.0
HDD: 8 TB or S3-compatible object storage
Hot cache: 48 GiB
```

## Comparison with Solana's Cloudbreak

| Aspect | Cloudbreak | TRv1 Tiered Storage |
|--------|-----------|---------------------|
| **Storage model** | Memory-mapped AppendVecs | Three-tier (RAM → SSD → Archive) |
| **RAM requirement** | All accounts in RAM | Only hot accounts in RAM |
| **Account eviction** | None (all accounts permanent) | LRU/LFU/ARC eviction to SSD |
| **State growth** | Unbounded RAM growth | Bounded by cache size |
| **Inactive accounts** | Same cost as active | Archived after 1 year |
| **Account recovery** | N/A | Revival with rent deposit |
| **Read latency (hot)** | ~100ns (mmap) | ~100ns (HashMap) |
| **Read latency (warm)** | N/A | ~100μs (NVMe SSD) |
| **Read latency (cold)** | N/A | ~10ms (HDD) |
| **Validator entry cost** | Very high (~$1000/mo) | Lower (~$400/mo) |
| **Decentralization** | Limited by cost | More accessible |

## Implementation Status

### Phase 1 (Current) ✅
- [x] `TieredStorageConfig` — Configuration and constants
- [x] `AccountCache` — LRU cache with eviction support
- [x] `StateRentExpiry` — Archival and revival logic
- [x] Unit tests for all modules
- [x] Architecture documentation

### Phase 2 (Next)
- [ ] Wire `AccountCache` into `AccountsDb::do_load` path
- [ ] Background eviction thread
- [ ] Warm storage serialization/deserialization
- [ ] Integration with `ReadOnlyAccountsCache`
- [ ] Metrics and monitoring (Prometheus/Grafana)

### Phase 3
- [ ] Full ARC eviction policy with ghost lists
- [ ] Sharded cache for concurrent access (DashMap-style)
- [ ] Cold storage with RocksDB or similar persistent KV store
- [ ] Merkle proof generation integrated with accounts hash
- [ ] ReviveAccount instruction in the runtime
- [ ] Genesis config for tiered storage parameters

### Phase 4
- [ ] State rent collection during epoch boundaries
- [ ] Archival batch processing in AccountsBackgroundService
- [ ] Cold storage compaction and garbage collection
- [ ] Cross-validator cold storage replication
- [ ] Light client archive proof verification

## Source Files

| File | Description |
|------|-------------|
| `accounts-db/src/tiered_storage_config.rs` | Configuration, constants, tier stats, eviction policies |
| `accounts-db/src/account_cache.rs` | Hot-tier LRU cache implementation |
| `accounts-db/src/state_rent_expiry.rs` | Archival, revival, Merkle proofs, rent calculation |
| `docs/tiered-storage.md` | This document |

# TRv1 Benchmarking & Performance Testing Guide

## Overview

TRv1 includes a comprehensive performance testing infrastructure organized into three layers:

1. **Criterion Benchmarks** (`benches/trv1-bench/`) — micro-benchmarks with statistical analysis
2. **Stress Tests** (`tests/stress/`) — integration-level scenario tests
3. **Monitoring** (`monitoring/`) — runtime metrics with Prometheus export

---

## 1. Running Benchmarks

### Prerequisites

```bash
source "$HOME/.cargo/env"
```

### Run All Benchmarks

```bash
cargo bench -p trv1-bench
```

### Run a Specific Benchmark Group

```bash
cargo bench -p trv1-bench --bench consensus_bench
cargo bench -p trv1-bench --bench fee_market_bench
cargo bench -p trv1-bench --bench cache_bench
cargo bench -p trv1-bench --bench staking_bench
cargo bench -p trv1-bench --bench rent_bench
```

### Run with Specific Filters

Criterion supports filtering by benchmark name:

```bash
cargo bench -p trv1-bench --bench consensus_bench -- "propose_commit"
cargo bench -p trv1-bench --bench fee_market_bench -- "multi_block"
```

### View HTML Reports

After running benchmarks, Criterion generates HTML reports:

```
target/criterion/<group_name>/report/index.html
```

Open these in a browser to see statistical analysis, violin plots, and regression detection.

---

## 2. Benchmark Groups

### 2.1 Consensus BFT (`consensus_bench`)

Tests the Tendermint-style BFT consensus engine performance.

| Benchmark | What it measures | Target |
|-----------|-----------------|--------|
| `propose_commit_cycle` | Full propose → prevotes → precommits → commit | < 100ms for 100 validators |
| `proposal_processing` | Single proposal evaluation | < 1ms |
| `prevote_throughput` | Processing all prevotes for N validators | > 10k prevotes/sec |
| `precommit_throughput` | Processing all precommits for N validators | > 10k precommits/sec |
| `validator_set_creation` | Creating a new weighted validator set | < 10ms for 200 validators |
| `round_trip_simulated_latency` | Full cycle with batched message delivery | < 200ms for 100 validators |

**Validator sizes tested:** 50, 100, 200

### 2.2 Fee Market (`fee_market_bench`)

Tests the EIP-1559 dynamic fee market implementation.

| Benchmark | What it measures | Target |
|-----------|-----------------|--------|
| `base_fee_calc` | Single base fee calculation | < 100ns |
| `tx_fee_calc` | Transaction fee computation | < 50ns |
| `validation` | Full fee validation with config checks | < 200ns |
| `multi_block_simulation` | Fee adjustment over 100–10k blocks | < 10ms for 1k blocks |
| `sustained_congestion` | Fee behaviour under 100% utilization | Monotonic increase |

### 2.3 Account Cache (`cache_bench`)

Tests the LRU account cache that forms the "hot" tier of TRv1's tiered storage.

| Benchmark | What it measures | Target |
|-----------|-----------------|--------|
| `insert` | Inserting 10k–100k accounts | < 50ms for 100k |
| `lookup` | Looking up cached accounts | < 100ns per lookup |
| `hit_miss_ratio` | Realistic 80/20 access patterns | > 70% hit rate at 50% fill |
| `eviction_throughput` | Eviction when cache is 10% of working set | Measure baseline |
| `varying_data_sizes` | Performance with 128B–10KB accounts | Scales linearly |

**Cache sizes tested:** Configured via entry count to simulate 1GB, 4GB memory budgets.

### 2.4 Passive Staking (`staking_bench`)

Tests reward calculation and epoch transitions for passive stakers.

| Benchmark | What it measures | Target |
|-----------|-----------------|--------|
| `reward_calculation` | Computing rewards for N accounts | < 1μs per account |
| `epoch_transition` | Full epoch transition (10k–1M stakes) | < 500ms for 100k stakes |
| `multi_epoch_transition` | Multiple consecutive epoch transitions | Linear scaling |
| `tier_distribution` | Realistic tier distribution (40/25/15/10/7/3%) | Baseline |

### 2.5 State Rent (`rent_bench`)

Tests the archive/revival mechanism for rent-expired accounts.

| Benchmark | What it measures | Target |
|-----------|-----------------|--------|
| `archive_throughput` | Hot → cold account migration | > 10k accounts/sec |
| `revival_throughput` | Cold → hot account restoration | > 5k accounts/sec |
| `merkle_proof_generation` | Generating inclusion proofs | < 1μs per proof |
| `merkle_tree_construction` | Building Merkle tree from leaves | < 100ms for 100k leaves |
| `merkle_proof_verification` | Verifying a single Merkle proof | < 1μs |

---

## 3. Stress Tests

Stress tests are standalone integration tests that exercise the system under extreme conditions.

### Run All Stress Tests

```bash
cargo test --test high_tx_throughput -- --nocapture
cargo test --test validator_churn -- --nocapture
cargo test --test state_growth -- --nocapture
cargo test --test fee_spike -- --nocapture
cargo test --test epoch_transition -- --nocapture
```

### 3.1 High TX Throughput (`high_tx_throughput`)

Simulates maximum transaction throughput with a realistic mix:
- 60% simple transfers, 20% token transfers, 15% smart contracts, 5% DeFi swaps
- Validates block processing stays under 1 second
- Tests fee market response to varying load

### 3.2 Validator Churn (`validator_churn`)

Simulates rapid validator set changes:
- Validators joining, leaving, and being jailed every epoch
- Verifies the network maintains ≥ 2/3 active validators (safety threshold)
- Tests rapid join/leave of 10,000 validators

### 3.3 State Growth (`state_growth`)

Simulates rapid account creation to stress tiered storage:
- Creates 500k accounts and forces tier transitions
- Tests hot → warm → cold demotion
- Validates rent collection archives stale accounts

### 3.4 Fee Spike (`fee_spike`)

Simulates sudden demand spikes (e.g., NFT mint):
- Quiet → ramp-up → spike → sustained → cool-down → recovery phases
- Validates fee rises ≥ 2x during spike
- Tests fee oscillation stability under alternating full/empty blocks
- Verifies fee ceiling and floor bounds

### 3.5 Epoch Transition (`epoch_transition`)

Full epoch lifecycle with all subsystems:
- Block production simulation for 432k slots
- Fee distribution: 50% burn / 25% treasury / 10% dev / 15% validators
- Passive staking reward distribution (100k–1M positions)
- Validator set rotation (active ↔ standby ↔ jailed)

---

## 4. Performance Targets

### Design Goals

| Metric | Target | Rationale |
|--------|--------|-----------|
| Block time | 1 second | User experience parity with traditional fintech |
| Finality | ≤ 6 seconds | Deterministic (not probabilistic like Solana) |
| Hardware (RAM) | 32 GB | Low barrier for validators |
| Hot cache hit rate | > 80% | With 80/20 access pattern |
| Epoch transition | < 5 seconds | For 100k passive stakes |
| Fee adjustment | Per block | Real-time market response |
| Validator set | 100–200 | Balance between decentralization and performance |

### Per-Operation Budgets (1-second block target)

Within each 1-second block, the validator must:

```
┌─────────────────────────────────────┬──────────┐
│ Operation                           │ Budget   │
├─────────────────────────────────────┼──────────┤
│ Receive & deserialize proposal      │   10ms   │
│ Validate transactions               │  200ms   │
│ Execute transactions (SVM)          │  500ms   │
│ Compute state root (Merkle)         │  100ms   │
│ Sign & broadcast prevote            │   10ms   │
│ Collect & verify prevotes           │   50ms   │
│ Sign & broadcast precommit          │   10ms   │
│ Collect & verify precommits         │   50ms   │
│ Commit block & update state         │   50ms   │
│ Buffer                              │   20ms   │
├─────────────────────────────────────┼──────────┤
│ TOTAL                               │ 1000ms   │
└─────────────────────────────────────┴──────────┘
```

---

## 5. Monitoring Setup

### Using TRv1 Metrics in Your Validator

```rust
use trv1_monitoring::{TRv1Metrics, prometheus};

// Initialize metrics (typically once at startup)
let metrics = TRv1Metrics::new();

// Record events throughout your code
metrics.blocks_produced.inc();
metrics.consensus_rounds.observe(1.0);
metrics.finality_time_ms.observe(1200.0);
metrics.current_base_fee.set(5_000);
metrics.cache_hit_rate.set(8500); // 85.00% in basis points
```

### Exposing Prometheus Endpoint

```rust
use trv1_monitoring::{TRv1Metrics, prometheus};

// In your HTTP handler for GET /metrics:
fn metrics_handler(metrics: &TRv1Metrics) -> String {
    let snapshot = metrics.snapshot();
    prometheus::encode(&snapshot)
}
```

### Prometheus Configuration

Add to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'trv1-validator'
    scrape_interval: 5s
    static_configs:
      - targets: ['localhost:8899']
        labels:
          chain: 'trv1'
          role: 'validator'
```

### Available Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `trv1_blocks_produced_total` | Counter | Total blocks produced |
| `trv1_consensus_rounds` | Histogram | Rounds needed per block |
| `trv1_finality_time_ms` | Histogram | Proposal-to-commit latency |
| `trv1_missed_proposals_total` | Counter | Missed block proposals |
| `trv1_current_base_fee` | Gauge | Current EIP-1559 base fee |
| `trv1_block_utilization_bps` | Gauge | Block utilization (bps) |
| `trv1_fees_burned_total` | Counter | Total burned fees |
| `trv1_fees_treasury_total` | Counter | Total treasury fees |
| `trv1_fees_dev_total` | Counter | Total developer fees |
| `trv1_fees_validator_total` | Counter | Total validator fees |
| `trv1_hot_cache_size_bytes` | Gauge | Hot cache size |
| `trv1_warm_storage_size_bytes` | Gauge | Warm storage size |
| `trv1_cold_storage_size_bytes` | Gauge | Cold storage size |
| `trv1_cache_hit_rate_bps` | Gauge | Cache hit rate (bps) |
| `trv1_cache_evictions_total` | Counter | Total cache evictions |
| `trv1_total_staked_lamports` | Gauge | Total validator stake |
| `trv1_staking_participation_rate_bps` | Gauge | Staking participation |
| `trv1_active_validators` | Gauge | Active validator count |
| `trv1_standby_validators` | Gauge | Standby validator count |
| `trv1_jailed_validators` | Gauge | Jailed validator count |
| `trv1_passive_stake_total_lamports` | Gauge | Total passive stake |
| `trv1_passive_stake_tier_*_lamports` | Gauge | Per-tier passive stake |

### Grafana Dashboard

Import the following alert rules for operational monitoring:

```
# Critical: Finality > 10 seconds
trv1_finality_time_ms{quantile="0.99"} > 10000

# Warning: Cache hit rate below 70%
trv1_cache_hit_rate_bps < 7000

# Warning: Missed proposals increasing
rate(trv1_missed_proposals_total[5m]) > 0.1

# Info: Base fee rising rapidly
delta(trv1_current_base_fee[1m]) > 1000000

# Critical: Active validators below safety threshold (2/3)
trv1_active_validators < (trv1_active_validators + trv1_standby_validators + trv1_jailed_validators) * 2 / 3
```

---

## 6. Interpreting Results

### Criterion Output

Criterion prints results like:

```
consensus/propose_commit_cycle/validators/100
                        time:   [84.521 µs 85.102 µs 85.730 µs]
                        change: [-0.5% +0.2% +0.9%] (p = 0.58 > 0.05)
                        No change in performance detected.
```

- **time**: [lower bound, estimate, upper bound] of mean iteration time
- **change**: comparison to previous run (if baseline exists)
- **thrpt**: throughput in elements/second (when configured)

### What to Watch For

1. **Regressions**: If `change` shows significant increase, investigate the cause
2. **Outliers**: High variance suggests non-deterministic behaviour (GC, allocation)
3. **Scaling**: Compare results across validator counts (50 → 100 → 200) — should scale sub-linearly
4. **Memory**: Watch for allocation-heavy benchmarks that may pressure the 32GB target

### Comparing Runs

Criterion stores baselines in `target/criterion/`:

```bash
# Save a baseline
cargo bench -p trv1-bench -- --save-baseline v1.0

# Compare against baseline
cargo bench -p trv1-bench -- --baseline v1.0
```

---

## 7. CI Integration

Add to your CI pipeline:

```yaml
benchmark:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - name: Run benchmarks
      run: |
        cargo bench -p trv1-bench -- --output-format bencher | tee bench-output.txt
    - name: Run stress tests
      run: |
        cargo test --test high_tx_throughput -- --nocapture
        cargo test --test fee_spike -- --nocapture
        cargo test --test epoch_transition -- --nocapture
```

For benchmark tracking over time, use [criterion-compare-action](https://github.com/boa-dev/criterion-compare-action) or similar.

//! Passive Staking benchmarks.
//!
//! Measures:
//! - Reward calculation throughput for N accounts
//! - Epoch transition with 10k, 100k, 1M passive stakes

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use solana_pubkey::Pubkey;

// ---------------------------------------------------------------------------
// Types mirroring TRv1 passive staking (simplified for benchmarking)
// ---------------------------------------------------------------------------

/// Tier reward rates in basis points (of validator staking rate).
const REWARD_RATES_BPS: [u64; 6] = [500, 1_000, 2_000, 3_000, 5_000, 12_000];
const _LOCK_TIERS: [u64; 6] = [0, 30, 90, 180, 360, u64::MAX];
const BPS_DENOMINATOR: u64 = 10_000;

/// Assumed validator staking APY in basis points (5%).
const VALIDATOR_APY_BPS: u64 = 500;

/// Assumed slots per epoch.
const SLOTS_PER_EPOCH: u64 = 432_000;

/// Assumed slots per year (at 1-second blocks).
const SLOTS_PER_YEAR: u64 = 31_536_000;

/// A simulated passive stake position.
#[derive(Clone)]
struct StakePosition {
    _authority: Pubkey,
    amount: u64,
    tier_index: usize,
    unclaimed_rewards: u64,
    last_reward_epoch: u64,
}

impl StakePosition {
    fn new(amount: u64, tier_index: usize) -> Self {
        Self {
            _authority: Pubkey::new_unique(),
            amount,
            tier_index: tier_index % 6,
            unclaimed_rewards: 0,
            last_reward_epoch: 0,
        }
    }
}

/// Calculate rewards for a single position for one epoch.
#[inline]
fn calculate_epoch_reward(position: &StakePosition) -> u64 {
    // reward = amount × (validator_apy × tier_rate / BPS²) × (epoch_slots / year_slots)
    // Simplified to avoid floating point:
    // reward = amount × validator_apy_bps × tier_rate_bps / (BPS_DENOM² × SLOTS_PER_YEAR / SLOTS_PER_EPOCH)
    let rate_bps = REWARD_RATES_BPS[position.tier_index];

    // amount × validator_apy × tier_rate × slots_per_epoch
    // ÷ (BPS_DENOM × BPS_DENOM × slots_per_year)
    let numerator = (position.amount as u128)
        .saturating_mul(VALIDATOR_APY_BPS as u128)
        .saturating_mul(rate_bps as u128)
        .saturating_mul(SLOTS_PER_EPOCH as u128);
    let denominator = (BPS_DENOMINATOR as u128)
        .saturating_mul(BPS_DENOMINATOR as u128)
        .saturating_mul(SLOTS_PER_YEAR as u128);

    if denominator == 0 {
        return 0;
    }

    (numerator / denominator) as u64
}

/// Process epoch transition for all positions.
fn process_epoch_transition(positions: &mut [StakePosition], current_epoch: u64) -> u64 {
    let mut total_rewards = 0u64;

    for pos in positions.iter_mut() {
        if pos.last_reward_epoch < current_epoch {
            let epochs_elapsed = current_epoch.saturating_sub(pos.last_reward_epoch);
            for _ in 0..epochs_elapsed {
                let reward = calculate_epoch_reward(pos);
                pos.unclaimed_rewards = pos.unclaimed_rewards.saturating_add(reward);
                total_rewards = total_rewards.saturating_add(reward);
            }
            pos.last_reward_epoch = current_epoch;
        }
    }

    total_rewards
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_reward_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("staking/reward_calculation");

    for &n_accounts in &[1_000u64, 10_000, 100_000] {
        group.throughput(Throughput::Elements(n_accounts));
        group.bench_with_input(
            BenchmarkId::new("accounts", n_accounts),
            &n_accounts,
            |b, &n| {
                let positions: Vec<StakePosition> = (0..n as usize)
                    .map(|i| StakePosition::new(1_000_000_000, i % 6)) // 1 SOL each
                    .collect();

                b.iter(|| {
                    let mut total = 0u64;
                    for pos in &positions {
                        total = total.saturating_add(calculate_epoch_reward(pos));
                    }
                    total
                });
            },
        );
    }
    group.finish();
}

fn bench_epoch_transition(c: &mut Criterion) {
    let mut group = c.benchmark_group("staking/epoch_transition");
    group.sample_size(10);

    for &n_accounts in &[10_000u64, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(n_accounts));
        group.bench_with_input(
            BenchmarkId::new("passive_stakes", n_accounts),
            &n_accounts,
            |b, &n| {
                b.iter_batched(
                    || {
                        // Setup: create fresh positions at epoch 0
                        (0..n as usize)
                            .map(|i| StakePosition::new(1_000_000_000, i % 6))
                            .collect::<Vec<_>>()
                    },
                    |mut positions| {
                        // Transition from epoch 0 to epoch 1
                        process_epoch_transition(&mut positions, 1)
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_multi_epoch_transition(c: &mut Criterion) {
    let mut group = c.benchmark_group("staking/multi_epoch_transition");
    group.sample_size(10);

    let n_accounts = 100_000u64;
    for &n_epochs in &[1u64, 5, 10] {
        group.throughput(Throughput::Elements(n_accounts * n_epochs));
        group.bench_with_input(
            BenchmarkId::new("epochs", n_epochs),
            &n_epochs,
            |b, &n_ep| {
                b.iter_batched(
                    || {
                        (0..n_accounts as usize)
                            .map(|i| StakePosition::new(1_000_000_000, i % 6))
                            .collect::<Vec<_>>()
                    },
                    |mut positions| {
                        let mut total = 0u64;
                        for epoch in 1..=n_ep {
                            total = total.saturating_add(
                                process_epoch_transition(&mut positions, epoch),
                            );
                        }
                        total
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_tier_distribution(c: &mut Criterion) {
    let mut group = c.benchmark_group("staking/tier_distribution");

    // Benchmark with realistic tier distribution:
    // 40% no-lock, 25% 30-day, 15% 90-day, 10% 180-day, 7% 360-day, 3% permanent
    let distribution = [40usize, 25, 15, 10, 7, 3];

    let n_accounts = 100_000usize;
    group.throughput(Throughput::Elements(n_accounts as u64));
    group.bench_function("realistic_distribution", |b| {
        let mut positions = Vec::with_capacity(n_accounts);
        for (tier, &pct) in distribution.iter().enumerate() {
            let count = n_accounts * pct / 100;
            for _ in 0..count {
                positions.push(StakePosition::new(1_000_000_000, tier));
            }
        }
        // Fill remainder
        while positions.len() < n_accounts {
            positions.push(StakePosition::new(1_000_000_000, 0));
        }

        b.iter_batched(
            || positions.clone(),
            |mut pos| process_epoch_transition(&mut pos, 1),
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_reward_calculation,
    bench_epoch_transition,
    bench_multi_epoch_transition,
    bench_tier_distribution,
);
criterion_main!(benches);

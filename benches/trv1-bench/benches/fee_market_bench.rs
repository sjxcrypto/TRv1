//! Fee Market benchmarks.
//!
//! Measures:
//! - Base fee calculation throughput
//! - Fee validation throughput
//! - Multi-block fee adjustment simulation

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use trv1_fee_market::{
    calculator,
    FeeMarketConfig,
    BlockFeeState,
};

// ---------------------------------------------------------------------------
// Base fee calculation
// ---------------------------------------------------------------------------

fn bench_base_fee_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("fee_market/base_fee_calc");

    let config = FeeMarketConfig::default();
    let target = config.target_gas();

    // Scenario 1: block exactly at target
    let state_at_target = BlockFeeState {
        base_fee_per_cu: config.min_base_fee,
        parent_gas_used: target,
        current_gas_used: 0,
        height: 1,
    };

    // Scenario 2: block above target (congested)
    let state_above = BlockFeeState {
        base_fee_per_cu: config.min_base_fee,
        parent_gas_used: target.saturating_mul(3) / 2, // 150% utilization
        current_gas_used: 0,
        height: 1,
    };

    // Scenario 3: block below target (underutilized)
    let state_below = BlockFeeState {
        base_fee_per_cu: 100_000,
        parent_gas_used: target / 4, // 25% utilization
        current_gas_used: 0,
        height: 1,
    };

    group.throughput(Throughput::Elements(1));
    group.bench_function("at_target", |b| {
        b.iter(|| calculator::calculate_next_base_fee(&config, &state_at_target))
    });

    group.bench_function("above_target_150pct", |b| {
        b.iter(|| calculator::calculate_next_base_fee(&config, &state_above))
    });

    group.bench_function("below_target_25pct", |b| {
        b.iter(|| calculator::calculate_next_base_fee(&config, &state_below))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Transaction fee calculation
// ---------------------------------------------------------------------------

fn bench_transaction_fee_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("fee_market/tx_fee_calc");
    group.throughput(Throughput::Elements(1));

    group.bench_function("simple", |b| {
        b.iter(|| {
            calculator::calculate_transaction_fee(
                5_000,    // base_fee_per_cu
                100,      // priority_fee_per_cu
                200_000,  // compute_units
            )
        })
    });

    group.bench_function("high_cu", |b| {
        b.iter(|| {
            calculator::calculate_transaction_fee(
                50_000,      // base_fee_per_cu
                10_000,      // priority_fee_per_cu
                1_400_000,   // max CU per tx
            )
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Fee validation
// ---------------------------------------------------------------------------

fn bench_fee_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("fee_market/validation");
    let config = FeeMarketConfig::default();
    group.throughput(Throughput::Elements(1));

    // Valid transaction
    group.bench_function("valid_tx", |b| {
        b.iter(|| {
            calculator::validate_transaction_fee(
                10_000_000_000, // offered_lamports (10 SOL)
                100,            // priority_fee_per_cu
                5_000,          // base_fee_per_cu
                200_000,        // requested_cu
                &config,
            )
        })
    });

    // Insufficient fee (expect Err)
    group.bench_function("insufficient_fee", |b| {
        b.iter(|| {
            let _ = calculator::validate_transaction_fee(
                1,          // too little
                100,
                5_000,
                200_000,
                &config,
            );
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Multi-block simulation
// ---------------------------------------------------------------------------

fn bench_multi_block_fee_adjustment(c: &mut Criterion) {
    let mut group = c.benchmark_group("fee_market/multi_block_simulation");

    for &n_blocks in &[100u64, 1_000, 10_000] {
        group.throughput(Throughput::Elements(n_blocks));
        group.bench_with_input(
            BenchmarkId::new("blocks", n_blocks),
            &n_blocks,
            |b, &n| {
                let config = FeeMarketConfig::default();

                b.iter(|| {
                    let mut state = BlockFeeState::genesis(config.min_base_fee);

                    for height in 1..=n {
                        // Simulate alternating congestion: odd blocks are busy, even are light
                        let gas_used = if height % 2 == 1 {
                            config.target_gas().saturating_mul(3) / 2
                        } else {
                            config.target_gas() / 3
                        };

                        state.current_gas_used = gas_used;

                        let next_base_fee = calculator::calculate_next_base_fee(&config, &state);
                        state = state.next_block(next_base_fee, height);
                    }

                    state
                });
            },
        );
    }
    group.finish();
}

fn bench_sustained_congestion(c: &mut Criterion) {
    let mut group = c.benchmark_group("fee_market/sustained_congestion");
    group.sample_size(20);

    for &n_blocks in &[100u64, 1_000] {
        group.throughput(Throughput::Elements(n_blocks));
        group.bench_with_input(
            BenchmarkId::new("blocks", n_blocks),
            &n_blocks,
            |b, &n| {
                let config = FeeMarketConfig::default();

                b.iter(|| {
                    let mut state = BlockFeeState::genesis(config.min_base_fee);

                    // Sustained full-block congestion
                    for height in 1..=n {
                        state.current_gas_used = config.max_block_compute_units;
                        let next_base_fee = calculator::calculate_next_base_fee(&config, &state);
                        state = state.next_block(next_base_fee, height);
                    }

                    state
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_base_fee_calculation,
    bench_transaction_fee_calculation,
    bench_fee_validation,
    bench_multi_block_fee_adjustment,
    bench_sustained_congestion,
);
criterion_main!(benches);

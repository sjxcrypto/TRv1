//! Fuzz the EIP-1559 fee calculator with random and extreme inputs.
//!
//! Goals:
//! - Find panics, overflows, underflows, or division-by-zero.
//! - Verify that output is always within [min_base_fee, max_base_fee].
//! - Verify that transaction fee calculations never panic.
//! - Verify monotonicity: higher utilization → higher next base fee.

#![no_main]

use {
    arbitrary::{Arbitrary, Unstructured},
    libfuzzer_sys::fuzz_target,
    trv1_fee_market::{
        calculator::{
            calculate_next_base_fee, calculate_transaction_fee, validate_config,
            validate_transaction_fee,
        },
        config::FeeMarketConfig,
        state::BlockFeeState,
    },
};

/// Fuzz input: random fee market parameters and usage.
#[derive(Debug)]
struct FuzzInput {
    // Config
    min_base_fee: u64,
    max_base_fee: u64,
    target_utilization_pct: u8,
    max_block_compute_units: u64,
    base_fee_change_denominator: u64,
    min_priority_fee: u64,

    // State
    base_fee_per_cu: u64,
    parent_gas_used: u64,

    // Transaction
    priority_fee_per_cu: u64,
    compute_units_used: u64,
    offered_lamports: u64,

    // Multi-block sequence length
    sequence_len: u8,
}

impl<'a> Arbitrary<'a> for FuzzInput {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(FuzzInput {
            min_base_fee: u.arbitrary()?,
            max_base_fee: u.arbitrary()?,
            target_utilization_pct: u.int_in_range(0..=100)?,
            max_block_compute_units: u.arbitrary()?,
            base_fee_change_denominator: u.arbitrary()?,
            min_priority_fee: u.arbitrary()?,
            base_fee_per_cu: u.arbitrary()?,
            parent_gas_used: u.arbitrary()?,
            priority_fee_per_cu: u.arbitrary()?,
            compute_units_used: u.arbitrary()?,
            offered_lamports: u.arbitrary()?,
            sequence_len: u.int_in_range(1..=50)?,
        })
    }
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let input: FuzzInput = match u.arbitrary() {
        Ok(i) => i,
        Err(_) => return,
    };

    // ── Test 1: calculate_next_base_fee must not panic ──

    let config = FeeMarketConfig {
        min_base_fee: input.min_base_fee,
        max_base_fee: input.max_base_fee,
        target_utilization_pct: input.target_utilization_pct,
        max_block_compute_units: input.max_block_compute_units,
        base_fee_change_denominator: input.base_fee_change_denominator,
        min_priority_fee: input.min_priority_fee,
    };

    let state = BlockFeeState {
        base_fee_per_cu: input.base_fee_per_cu,
        parent_gas_used: input.parent_gas_used,
        current_gas_used: 0,
        height: 0,
    };

    // Must not panic regardless of inputs.
    let next_fee = calculate_next_base_fee(&config, &state);

    // ── Invariant: output is clamped to [min, max] when min <= max ──
    if config.min_base_fee <= config.max_base_fee {
        assert!(
            next_fee >= config.min_base_fee,
            "next_fee ({next_fee}) < min_base_fee ({})",
            config.min_base_fee
        );
        assert!(
            next_fee <= config.max_base_fee,
            "next_fee ({next_fee}) > max_base_fee ({})",
            config.max_base_fee
        );
    }

    // ── Test 2: calculate_transaction_fee must not panic ──

    let tx_fee = calculate_transaction_fee(
        input.base_fee_per_cu,
        input.priority_fee_per_cu,
        input.compute_units_used,
    );

    // Invariant: total_fee >= base_fee (since priority >= 0, both components >= 0).
    assert!(tx_fee.total_fee >= tx_fee.base_fee);
    assert!(tx_fee.total_fee >= tx_fee.priority_fee);

    // Invariant: total_fee == base_fee + priority_fee (with saturation).
    let expected_total = tx_fee.base_fee.saturating_add(tx_fee.priority_fee);
    assert_eq!(tx_fee.total_fee, expected_total);

    // ── Test 3: validate_transaction_fee must not panic ──

    // Use a valid config for validation (ensure min <= max and denominator > 0).
    let valid_config = FeeMarketConfig {
        min_base_fee: input.min_base_fee.min(input.max_base_fee),
        max_base_fee: input.min_base_fee.max(input.max_base_fee),
        target_utilization_pct: input.target_utilization_pct,
        max_block_compute_units: input.max_block_compute_units.max(1),
        base_fee_change_denominator: input.base_fee_change_denominator.max(1),
        min_priority_fee: input.min_priority_fee,
    };

    // Must not panic.
    let _ = validate_transaction_fee(
        input.offered_lamports,
        input.priority_fee_per_cu,
        input.base_fee_per_cu,
        input.compute_units_used,
        &valid_config,
    );

    // ── Test 4: validate_config must not panic ──
    let _ = validate_config(&config);

    // ── Test 5: Multi-block sequence stability ──
    // Run a sequence of blocks and verify the base fee stays within bounds.
    if config.min_base_fee <= config.max_base_fee
        && config.base_fee_change_denominator > 0
        && config.target_utilization_pct <= 100
    {
        let mut current_state = state;
        for i in 0..input.sequence_len as u64 {
            let next = calculate_next_base_fee(&config, &current_state);
            assert!(
                next >= config.min_base_fee && next <= config.max_base_fee,
                "Block {i}: fee {next} out of bounds [{}, {}]",
                config.min_base_fee,
                config.max_base_fee
            );
            current_state = BlockFeeState {
                base_fee_per_cu: next,
                parent_gas_used: input.parent_gas_used, // same utilization pattern
                current_gas_used: 0,
                height: i + 1,
            };
        }
    }

    // ── Test 6: Monotonicity check ──
    // For a valid config: higher parent_gas_used should result in higher or equal next_fee.
    if config.min_base_fee <= config.max_base_fee
        && config.base_fee_change_denominator > 0
        && config.target_utilization_pct <= 100
        && config.max_block_compute_units > 0
    {
        let state_low = BlockFeeState {
            base_fee_per_cu: input.base_fee_per_cu,
            parent_gas_used: input.parent_gas_used.min(input.parent_gas_used.wrapping_add(1000)),
            current_gas_used: 0,
            height: 0,
        };
        let state_high = BlockFeeState {
            base_fee_per_cu: input.base_fee_per_cu,
            parent_gas_used: input.parent_gas_used.max(input.parent_gas_used.wrapping_add(1000)),
            current_gas_used: 0,
            height: 0,
        };

        let fee_low = calculate_next_base_fee(&config, &state_low);
        let fee_high = calculate_next_base_fee(&config, &state_high);

        // Higher utilization should not produce a lower fee (monotonicity).
        assert!(
            fee_high >= fee_low,
            "Monotonicity violation: usage {} → fee {}, usage {} → fee {}",
            state_low.parent_gas_used,
            fee_low,
            state_high.parent_gas_used,
            fee_high
        );
    }
});

//! Comprehensive tests for the TRv1 EIP-1559 fee market.

use crate::{
    calculator::{
        calculate_next_base_fee, calculate_transaction_fee, validate_config,
        validate_transaction_fee,
    },
    config::FeeMarketConfig,
    error::FeeError,
    state::BlockFeeState,
};

// ---------------------------------------------------------------------------
// Helper: default config short-hand
// ---------------------------------------------------------------------------

fn cfg() -> FeeMarketConfig {
    FeeMarketConfig::default()
}

/// Create a state where the parent used `parent_cu` compute units.
fn state_with_parent(base_fee: u64, parent_cu: u64, height: u64) -> BlockFeeState {
    BlockFeeState {
        base_fee_per_cu: base_fee,
        parent_gas_used: parent_cu,
        current_gas_used: 0,
        height,
    }
}

// ===========================================================================
// 1. Base fee increases when blocks are full
// ===========================================================================

#[test]
fn base_fee_increases_when_block_is_full() {
    let config = cfg();
    let state = state_with_parent(5_000, 48_000_000, 0); // 100 % full
    let next = calculate_next_base_fee(&config, &state);
    assert!(
        next > 5_000,
        "base fee must increase when block is 100 % full, got {next}"
    );
}

#[test]
fn base_fee_increases_when_block_is_above_target() {
    let config = cfg();
    // 75 % utilization (above the 50 % target)
    let state = state_with_parent(10_000, 36_000_000, 1);
    let next = calculate_next_base_fee(&config, &state);
    assert!(
        next > 10_000,
        "base fee must increase above target, got {next}"
    );
}

#[test]
fn base_fee_increases_with_100pct_full_exact_value() {
    // 100 % utilization: parent_gas_used = 48 M, target = 24 M
    // excess = 48 M - 24 M = 24 M
    // delta = 5000 * 24_000_000 / 24_000_000 / 8 = 5000 / 8 = 625
    // next = 5000 + 625 = 5625
    let config = cfg();
    let state = state_with_parent(5_000, 48_000_000, 0);
    let next = calculate_next_base_fee(&config, &state);
    assert_eq!(next, 5_625, "exact EIP-1559 value for 100 % block");
}

// ===========================================================================
// 2. Base fee decreases when blocks are empty
// ===========================================================================

#[test]
fn base_fee_decreases_when_block_is_empty() {
    let config = cfg();
    let state = state_with_parent(10_000, 0, 1); // 0 % utilization
    let next = calculate_next_base_fee(&config, &state);
    assert!(
        next < 10_000,
        "base fee must decrease when block is empty, got {next}"
    );
}

#[test]
fn base_fee_decreases_when_block_is_below_target() {
    let config = cfg();
    // 25 % utilization (below the 50 % target)
    let state = state_with_parent(10_000, 12_000_000, 1);
    let next = calculate_next_base_fee(&config, &state);
    assert!(
        next < 10_000,
        "base fee must decrease below target, got {next}"
    );
}

#[test]
fn base_fee_decreases_empty_block_exact_value() {
    // 0 % utilization: deficit = 24 M
    // delta = 10_000 * 24_000_000 / 24_000_000 / 8 = 10_000 / 8 = 1250
    // next = 10_000 - 1250 = 8750
    let config = cfg();
    let state = state_with_parent(10_000, 0, 1);
    let next = calculate_next_base_fee(&config, &state);
    assert_eq!(next, 8_750, "exact EIP-1559 value for empty block");
}

// ===========================================================================
// 3. Base fee doesn't go below minimum
// ===========================================================================

#[test]
fn base_fee_clamped_at_minimum() {
    let config = cfg(); // min = 5_000
    // Start at minimum, empty parent — should stay at minimum.
    let state = state_with_parent(5_000, 0, 1);
    let next = calculate_next_base_fee(&config, &state);
    assert_eq!(
        next, 5_000,
        "base fee must not drop below min_base_fee"
    );
}

#[test]
fn base_fee_does_not_go_below_min_even_with_many_empty_blocks() {
    let config = cfg();
    let mut state = state_with_parent(6_000, 0, 0);
    for i in 1..200 {
        let next = calculate_next_base_fee(&config, &state);
        assert!(next >= config.min_base_fee, "block {i}: fee {next} < min");
        state = state_with_parent(next, 0, i);
    }
    assert_eq!(
        state.base_fee_per_cu, config.min_base_fee,
        "should converge to min after many empty blocks"
    );
}

// ===========================================================================
// 4. Base fee doesn't go above maximum
// ===========================================================================

#[test]
fn base_fee_clamped_at_maximum() {
    let config = cfg(); // max = 50_000_000
    // Start at max, full parent — should stay at max.
    let state = state_with_parent(50_000_000, 48_000_000, 1);
    let next = calculate_next_base_fee(&config, &state);
    assert_eq!(
        next, 50_000_000,
        "base fee must not exceed max_base_fee"
    );
}

#[test]
fn base_fee_does_not_go_above_max_even_with_many_full_blocks() {
    let config = cfg();
    let mut state = state_with_parent(40_000_000, 48_000_000, 0);
    for i in 1..200 {
        let next = calculate_next_base_fee(&config, &state);
        assert!(next <= config.max_base_fee, "block {i}: fee {next} > max");
        state = state_with_parent(next, 48_000_000, i);
    }
    assert_eq!(
        state.base_fee_per_cu, config.max_base_fee,
        "should converge to max after many full blocks"
    );
}

// ===========================================================================
// 5. Exact EIP-1559 calculation matches known values
// ===========================================================================

#[test]
fn exact_value_at_target() {
    // Exactly at target (50 %) — no change.
    let config = cfg();
    let state = state_with_parent(10_000, 24_000_000, 1);
    let next = calculate_next_base_fee(&config, &state);
    assert_eq!(next, 10_000, "no change at exactly 50 % target");
}

#[test]
fn exact_value_75pct() {
    // 75 % utilization: parent = 36 M, target = 24 M
    // excess = 12 M
    // delta = 10_000 * 12_000_000 / 24_000_000 / 8 = 10_000 * 0.5 / 8 = 625
    // next = 10_000 + 625 = 10_625
    let config = cfg();
    let state = state_with_parent(10_000, 36_000_000, 1);
    let next = calculate_next_base_fee(&config, &state);
    assert_eq!(next, 10_625, "exact value at 75 % utilization");
}

#[test]
fn exact_value_25pct() {
    // 25 % utilization: parent = 12 M, target = 24 M
    // deficit = 12 M
    // delta = 10_000 * 12_000_000 / 24_000_000 / 8 = 625
    // next = 10_000 - 625 = 9_375
    let config = cfg();
    let state = state_with_parent(10_000, 12_000_000, 1);
    let next = calculate_next_base_fee(&config, &state);
    assert_eq!(next, 9_375, "exact value at 25 % utilization");
}

#[test]
fn exact_value_just_above_target() {
    // Parent used 24_000_001 CU (1 CU above target).
    // excess = 1
    // delta = 10_000 * 1 / 24_000_000 / 8 = 10000 / 192_000_000 ≈ 0 → clamped to 1
    // next = 10_001
    let config = cfg();
    let state = state_with_parent(10_000, 24_000_001, 1);
    let next = calculate_next_base_fee(&config, &state);
    assert_eq!(next, 10_001, "min +1 when just barely above target");
}

#[test]
fn exact_value_just_below_target() {
    // Parent used 23_999_999 CU (1 CU below target).
    // deficit = 1
    // delta = 10_000 * 1 / 24_000_000 / 8 = 0 (integer division)
    // next = 10_000 - 0 = 10_000
    let config = cfg();
    let state = state_with_parent(10_000, 23_999_999, 1);
    let next = calculate_next_base_fee(&config, &state);
    assert_eq!(next, 10_000, "no change when deficit rounds to 0");
}

// ===========================================================================
// 6. Multi-block convergence
// ===========================================================================

#[test]
fn sustained_congestion_drives_fee_up_then_stabilises() {
    let config = cfg();
    let mut fee = config.min_base_fee;
    for i in 0..500 {
        let state = state_with_parent(fee, 48_000_000, i); // always 100 % full
        fee = calculate_next_base_fee(&config, &state);
    }
    assert_eq!(fee, config.max_base_fee, "should hit ceiling");
}

#[test]
fn alternating_full_empty_oscillates_around_initial() {
    let config = cfg();
    let initial = 1_000_000u64;
    let mut fee = initial;
    for i in 0..100 {
        let usage = if i % 2 == 0 { 48_000_000 } else { 0 };
        let state = state_with_parent(fee, usage, i);
        fee = calculate_next_base_fee(&config, &state);
    }
    // After many alternating blocks the fee drifts downward because the
    // multiplicative decrease from empty blocks compounds slightly more than
    // the increase from full blocks (standard EIP-1559 asymmetry).  We just
    // verify it stays in a reasonable range and above minimum.
    assert!(
        fee >= config.min_base_fee,
        "fee must stay above minimum, got {fee}"
    );
    assert!(
        fee <= initial * 2,
        "fee should not explode, got {fee}"
    );
}

// ===========================================================================
// 7. Transaction fee calculation
// ===========================================================================

#[test]
fn transaction_fee_basic() {
    let fee = calculate_transaction_fee(5_000, 100, 200_000);
    assert_eq!(fee.base_fee, 5_000 * 200_000);
    assert_eq!(fee.priority_fee, 100 * 200_000);
    assert_eq!(fee.total_fee, fee.base_fee + fee.priority_fee);
}

#[test]
fn transaction_fee_zero_priority() {
    let fee = calculate_transaction_fee(5_000, 0, 200_000);
    assert_eq!(fee.priority_fee, 0);
    assert_eq!(fee.total_fee, fee.base_fee);
}

#[test]
fn transaction_fee_zero_cu() {
    let fee = calculate_transaction_fee(5_000, 100, 0);
    assert_eq!(fee.base_fee, 0);
    assert_eq!(fee.priority_fee, 0);
    assert_eq!(fee.total_fee, 0);
}

#[test]
fn transaction_fee_saturates() {
    let fee = calculate_transaction_fee(u64::MAX, u64::MAX, u64::MAX);
    assert_eq!(fee.total_fee, u64::MAX, "should saturate, not overflow");
}

// ===========================================================================
// 8. Priority fee ordering
// ===========================================================================

#[test]
fn higher_priority_fee_means_higher_total() {
    let low = calculate_transaction_fee(5_000, 10, 200_000);
    let high = calculate_transaction_fee(5_000, 1_000, 200_000);
    assert!(high.total_fee > low.total_fee);
    assert!(high.priority_fee > low.priority_fee);
    // Base fees should be identical.
    assert_eq!(low.base_fee, high.base_fee);
}

#[test]
fn priority_fee_ordering_is_total_ordering() {
    let base = 5_000u64;
    let cu = 200_000u64;
    let priorities = [0u64, 1, 10, 100, 1_000, 10_000];
    let totals: Vec<u64> = priorities
        .iter()
        .map(|&p| calculate_transaction_fee(base, p, cu).total_fee)
        .collect();
    for window in totals.windows(2) {
        assert!(
            window[0] <= window[1],
            "total fees should be monotonically increasing with priority"
        );
    }
}

// ===========================================================================
// 9. Transaction fee validation
// ===========================================================================

#[test]
fn validate_ok() {
    let config = cfg();
    let result = validate_transaction_fee(
        100_000_000_000, // plenty of lamports
        100,             // priority
        5_000,           // base fee
        200_000,         // CU
        &config,
    );
    assert!(result.is_ok());
    let fee = result.unwrap();
    assert_eq!(fee.total_fee, 5_000 * 200_000 + 100 * 200_000);
}

#[test]
fn validate_insufficient_fee() {
    let config = cfg();
    let result = validate_transaction_fee(
        1, // far too little
        0,
        5_000,
        200_000,
        &config,
    );
    assert!(matches!(result, Err(FeeError::InsufficientFee { .. })));
}

#[test]
fn validate_priority_too_low() {
    let config = FeeMarketConfig {
        min_priority_fee: 100,
        ..Default::default()
    };
    let result = validate_transaction_fee(
        100_000_000_000,
        50, // below minimum of 100
        5_000,
        200_000,
        &config,
    );
    assert!(matches!(result, Err(FeeError::PriorityFeeTooLow { .. })));
}

#[test]
fn validate_cu_exceed_max() {
    let config = cfg();
    let result = validate_transaction_fee(
        100_000_000_000,
        0,
        5_000,
        config.max_block_compute_units + 1, // too many CU
        &config,
    );
    assert!(matches!(
        result,
        Err(FeeError::ComputeUnitsExceedMax { .. })
    ));
}

#[test]
fn validate_exact_boundary() {
    let config = cfg();
    let base = 5_000u64;
    let cu = 200_000u64;
    let exact_fee = base * cu; // no priority
    let result = validate_transaction_fee(exact_fee, 0, base, cu, &config);
    assert!(result.is_ok(), "should accept exact amount");
}

#[test]
fn validate_one_lamport_short() {
    let config = cfg();
    let base = 5_000u64;
    let cu = 200_000u64;
    let exact_fee = base * cu;
    let result = validate_transaction_fee(exact_fee - 1, 0, base, cu, &config);
    assert!(
        matches!(result, Err(FeeError::InsufficientFee { .. })),
        "should reject when 1 lamport short"
    );
}

// ===========================================================================
// 10. Config validation
// ===========================================================================

#[test]
fn validate_config_ok() {
    assert!(validate_config(&cfg()).is_ok());
}

#[test]
fn validate_config_min_gt_max() {
    let config = FeeMarketConfig {
        min_base_fee: 100,
        max_base_fee: 50,
        ..Default::default()
    };
    assert!(matches!(
        validate_config(&config),
        Err(FeeError::InvalidConfig { .. })
    ));
}

#[test]
fn validate_config_zero_denominator() {
    let config = FeeMarketConfig {
        base_fee_change_denominator: 0,
        ..Default::default()
    };
    assert!(matches!(
        validate_config(&config),
        Err(FeeError::InvalidConfig { .. })
    ));
}

#[test]
fn validate_config_utilization_over_100() {
    let config = FeeMarketConfig {
        target_utilization_pct: 101,
        ..Default::default()
    };
    assert!(matches!(
        validate_config(&config),
        Err(FeeError::InvalidConfig { .. })
    ));
}

// ===========================================================================
// 11. Edge cases
// ===========================================================================

#[test]
fn edge_case_zero_base_fee_increases_when_congested() {
    // If base fee is somehow 0 and block is above target,
    // the min(delta, 1) rule should push it to at least 1 (then clamped to min).
    let config = FeeMarketConfig {
        min_base_fee: 0,
        ..Default::default()
    };
    let state = state_with_parent(0, 48_000_000, 0);
    let next = calculate_next_base_fee(&config, &state);
    assert!(next >= 1, "should increase from 0 by at least 1");
}

#[test]
fn edge_case_single_cu_in_block() {
    let config = cfg();
    let state = state_with_parent(10_000, 1, 0);
    let next = calculate_next_base_fee(&config, &state);
    // 1 CU is way below target (24 M), so fee should decrease.
    assert!(next < 10_000);
}

#[test]
fn edge_case_very_large_base_fee() {
    let config = FeeMarketConfig {
        max_base_fee: u64::MAX,
        ..Default::default()
    };
    let state = state_with_parent(u64::MAX / 2, 48_000_000, 0);
    let next = calculate_next_base_fee(&config, &state);
    // Should not panic; should saturate.
    assert!(next >= u64::MAX / 2);
}

#[test]
fn edge_case_block_fee_state_next_block() {
    let mut parent = BlockFeeState::genesis(5_000);
    parent.record_gas(30_000_000);
    let child = parent.next_block(5_500, 1);
    assert_eq!(child.base_fee_per_cu, 5_500);
    assert_eq!(child.parent_gas_used, 30_000_000);
    assert_eq!(child.current_gas_used, 0);
    assert_eq!(child.height, 1);
}

#[test]
fn edge_case_target_zero_pct_nonempty() {
    // target = 0 means ANY usage is "above target".
    let config = FeeMarketConfig {
        target_utilization_pct: 0,
        ..Default::default()
    };
    let state = state_with_parent(5_000, 1, 0);
    let next = calculate_next_base_fee(&config, &state);
    assert_eq!(next, config.max_base_fee);
}

#[test]
fn edge_case_target_zero_pct_empty() {
    let config = FeeMarketConfig {
        target_utilization_pct: 0,
        ..Default::default()
    };
    let state = state_with_parent(5_000, 0, 0);
    let next = calculate_next_base_fee(&config, &state);
    assert_eq!(next, 5_000, "no change when target=0 and parent is empty");
}

// ===========================================================================
// 12. Integration-style: multi-block simulation
// ===========================================================================

#[test]
fn simulation_gradual_congestion_then_relief() {
    let config = cfg();
    let mut state = BlockFeeState::genesis(config.min_base_fee);
    let mut fees: Vec<u64> = vec![state.base_fee_per_cu];

    // 20 blocks of increasing load (50 % → 100 %).
    for i in 0..20 {
        let usage = 24_000_000 + (i as u64) * 1_200_000; // 24 M → 48 M
        state.current_gas_used = 0;
        state.record_gas(usage);
        let next = calculate_next_base_fee(&config, &state);
        state = state.next_block(next, state.height + 1);
        fees.push(next);
    }

    let peak = *fees.last().unwrap();

    // 20 blocks of zero load.
    for _ in 0..20 {
        let next = calculate_next_base_fee(&config, &state);
        state = state.next_block(next, state.height + 1);
        fees.push(next);
    }

    let trough = *fees.last().unwrap();
    assert!(peak > config.min_base_fee, "should have risen");
    assert!(trough < peak, "should have fallen after relief");
    // Fee should still be >= min.
    assert!(trough >= config.min_base_fee);
}

#[test]
fn simulation_steady_state_at_target() {
    // If every block uses exactly the target (50 %), fee should remain constant.
    let config = cfg();
    let initial_fee = 100_000;
    let mut state = state_with_parent(initial_fee, 24_000_000, 0);
    for i in 1..100 {
        let next = calculate_next_base_fee(&config, &state);
        assert_eq!(
            next, initial_fee,
            "block {i}: fee should be constant at target"
        );
        state = state_with_parent(next, 24_000_000, i);
    }
}

// ===========================================================================
// 13. Denominator sensitivity
// ===========================================================================

#[test]
fn smaller_denominator_means_faster_adjustment() {
    let config_slow = FeeMarketConfig {
        base_fee_change_denominator: 16,
        ..Default::default()
    };
    let config_fast = FeeMarketConfig {
        base_fee_change_denominator: 4,
        ..Default::default()
    };
    let state = state_with_parent(10_000, 48_000_000, 0);
    let slow = calculate_next_base_fee(&config_slow, &state);
    let fast = calculate_next_base_fee(&config_fast, &state);
    assert!(
        fast > slow,
        "smaller denominator should produce larger increase"
    );
}

// ===========================================================================
// 14. Error Display
// ===========================================================================

#[test]
fn error_messages_are_readable() {
    let err = FeeError::InsufficientFee {
        offered: 100,
        required: 1_000_000,
        base_fee_per_cu: 5_000,
        compute_units: 200,
    };
    let msg = format!("{err}");
    assert!(msg.contains("100"));
    assert!(msg.contains("1000000"));
}

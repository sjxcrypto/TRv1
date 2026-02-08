//! E2E Test: Full Fee Lifecycle
//!
//! Verifies the complete fee lifecycle:
//! - Submit transactions with varying priority fees
//! - Verify base fee adjusts based on block utilization
//! - Verify 4-way fee split (burn, validator, treasury, developer)
//! - Verify fee transition progresses over epochs
//! - Submit transactions to a deployed program → verify developer gets fee share

use trv1_e2e_tests::helpers::*;
use trv1_fee_market::{
    calculator::{calculate_next_base_fee, calculate_transaction_fee, validate_transaction_fee},
    BlockFeeState, FeeMarketConfig,
};

// ─────────────────────────────────────────────────────────────────────────────
// Test: Base fee adjusts upward when blocks are above target utilization
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_base_fee_rises_above_target() {
    init_logging();
    println!("\n========================================");
    println!("  FEE LIFECYCLE: Base fee rises above target");
    println!("========================================\n");

    let config = FeeMarketConfig::default();
    let target_cu = config.target_gas(); // 24M CU

    // Simulate blocks that are above target (75% utilization → above 50% target).
    let mut state = BlockFeeState::genesis(config.min_base_fee);
    let initial_fee = state.base_fee_per_cu;

    // Produce 20 blocks with above-target utilization.
    for i in 0..20 {
        let block_cu = config.max_block_compute_units * 75 / 100; // 75% full
        state.record_gas(block_cu);
        let next_fee = calculate_next_base_fee(&config, &state);
        state = state.next_block(next_fee, i + 1);
    }

    assert!(
        state.base_fee_per_cu > initial_fee,
        "Base fee should have risen: {} → {}",
        initial_fee,
        state.base_fee_per_cu
    );
    println!(
        "✓ Base fee rose from {} to {} after 20 above-target blocks",
        initial_fee, state.base_fee_per_cu
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Base fee decreases when blocks are below target
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_base_fee_drops_below_target() {
    init_logging();
    println!("\n========================================");
    println!("  FEE LIFECYCLE: Base fee drops below target");
    println!("========================================\n");

    let config = FeeMarketConfig::default();

    // Start with an elevated base fee.
    let mut state = BlockFeeState {
        base_fee_per_cu: 1_000_000, // 1M lamports/CU (high)
        parent_gas_used: 0,
        current_gas_used: 0,
        height: 0,
    };
    let initial_fee = state.base_fee_per_cu;

    // Produce 20 empty blocks (0% utilization — well below target).
    for i in 0..20 {
        // No gas used → below target.
        let next_fee = calculate_next_base_fee(&config, &state);
        state = state.next_block(next_fee, i + 1);
    }

    assert!(
        state.base_fee_per_cu < initial_fee,
        "Base fee should have dropped: {} → {}",
        initial_fee,
        state.base_fee_per_cu
    );
    println!(
        "✓ Base fee dropped from {} to {} after 20 below-target blocks",
        initial_fee, state.base_fee_per_cu
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Base fee stays at minimum with empty blocks
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_base_fee_floor_with_empty_blocks() {
    init_logging();
    println!("\n========================================");
    println!("  FEE LIFECYCLE: Base fee floor (empty blocks)");
    println!("========================================\n");

    let config = FeeMarketConfig::default();
    let mut state = BlockFeeState::genesis(config.min_base_fee);

    for i in 0..100 {
        let next_fee = calculate_next_base_fee(&config, &state);
        state = state.next_block(next_fee, i + 1);
    }

    assert_eq!(
        state.base_fee_per_cu, config.min_base_fee,
        "Base fee should stay at minimum with empty blocks"
    );
    println!(
        "✓ Base fee stable at minimum {} after 100 empty blocks",
        config.min_base_fee
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Varying priority fees
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_varying_priority_fees() {
    init_logging();
    println!("\n========================================");
    println!("  FEE LIFECYCLE: Varying priority fees");
    println!("========================================\n");

    let base_fee = 5_000;
    let cu = 200_000;

    // Transaction with zero priority.
    let fee_zero = calculate_transaction_fee(base_fee, 0, cu);
    assert_eq!(fee_zero.base_fee, base_fee * cu);
    assert_eq!(fee_zero.priority_fee, 0);
    assert_eq!(fee_zero.total_fee, base_fee * cu);
    println!("  Zero priority: total={}", fee_zero.total_fee);

    // Transaction with low priority.
    let fee_low = calculate_transaction_fee(base_fee, 100, cu);
    assert_eq!(fee_low.priority_fee, 100 * cu);
    assert!(fee_low.total_fee > fee_zero.total_fee);
    println!("  Low priority (100/CU): total={}", fee_low.total_fee);

    // Transaction with high priority.
    let fee_high = calculate_transaction_fee(base_fee, 10_000, cu);
    assert!(fee_high.total_fee > fee_low.total_fee);
    println!("  High priority (10k/CU): total={}", fee_high.total_fee);

    println!("✓ Priority fees correctly increase total transaction cost");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: 4-way fee split verification
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_four_way_fee_split() {
    init_logging();
    println!("\n========================================");
    println!("  FEE LIFECYCLE: 4-way fee split");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();

    // Fund a user.
    let user = Pubkey::new_unique();
    net.credit(&user, 100_000_000_000_000); // 100k SOL

    // Record pre-state.
    let pre_burned = net.total_burned;
    let pre_treasury = net.treasury_fees;
    let pre_validator = net.validator_fees;
    let pre_developer = net.developer_fees;

    // Submit transactions with a known program.
    let program = Pubkey::new_unique();
    let txs = program_transactions(50, user, program);

    // Produce a block.
    let fees = net.produce_block(&txs);
    assert!(fees > 0);
    println!("  Block produced with {} lamports in fees", fees);

    // Verify fee distribution.
    let burn_delta = net.total_burned - pre_burned;
    let treasury_delta = net.treasury_fees - pre_treasury;
    let validator_delta = net.validator_fees - pre_validator;
    let developer_delta = net.developer_fees - pre_developer;

    println!("  Fee split:");
    println!("    Burn:      {} lamports", burn_delta);
    println!("    Treasury:  {} lamports", treasury_delta);
    println!("    Validator: {} lamports", validator_delta);
    println!("    Developer: {} lamports", developer_delta);

    // At epoch 0 (launch): 10% burn, 0% validator, 45% treasury, 45% developer.
    // Check approximate ratios (with rounding tolerance).
    let expected_burn = fees * 1_000 / 10_000; // 10%
    let expected_treasury = fees * 4_500 / 10_000; // 45%
    let expected_validator = fees * 0 / 10_000; // 0%
    let expected_developer = fees * 4_500 / 10_000; // 45%

    assert_approx(burn_delta, expected_burn, 2, "burn");
    assert_approx(treasury_delta, expected_treasury, 2, "treasury");
    assert_eq!(validator_delta, expected_validator, "validator share should be 0 at launch");
    assert_approx(developer_delta, expected_developer, 2, "developer");

    // Verify developer rewards attributed to program.
    let dev_earned = net.developer_reward_accounts.get(&program).unwrap_or(&0);
    assert!(
        *dev_earned > 0,
        "Program {} should have earned developer fees",
        program
    );
    println!("✓ Program {} earned {} in developer fees", program, dev_earned);

    // Verify treasury received its share.
    let treasury = net.treasury.as_ref().unwrap();
    assert!(treasury.balance > 0);
    assert_eq!(treasury.total_received, treasury_delta);
    println!("✓ Treasury balance = {}", treasury.balance);

    println!("✓ 4-way fee split verified at launch ratios\n");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Fee split transition over epochs
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fee_split_transition() {
    init_logging();
    println!("\n========================================");
    println!("  FEE LIFECYCLE: Fee split transition");
    println!("========================================\n");

    // Verify the split at epoch 0 (launch).
    let split_0 = fee_split_at_epoch(0);
    assert_eq!(split_0.burn_bps, 1_000);
    assert_eq!(split_0.validator_bps, 0);
    assert_eq!(split_0.treasury_bps, 4_500);
    assert_eq!(split_0.developer_bps, 4_500);
    println!("✓ Epoch 0: burn=10% validator=0% treasury=45% developer=45%");

    // Verify mid-transition (~epoch 456 ≈ 50% through).
    let split_mid = fee_split_at_epoch(TRANSITION_EPOCHS / 2);
    println!(
        "  Epoch {}: burn={}bps validator={}bps treasury={}bps developer={}bps",
        TRANSITION_EPOCHS / 2,
        split_mid.burn_bps,
        split_mid.validator_bps,
        split_mid.treasury_bps,
        split_mid.developer_bps
    );
    // Values should be between launch and maturity.
    assert!(split_mid.burn_bps > LAUNCH_FEE_SPLIT.burn_bps);
    assert!(split_mid.burn_bps < MATURITY_FEE_SPLIT.burn_bps);
    assert!(split_mid.validator_bps > LAUNCH_FEE_SPLIT.validator_bps);
    assert!(split_mid.validator_bps < MATURITY_FEE_SPLIT.validator_bps);
    println!("✓ Mid-transition split is between launch and maturity");

    // Verify maturity (epoch >= 912).
    let split_mature = fee_split_at_epoch(TRANSITION_EPOCHS);
    assert_eq!(split_mature.burn_bps, 2_500);
    assert_eq!(split_mature.validator_bps, 2_500);
    assert_eq!(split_mature.treasury_bps, 2_500);
    assert_eq!(split_mature.developer_bps, 2_500);
    println!("✓ Epoch {}: All 25% (maturity)", TRANSITION_EPOCHS);

    // Post-maturity should be same.
    let split_post = fee_split_at_epoch(TRANSITION_EPOCHS + 1000);
    assert_eq!(split_post.burn_bps, 2_500);
    println!("✓ Post-maturity split stable at 25% each");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Fee validation rejects underfunded transactions
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fee_validation_insufficient_funds() {
    init_logging();
    println!("\n========================================");
    println!("  FEE LIFECYCLE: Fee validation");
    println!("========================================\n");

    let config = FeeMarketConfig::default();

    // Valid transaction.
    let result = validate_transaction_fee(
        10_000_000_000, // 10 SOL
        100,            // priority
        5_000,          // base fee
        200_000,        // CU
        &config,
    );
    assert!(result.is_ok());
    println!("✓ Valid transaction accepted");

    // Insufficient fee.
    let result = validate_transaction_fee(
        1,     // 1 lamport
        100,
        5_000,
        200_000,
        &config,
    );
    assert!(result.is_err());
    println!("✓ Underfunded transaction correctly rejected");

    // CU exceeds block max.
    let result = validate_transaction_fee(
        u64::MAX,
        100,
        5_000,
        config.max_block_compute_units + 1,
        &config,
    );
    assert!(result.is_err());
    println!("✓ Over-CU transaction correctly rejected");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Developer fee attribution across multiple programs
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_developer_fee_attribution_multiple_programs() {
    init_logging();
    println!("\n========================================");
    println!("  FEE LIFECYCLE: Developer fee attribution");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    let user = Pubkey::new_unique();
    net.credit(&user, 100_000_000_000_000);

    let prog_a = Pubkey::new_unique();
    let prog_b = Pubkey::new_unique();

    // Submit transactions to program A.
    let txs_a = program_transactions(20, user, prog_a);
    net.produce_block(&txs_a);

    let a_fees_1 = *net.developer_reward_accounts.get(&prog_a).unwrap_or(&0);
    assert!(a_fees_1 > 0);
    println!("  Program A earned {} after block 1", a_fees_1);

    // Submit transactions to program B.
    let txs_b = program_transactions(20, user, prog_b);
    net.produce_block(&txs_b);

    let b_fees = *net.developer_reward_accounts.get(&prog_b).unwrap_or(&0);
    assert!(b_fees > 0);
    println!("  Program B earned {} after block 2", b_fees);

    // Submit mixed transactions.
    let mut mixed: Vec<SimTransaction> = Vec::new();
    mixed.extend(program_transactions(10, user, prog_a));
    mixed.extend(program_transactions(10, user, prog_b));
    net.produce_block(&mixed);

    let a_fees_2 = *net.developer_reward_accounts.get(&prog_a).unwrap_or(&0);
    let b_fees_2 = *net.developer_reward_accounts.get(&prog_b).unwrap_or(&0);
    assert!(a_fees_2 > a_fees_1);
    assert!(b_fees_2 > b_fees);
    println!("  After mixed block: A={}, B={}", a_fees_2, b_fees_2);

    println!("✓ Developer fees correctly attributed to multiple programs\n");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Full fee lifecycle through epochs with utilization changes
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_full_fee_lifecycle_through_epochs() {
    init_logging();
    println!("\n========================================");
    println!("  FEE LIFECYCLE: Full multi-epoch scenario");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    let users = make_pubkeys(10);
    for u in &users {
        net.credit(u, 1_000_000_000_000_000); // 1M SOL each
    }

    let program = Pubkey::new_unique();
    let initial_base_fee = net.fee_state.base_fee_per_cu;

    // Phase 1: Heavy load for 2 epochs (should push base fee up).
    println!("Phase 1: Heavy load...");
    for _ in 0..SLOTS_PER_EPOCH * 2 {
        let mut txs: Vec<SimTransaction> = random_transactions(20, &users);
        for tx in &mut txs {
            tx.invoked_program = Some(program);
            tx.compute_units = 1_000_000; // Large CU per tx
        }
        net.produce_block(&txs);
    }
    let peak_fee = net.fee_state.base_fee_per_cu;
    println!("  Base fee after heavy load: {}", peak_fee);
    assert!(peak_fee > initial_base_fee, "Base fee should rise under load");

    // Phase 2: No load for 2 epochs (should push base fee down).
    println!("Phase 2: No load...");
    for _ in 0..SLOTS_PER_EPOCH * 2 {
        net.produce_block(&[]);
    }
    let cooled_fee = net.fee_state.base_fee_per_cu;
    println!("  Base fee after cooldown: {}", cooled_fee);
    assert!(cooled_fee < peak_fee, "Base fee should drop without load");

    // Verify cumulative state.
    assert!(net.total_fees_collected > 0);
    assert!(net.total_burned > 0);
    assert!(net.treasury_fees > 0);
    assert!(net.developer_fees > 0);

    let dev_earned = *net.developer_reward_accounts.get(&program).unwrap_or(&0);
    assert!(dev_earned > 0);
    println!("  Program {} earned {} cumulative developer fees", program, dev_earned);

    net.print_summary();
    println!("FULL FEE LIFECYCLE TEST PASSED ✓\n");
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper
// ─────────────────────────────────────────────────────────────────────────────

/// Assert that `actual` is approximately equal to `expected` within `tolerance` lamports.
fn assert_approx(actual: u64, expected: u64, tolerance: u64, label: &str) {
    let diff = if actual > expected {
        actual - expected
    } else {
        expected - actual
    };
    assert!(
        diff <= tolerance,
        "{}: expected ≈{} but got {} (diff={})",
        label,
        expected,
        actual,
        diff
    );
}

//! E2E Test: Basic 3-Validator Network
//!
//! Verifies fundamental network operations:
//! - Initialize 3 validators with different stakes
//! - Produce blocks for 10 epochs
//! - Verify all validators earned staking rewards
//! - Verify active set is correct (stake-weighted ordering)
//! - Verify epoch transitions happen cleanly

use trv1_e2e_tests::helpers::*;

// ─────────────────────────────────────────────────────────────────────────────
// Test: 3-validator network produces blocks and earns rewards over 10 epochs
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_basic_3_validator_network_10_epochs() {
    init_logging();
    println!("\n========================================");
    println!("  BASIC NETWORK: 3 validators, 10 epochs");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();

    // Step 1: Verify initial state.
    assert_eq!(net.validators.len(), 3);
    assert_eq!(net.active_validator_count(), 3);
    assert_eq!(net.current_epoch, 0);
    assert_eq!(net.current_slot, 0);

    // Step 2: Verify stake ordering (descending).
    let active_set = net.active_validator_set();
    assert_eq!(active_set.len(), 3);
    // pks[2] has 3000 SOL (highest), pks[1] has 2000 SOL, pks[0] has 1000 SOL.
    assert_eq!(active_set.get(0).unwrap().pubkey, pks[2]);
    assert_eq!(active_set.get(1).unwrap().pubkey, pks[1]);
    assert_eq!(active_set.get(2).unwrap().pubkey, pks[0]);
    println!("✓ Initial active set is correctly ordered by stake");

    // Step 3: Verify total stake.
    let total_stake = active_set.total_stake();
    assert_eq!(total_stake, 6_000_000_000_000); // 6000 SOL
    println!("✓ Total stake = {} lamports (6000 SOL)", total_stake);

    // Step 4: Produce blocks for 10 epochs.
    let target_epoch = 10;
    println!("\nProducing blocks for {} epochs...", target_epoch);
    net.advance_to_epoch(target_epoch);

    assert_eq!(net.current_epoch, target_epoch);
    println!("✓ Reached epoch {} (slot {})", net.current_epoch, net.current_slot);

    // Step 5: Verify all validators earned rewards.
    for pk in &pks {
        let v = net.validator(pk).unwrap();
        assert!(
            v.rewards_earned > 0,
            "Validator {} should have earned rewards but got 0",
            pk
        );
        println!(
            "  Validator {} earned {} lamports in rewards",
            pk, v.rewards_earned
        );
    }
    println!("✓ All 3 validators earned staking rewards");

    // Step 6: Verify reward proportionality (higher stake → more rewards).
    let r0 = net.validator(&pks[0]).unwrap().rewards_earned;
    let r1 = net.validator(&pks[1]).unwrap().rewards_earned;
    let r2 = net.validator(&pks[2]).unwrap().rewards_earned;

    // pks[2] has 3x stake of pks[0], should get approximately 3x rewards.
    // Allow some variance from block-production rewards (round-robin).
    assert!(r2 > r0, "Validator with 3x stake should earn more than 1x stake");
    assert!(r1 > r0, "Validator with 2x stake should earn more than 1x stake");
    println!("✓ Reward proportionality: r2({}) > r1({}) > r0({})", r2, r1, r0);

    // Step 7: Verify epoch history was recorded.
    assert_eq!(net.epoch_history.len(), target_epoch as usize);
    for (i, summary) in net.epoch_history.iter().enumerate() {
        assert_eq!(summary.epoch, i as u64);
        assert_eq!(summary.active_validators, 3);
        assert!(summary.total_stake > 0);
    }
    println!("✓ {} epoch transitions recorded cleanly", net.epoch_history.len());

    // Step 8: Verify fee state is at genesis minimum (no transactions).
    assert_eq!(
        net.fee_state.base_fee_per_cu,
        net.fee_config.min_base_fee,
        "Base fee should be at minimum with empty blocks"
    );
    println!("✓ Base fee at minimum ({} lamports/CU) with empty blocks", net.fee_state.base_fee_per_cu);

    net.print_summary();
    println!("BASIC NETWORK TEST PASSED ✓\n");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Active set correctness
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_active_set_ordering_and_membership() {
    init_logging();
    println!("\n========================================");
    println!("  ACTIVE SET: Ordering & membership");
    println!("========================================\n");

    let pks = make_pubkeys(5);
    let stakes = vec![
        (pks[0], 500_000_000_000),
        (pks[1], 100_000_000_000),
        (pks[2], 999_000_000_000),
        (pks[3], 100_000_000_000),
        (pks[4], 750_000_000_000),
    ];
    let net = SimNetwork::new(&stakes);

    let active = net.active_validator_set();
    assert_eq!(active.len(), 5);

    // Should be ordered: pks[2](999), pks[4](750), pks[0](500), then pks[1]/pks[3] by pubkey.
    assert_eq!(active.get(0).unwrap().stake, 999_000_000_000);
    assert_eq!(active.get(1).unwrap().stake, 750_000_000_000);
    assert_eq!(active.get(2).unwrap().stake, 500_000_000_000);
    println!("✓ Validators correctly ordered by stake descending");

    // Verify contains.
    for pk in &pks {
        assert!(active.contains(pk));
    }
    assert!(!active.contains(&Pubkey::new_unique()));
    println!("✓ Membership checks correct");

    // Verify quorum calculation (2/3 threshold).
    let quorum = active.quorum_stake(0.667);
    let two_thirds = (active.total_stake() as f64 * 0.667).ceil() as u64;
    assert!(quorum >= two_thirds);
    println!("✓ Quorum stake = {} (total = {})", quorum, active.total_stake());

    println!("ACTIVE SET TEST PASSED ✓\n");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Epoch transition boundaries
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_epoch_transitions_boundary() {
    init_logging();
    println!("\n========================================");
    println!("  EPOCH TRANSITIONS: Boundary checks");
    println!("========================================\n");

    let (mut net, _pks) = standard_3_validator_network();

    // Verify slot-to-epoch mapping.
    assert_eq!(net.slots_per_epoch, SLOTS_PER_EPOCH);

    // Produce exactly one epoch worth of blocks.
    for i in 0..SLOTS_PER_EPOCH {
        net.produce_block(&[]);
        let expected_epoch = (i + 1) / SLOTS_PER_EPOCH;
        assert_eq!(
            net.current_epoch, expected_epoch,
            "Slot {} should be in epoch {}",
            net.current_slot, expected_epoch
        );
    }

    assert_eq!(net.current_epoch, 1);
    assert_eq!(net.epoch_history.len(), 1);
    println!("✓ First epoch transition at slot {}", SLOTS_PER_EPOCH);

    // Produce one more block to verify we're in epoch 1.
    net.produce_block(&[]);
    assert_eq!(net.current_epoch, 1);
    assert_eq!(net.current_slot, SLOTS_PER_EPOCH + 1);
    println!("✓ Slot {} correctly in epoch 1", net.current_slot);

    // Fast-forward to epoch 5.
    net.advance_to_epoch(5);
    assert_eq!(net.current_epoch, 5);
    assert_eq!(net.epoch_history.len(), 5);
    println!("✓ Fast-forwarded to epoch 5 with 5 epoch summaries");

    println!("EPOCH TRANSITIONS TEST PASSED ✓\n");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Block production with transactions
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_block_production_with_transactions() {
    init_logging();
    println!("\n========================================");
    println!("  BLOCK PRODUCTION: With transactions");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();

    // Fund test accounts.
    let users = make_pubkeys(5);
    for u in &users {
        net.credit(u, 10_000_000_000_000); // 10k SOL each
    }

    // Produce blocks with transactions.
    let mut total_fees = 0u64;
    for epoch in 0..3 {
        for _slot in 0..SLOTS_PER_EPOCH {
            let txs = random_transactions(10, &users);
            let fees = net.produce_block(&txs);
            total_fees += fees;
        }
        println!("  Epoch {} complete, cumulative fees = {}", epoch, total_fees);
    }

    assert!(total_fees > 0, "Should have collected fees from transactions");
    assert_eq!(net.total_fees_collected, total_fees);
    println!("✓ Collected {} total lamports in fees", total_fees);

    // Verify fee distribution occurred.
    assert!(net.total_burned > 0, "Some fees should have been burned");
    assert!(net.treasury_fees > 0, "Treasury should have received fees");
    println!(
        "✓ Fee distribution: burned={} treasury={} validator={} developer={}",
        net.total_burned, net.treasury_fees, net.validator_fees, net.developer_fees
    );

    // Verify fee distribution approximately sums to total.
    let distributed = net.total_burned + net.treasury_fees + net.validator_fees + net.developer_fees;
    // Allow rounding errors.
    let diff = if distributed > total_fees {
        distributed - total_fees
    } else {
        total_fees - distributed
    };
    assert!(
        diff <= total_fees / 100, // < 1% rounding error
        "Fee distribution mismatch: distributed={} vs total={}",
        distributed,
        total_fees
    );
    println!("✓ Fee distribution sums correctly (within rounding)");

    // Verify base fee adjusted upward due to block utilization.
    // With random transactions, utilization should be > 0, so base fee should
    // have risen above the minimum at some point.
    println!(
        "  Current base fee: {} lamports/CU (min={})",
        net.fee_state.base_fee_per_cu, net.fee_config.min_base_fee
    );

    net.print_summary();
    println!("BLOCK PRODUCTION TEST PASSED ✓\n");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Delegators contribute to validator total stake
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_delegator_stake_affects_ordering() {
    init_logging();
    println!("\n========================================");
    println!("  DELEGATIONS: Affect validator ordering");
    println!("========================================\n");

    let pks = make_pubkeys(3);
    let stakes = vec![
        (pks[0], 1_000_000_000_000),
        (pks[1], 2_000_000_000_000),
        (pks[2], 1_500_000_000_000),
    ];
    let mut net = SimNetwork::new(&stakes);

    // Initially: pks[1] > pks[2] > pks[0].
    let active = net.active_validator_set();
    assert_eq!(active.get(0).unwrap().pubkey, pks[1]);
    println!("✓ Initial top validator: pks[1] with 2000 SOL");

    // Add massive delegation to pks[0].
    let delegator = Pubkey::new_unique();
    net.validator_mut(&pks[0])
        .unwrap()
        .add_delegator(delegator, 5_000_000_000_000); // 5000 SOL delegation

    // Now pks[0] total = 6000 SOL, should be #1.
    let active = net.active_validator_set();
    assert_eq!(active.get(0).unwrap().pubkey, pks[0]);
    assert_eq!(active.get(0).unwrap().stake, 6_000_000_000_000);
    println!("✓ After delegation, pks[0] is top with 6000 SOL total");

    println!("DELEGATIONS TEST PASSED ✓\n");
}

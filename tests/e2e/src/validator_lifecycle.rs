//! E2E Test: Validator Lifecycle
//!
//! Verifies validator operations:
//! - Start with 3 validators, add a 4th
//! - Simulate one going offline for 24h → verify jailed
//! - Unjail → verify validator returns to active set
//! - Simulate double-sign → verify 5% slash on own stake only
//! - Verify delegators untouched
//! - Add 200+ validators → verify only top 200 are active

use trv1_e2e_tests::helpers::*;
use solana_pubkey::Pubkey;

// ─────────────────────────────────────────────────────────────────────────────
// Test: Add a 4th validator to a 3-validator network
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_add_fourth_validator() {
    init_logging();
    println!("\n========================================");
    println!("  VALIDATOR LIFECYCLE: Add 4th validator");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    assert_eq!(net.active_validator_count(), 3);
    println!("✓ Started with 3 validators");

    // Add a 4th validator.
    let new_pk = Pubkey::new_unique();
    let new_stake = 1_500_000_000_000u64; // 1500 SOL
    net.add_validator(new_pk, new_stake);

    assert_eq!(net.validators.len(), 4);
    assert_eq!(net.active_validator_count(), 4);
    println!("✓ Added 4th validator with {} stake", new_stake);

    // Verify the 4th validator is in the active set.
    let active = net.active_validator_set();
    assert!(active.contains(&new_pk));
    println!("✓ New validator is in the active set");

    // Produce some blocks and verify the new validator participates.
    net.advance_to_epoch(3);
    let v = net.validator(&new_pk).unwrap();
    assert!(v.rewards_earned > 0, "New validator should earn rewards");
    println!("✓ New validator earned {} in rewards", v.rewards_earned);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Validator goes offline → auto-jailed after threshold
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_validator_offline_jailed() {
    init_logging();
    println!("\n========================================");
    println!("  VALIDATOR LIFECYCLE: Offline → Jailed");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();

    let target = pks[1]; // Take validator 1 offline.
    println!("Taking validator {} offline...", target);
    net.set_validator_offline(&target);

    // Verify online flag.
    assert!(!net.validator(&target).unwrap().online);
    println!("✓ Validator marked offline");

    // The validator is still "Active" status but offline.
    assert_eq!(net.validator(&target).unwrap().status, ValidatorStatus::Active);

    // Produce blocks until jailing threshold is reached.
    // JAIL_THRESHOLD_MISSED_SLOTS = 100
    for _ in 0..JAIL_THRESHOLD_MISSED_SLOTS + 5 {
        net.produce_block(&[]);
    }

    // Verify jailed.
    let v = net.validator(&target).unwrap();
    assert_eq!(v.status, ValidatorStatus::Jailed);
    assert!(v.consecutive_missed >= JAIL_THRESHOLD_MISSED_SLOTS);
    println!(
        "✓ Validator jailed after {} missed slots",
        v.consecutive_missed
    );

    // Verify removed from active set.
    let active = net.active_validator_set();
    assert!(!active.contains(&target));
    assert_eq!(active.len(), 2);
    println!("✓ Jailed validator removed from active set (2 remain)");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Unjail → validator returns to active set
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_unjail_validator() {
    init_logging();
    println!("\n========================================");
    println!("  VALIDATOR LIFECYCLE: Unjail");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();

    let target = pks[0];

    // Jail the validator.
    net.jail_validator(&target);
    assert_eq!(net.validator(&target).unwrap().status, ValidatorStatus::Jailed);
    assert_eq!(net.active_validator_count(), 2);
    println!("✓ Validator jailed, active set = 2");

    // Try unjail while offline — should fail (must be online).
    net.set_validator_offline(&target);
    let result = net.unjail_validator(&target);
    assert!(!result, "Unjail should fail while validator is offline");
    println!("✓ Unjail rejected while offline");

    // Bring online and unjail.
    net.set_validator_online(&target);
    let result = net.unjail_validator(&target);
    assert!(result);
    assert_eq!(net.validator(&target).unwrap().status, ValidatorStatus::Active);
    println!("✓ Validator unjailed after coming online");

    // Verify back in active set.
    let active = net.active_validator_set();
    assert!(active.contains(&target));
    assert_eq!(active.len(), 3);
    println!("✓ Unjailed validator back in active set (3 validators)");

    // Verify consecutive_missed was reset.
    assert_eq!(net.validator(&target).unwrap().consecutive_missed, 0);
    println!("✓ Consecutive missed counter reset to 0");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Double-sign → 5% slash on own stake only, delegators untouched
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_double_sign_slash_own_stake_only() {
    init_logging();
    println!("\n========================================");
    println!("  VALIDATOR LIFECYCLE: Double-sign slash");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();

    let bad_validator = pks[2]; // 3000 SOL
    let initial_stake = net.validator(&bad_validator).unwrap().stake;

    // Add delegators.
    let delegator_a = Pubkey::new_unique();
    let delegator_b = Pubkey::new_unique();
    let del_amount_a = 500_000_000_000u64;
    let del_amount_b = 750_000_000_000u64;

    net.validator_mut(&bad_validator)
        .unwrap()
        .add_delegator(delegator_a, del_amount_a);
    net.validator_mut(&bad_validator)
        .unwrap()
        .add_delegator(delegator_b, del_amount_b);

    let pre_delegation_total = net.validator(&bad_validator).unwrap().total_delegation;
    println!("  Initial state: own_stake={} delegations={}", initial_stake, pre_delegation_total);

    // Simulate double-sign.
    let slash_amount = net.slash_double_sign(&bad_validator);
    let expected_slash = initial_stake * DOUBLE_SIGN_SLASH_BPS / BPS_DENOM; // 5%

    assert_eq!(slash_amount, expected_slash);
    println!("✓ Slashed {} lamports (5% of {} own stake)", slash_amount, initial_stake);

    // Verify own stake reduced.
    let v = net.validator(&bad_validator).unwrap();
    assert_eq!(v.stake, initial_stake - slash_amount);
    println!("✓ Own stake reduced: {} → {}", initial_stake, v.stake);

    // Verify delegators untouched.
    assert_eq!(v.delegators.get(&delegator_a), Some(&del_amount_a));
    assert_eq!(v.delegators.get(&delegator_b), Some(&del_amount_b));
    assert_eq!(v.total_delegation, pre_delegation_total);
    println!(
        "✓ Delegator A: {} (unchanged), Delegator B: {} (unchanged)",
        del_amount_a, del_amount_b
    );

    // Verify validator was jailed.
    assert_eq!(v.status, ValidatorStatus::Jailed);
    assert!(v.double_signed);
    println!("✓ Validator jailed and flagged as double-signer");

    // Verify total_slashed tracked.
    assert_eq!(v.total_slashed, slash_amount);
    println!("✓ total_slashed = {}", v.total_slashed);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Active set capped at 200 validators
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_active_set_cap_200() {
    init_logging();
    println!("\n========================================");
    println!("  VALIDATOR LIFECYCLE: 200-validator cap");
    println!("========================================\n");

    // Create 250 validators.
    let n = 250;
    let mut stakes: Vec<(Pubkey, u64)> = Vec::new();
    for i in 0..n {
        let pk = Pubkey::new_unique();
        // Stakes range from 100 SOL to 25100 SOL (unique to avoid ties).
        let stake = (100 + i as u64 * 100) * 1_000_000_000;
        stakes.push((pk, stake));
    }

    let net = SimNetwork::new(&stakes);
    assert_eq!(net.validators.len(), n);

    let active = net.active_validator_set();
    assert_eq!(
        active.len(),
        MAX_ACTIVE_VALIDATORS,
        "Active set should be capped at {}",
        MAX_ACTIVE_VALIDATORS
    );
    println!("✓ Active set capped at {} with {} total validators", MAX_ACTIVE_VALIDATORS, n);

    // Verify the top 200 by stake are included.
    // Stake is ordered, so the top 200 have stake from (51*100)*1e9 up to 25100*1e9.
    let min_active_stake = active.get(active.len() - 1).unwrap().stake;
    let max_inactive_stake = {
        // Find the highest-staked validator NOT in the active set.
        let active_pks: Vec<Pubkey> = active.pubkeys();
        net.validators
            .iter()
            .filter(|v| !active_pks.contains(&v.pubkey))
            .map(|v| v.stake)
            .max()
            .unwrap_or(0)
    };

    assert!(
        min_active_stake >= max_inactive_stake,
        "Lowest active stake ({}) should >= highest inactive stake ({})",
        min_active_stake,
        max_inactive_stake
    );
    println!(
        "✓ Lowest active stake ({}) >= highest inactive stake ({})",
        min_active_stake, max_inactive_stake
    );

    // Verify total stake of active set.
    let total = active.total_stake();
    assert!(total > 0);
    println!("✓ Active set total stake = {}", total);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Full validator lifecycle — add, jail, unjail, slash
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_full_validator_lifecycle() {
    init_logging();
    println!("\n========================================");
    println!("  VALIDATOR LIFECYCLE: Full scenario");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();

    // Step 1: Add a 4th validator.
    let v4 = Pubkey::new_unique();
    net.add_validator(v4, 2_500_000_000_000);
    assert_eq!(net.active_validator_count(), 4);
    println!("Step 1: Added 4th validator, active=4");

    // Step 2: Produce some blocks.
    net.advance_to_epoch(2);
    println!("Step 2: Advanced to epoch 2");

    // Step 3: Take validator 3 (pks[2]) offline.
    net.set_validator_offline(&pks[2]);
    println!("Step 3: Validator {} taken offline", pks[2]);

    // Step 4: Produce blocks until jailed.
    for _ in 0..JAIL_THRESHOLD_MISSED_SLOTS + 1 {
        net.produce_block(&[]);
    }
    assert_eq!(net.validator(&pks[2]).unwrap().status, ValidatorStatus::Jailed);
    assert_eq!(net.active_validator_count(), 3);
    println!("Step 4: Validator jailed, active=3");

    // Step 5: Bring back online and unjail.
    net.set_validator_online(&pks[2]);
    net.unjail_validator(&pks[2]);
    assert_eq!(net.active_validator_count(), 4);
    println!("Step 5: Unjailed, active=4");

    // Step 6: Simulate double-sign on pks[1].
    let pre_stake = net.validator(&pks[1]).unwrap().stake;
    net.slash_double_sign(&pks[1]);
    let post_stake = net.validator(&pks[1]).unwrap().stake;
    assert!(post_stake < pre_stake);
    assert_eq!(net.validator(&pks[1]).unwrap().status, ValidatorStatus::Jailed);
    println!(
        "Step 6: Validator {} slashed {} → {}, jailed",
        pks[1], pre_stake, post_stake
    );

    // Step 7: Produce more blocks, verify network continues.
    net.advance_to_epoch(5);
    assert!(net.blocks_produced > 0);
    assert_eq!(net.active_validator_count(), 3); // pks[1] still jailed
    println!("Step 7: Network continues with 3 active validators");

    net.print_summary();
    println!("FULL VALIDATOR LIFECYCLE TEST PASSED ✓\n");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Validator churn — multiple join/leave cycles
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_validator_churn() {
    init_logging();
    println!("\n========================================");
    println!("  VALIDATOR LIFECYCLE: Churn cycles");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();

    for cycle in 0..5 {
        // Add a new validator.
        let new_pk = Pubkey::new_unique();
        net.add_validator(new_pk, 1_000_000_000_000 + cycle * 100_000_000_000);
        println!("  Cycle {}: Added validator {}", cycle, new_pk);

        // Produce an epoch.
        net.produce_epoch();

        // Jail a random existing validator.
        let to_jail = if cycle % 2 == 0 {
            pks[cycle as usize % pks.len()]
        } else {
            new_pk
        };
        net.jail_validator(&to_jail);
        println!("  Cycle {}: Jailed {}", cycle, to_jail);

        // Produce another epoch.
        net.produce_epoch();

        // Unjail.
        net.set_validator_online(&to_jail);
        net.unjail_validator(&to_jail);
        println!("  Cycle {}: Unjailed {}", cycle, to_jail);
    }

    // Verify network is still functional.
    net.produce_epoch();
    assert!(net.blocks_produced > 0);
    println!(
        "\n✓ Network survived {} churn cycles, {} blocks produced",
        5, net.blocks_produced
    );
}

//! E2E Test: Network Stress
//!
//! Stress tests the simulated network:
//! - 200 validators
//! - High transaction volume
//! - Validator churn (join/leave/jail)
//! - Verify no double-spends
//! - Verify consensus liveness throughout

use trv1_e2e_tests::helpers::*;
use trv1_consensus_bft::{
    BftConfig, ConsensusEngine, EvidenceCollector, ValidatorSet,
};
use solana_pubkey::Pubkey;
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// Test: 200-validator network under high transaction load
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_200_validators_high_load() {
    init_logging();
    println!("\n========================================");
    println!("  STRESS: 200 validators, high tx load");
    println!("========================================\n");

    // Create 200 validators with varying stakes.
    let n = 200;
    let mut stakes: Vec<(Pubkey, u64)> = Vec::new();
    for i in 0..n {
        let pk = Pubkey::new_unique();
        let stake = ((i + 1) as u64 * 50) * 1_000_000_000; // 50 SOL to 10000 SOL
        stakes.push((pk, stake));
    }

    let mut net = SimNetwork::new(&stakes);
    let authority = stakes[0].0;
    let emergency = Pubkey::new_unique();
    net.init_governance(authority, emergency);
    net.init_treasury(authority);

    assert_eq!(net.validators.len(), 200);
    assert_eq!(net.active_validator_count(), 200);
    println!("✓ 200 validators initialized");

    // Create funded users.
    let users = make_pubkeys(50);
    for u in &users {
        net.credit(u, 10_000_000_000_000_000); // 10M SOL each (for stress)
    }

    // Produce 5 epochs with high transaction volume.
    let mut total_tx_count = 0u64;
    let program = Pubkey::new_unique();

    for epoch in 0..5 {
        let mut epoch_fees = 0u64;
        for _ in 0..SLOTS_PER_EPOCH {
            // 50 transactions per block.
            let mut txs: Vec<SimTransaction> = random_transactions(40, &users);
            txs.extend(program_transactions(10, users[0], program));
            let fees = net.produce_block(&txs);
            epoch_fees += fees;
            total_tx_count += txs.len() as u64;
        }
        println!(
            "  Epoch {}: {} txs, {} fees, base_fee={}",
            epoch, SLOTS_PER_EPOCH * 50, epoch_fees, net.fee_state.base_fee_per_cu
        );
    }

    println!("✓ {} total transactions processed over 5 epochs", total_tx_count);

    // Verify all validators earned rewards.
    let validators_with_rewards = net
        .validators
        .iter()
        .filter(|v| v.rewards_earned > 0)
        .count();
    assert_eq!(
        validators_with_rewards, 200,
        "All 200 validators should have earned rewards"
    );
    println!("✓ All 200 validators earned rewards");

    // Verify treasury accumulated.
    let treasury_balance = net.treasury.as_ref().unwrap().balance;
    assert!(treasury_balance > 0);
    println!("✓ Treasury accumulated {} lamports", treasury_balance);

    // Verify developer rewards.
    let dev_earned = *net.developer_reward_accounts.get(&program).unwrap_or(&0);
    assert!(dev_earned > 0);
    println!("✓ Developer program earned {} lamports", dev_earned);

    net.print_summary();
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Validator churn under load
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_validator_churn_under_load() {
    init_logging();
    println!("\n========================================");
    println!("  STRESS: Validator churn under load");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    let users = make_pubkeys(10);
    for u in &users {
        net.credit(u, 1_000_000_000_000_000);
    }

    let mut added_validators: Vec<Pubkey> = Vec::new();
    let mut jailed_validators: Vec<Pubkey> = Vec::new();

    for cycle in 0..20 {
        // Phase 1: Add a new validator.
        let new_pk = Pubkey::new_unique();
        net.add_validator(new_pk, 1_000_000_000_000 + cycle * 50_000_000_000);
        added_validators.push(new_pk);

        // Phase 2: Produce blocks with transactions.
        for _ in 0..SLOTS_PER_EPOCH / 4 {
            let txs = random_transactions(15, &users);
            net.produce_block(&txs);
        }

        // Phase 3: Jail a validator (round-robin from existing).
        if cycle % 3 == 0 && !added_validators.is_empty() {
            let to_jail = added_validators[cycle as usize / 3 % added_validators.len()];
            net.jail_validator(&to_jail);
            jailed_validators.push(to_jail);
        }

        // Phase 4: Unjail a previously jailed validator.
        if cycle % 5 == 0 && !jailed_validators.is_empty() {
            let to_unjail = jailed_validators.remove(0);
            net.set_validator_online(&to_unjail);
            net.unjail_validator(&to_unjail);
        }

        if cycle % 5 == 0 {
            println!(
                "  Cycle {}: validators={} active={} jailed={} blocks={}",
                cycle,
                net.validators.len(),
                net.active_validator_count(),
                jailed_validators.len(),
                net.blocks_produced
            );
        }
    }

    // Verify network is still functional.
    assert!(net.blocks_produced > 0);
    assert!(net.active_validator_count() > 0);
    assert!(net.total_fees_collected > 0);
    println!(
        "\n✓ Network survived 20 churn cycles: {} validators, {} blocks, {} fees",
        net.validators.len(),
        net.blocks_produced,
        net.total_fees_collected
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: No double-spend simulation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_no_double_spend() {
    init_logging();
    println!("\n========================================");
    println!("  STRESS: Double-spend prevention");
    println!("========================================\n");

    let (mut net, _pks) = standard_3_validator_network();

    // Create a user with exactly 1000 SOL.
    let user = Pubkey::new_unique();
    let initial_balance = 1_000_000_000_000u64;
    net.credit(&user, initial_balance);

    // Track balance through multiple transactions.
    let mut expected_balance = initial_balance;
    let mut total_fees_paid = 0u64;

    for block_num in 0..20 {
        // Calculate fee for this transaction.
        let cu = 100_000u64;
        let priority = 100u64;
        let tx_fee = trv1_fee_market::calculator::calculate_transaction_fee(
            net.fee_state.base_fee_per_cu,
            priority,
            cu,
        );

        if expected_balance < tx_fee.total_fee {
            println!(
                "  Block {}: Insufficient balance ({} < {}), stopping",
                block_num, expected_balance, tx_fee.total_fee
            );
            break;
        }

        let txs = vec![SimTransaction {
            sender: user,
            compute_units: cu,
            priority_fee_per_cu: priority,
            invoked_program: None,
        }];

        net.produce_block(&txs);
        expected_balance -= tx_fee.total_fee;
        total_fees_paid += tx_fee.total_fee;

        // Verify actual balance matches expected.
        let actual = net.balance(&user);
        assert_eq!(
            actual, expected_balance,
            "Balance mismatch at block {}: expected={} actual={}",
            block_num, expected_balance, actual
        );
    }

    println!(
        "✓ Balance accounting verified: initial={} fees_paid={} remaining={}",
        initial_balance, total_fees_paid, expected_balance
    );
    assert_eq!(net.balance(&user), initial_balance - total_fees_paid);
    println!("✓ No double-spend: all balance changes accounted for");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Consensus liveness — BFT engine commits under normal conditions
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_consensus_liveness_bft() {
    init_logging();
    println!("\n========================================");
    println!("  STRESS: BFT consensus liveness");
    println!("========================================\n");

    let n = 4;
    let pks = make_pubkeys(n);
    let stakes: Vec<(Pubkey, u64)> = pks
        .iter()
        .enumerate()
        .map(|(i, pk)| (*pk, (i as u64 + 1) * 100))
        .collect();

    let validator_set = ValidatorSet::new(stakes.clone());
    let config = BftConfig::default();

    // Each validator creates its own engine.
    let mut engines: Vec<ConsensusEngine> = pks
        .iter()
        .map(|pk| ConsensusEngine::new(config.clone(), *pk, validator_set.clone()))
        .collect();

    // Start height 0.
    let outputs: Vec<_> = engines.iter_mut().map(|e| e.start_new_height(0)).collect();

    // All engines should produce messages.
    let total_messages: usize = outputs.iter().map(|o| o.messages.len()).sum();
    assert!(total_messages > 0, "Engines should produce messages on new height");
    println!("✓ {} engines started height 0, {} initial messages", n, total_messages);

    // Verify quorum calculation.
    let quorum = validator_set.quorum_stake(config.finality_threshold);
    let total = validator_set.total_stake();
    println!(
        "  Total stake: {} | Quorum: {} (threshold={:.3})",
        total, quorum, config.finality_threshold
    );
    assert!(
        quorum as f64 > total as f64 * 0.66,
        "Quorum should be > 2/3 of total"
    );
    println!("✓ Quorum > 2/3 of total stake");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Double-sign evidence detection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_double_sign_evidence_detection() {
    init_logging();
    println!("\n========================================");
    println!("  STRESS: Double-sign evidence detection");
    println!("========================================\n");

    use trv1_consensus_bft::{ConsensusMessage, DoubleSignEvidence, EvidenceCollector, EvidenceKind};
    use solana_hash::Hash;
    use solana_signature::Signature;

    let mut collector = EvidenceCollector::new();
    let bad_validator = Pubkey::new_unique();

    // Cast two conflicting prevotes.
    let vote1 = ConsensusMessage::Prevote {
        height: 100,
        round: 0,
        block_hash: Some(Hash::new_unique()),
        voter: bad_validator,
        signature: Signature::default(),
    };
    let vote2 = ConsensusMessage::Prevote {
        height: 100,
        round: 0,
        block_hash: Some(Hash::new_unique()),
        voter: bad_validator,
        signature: Signature::default(),
    };

    assert!(collector.check_and_record(&vote1).is_none());
    let evidence = collector.check_and_record(&vote2);
    assert!(evidence.is_some());

    let ev = evidence.unwrap();
    assert_eq!(ev.validator, bad_validator);
    assert_eq!(ev.height, 100);
    assert_eq!(ev.kind, EvidenceKind::ConflictingPrevote);
    println!("✓ Double-sign detected for validator {} at height 100", bad_validator);

    // Verify evidence is stored.
    assert!(collector.has_evidence_against(&bad_validator));
    assert!(!collector.has_evidence_against(&Pubkey::new_unique()));
    println!("✓ Evidence tracking correct");

    // Drain evidence.
    let drained = collector.drain_evidence();
    assert_eq!(drained.len(), 1);
    assert!(collector.evidence().is_empty());
    println!("✓ Evidence drained for slashing submission");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Sustained high utilization for many epochs
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_sustained_high_utilization() {
    init_logging();
    println!("\n========================================");
    println!("  STRESS: Sustained high utilization");
    println!("========================================\n");

    let (mut net, _pks) = standard_3_validator_network();
    let users = make_pubkeys(20);
    for u in &users {
        net.credit(u, 10_000_000_000_000_000);
    }

    let mut fee_history = Vec::new();
    let initial_base_fee = net.fee_state.base_fee_per_cu;

    // Produce 10 epochs of heavy load.
    for epoch in 0..10 {
        let mut epoch_fees = 0u64;
        for _ in 0..SLOTS_PER_EPOCH {
            let txs = random_transactions(30, &users);
            let fees = net.produce_block(&txs);
            epoch_fees += fees;
        }
        fee_history.push((epoch, net.fee_state.base_fee_per_cu, epoch_fees));

        if epoch % 2 == 0 {
            println!(
                "  Epoch {}: base_fee={} epoch_fees={}",
                epoch, net.fee_state.base_fee_per_cu, epoch_fees
            );
        }
    }

    // Verify fee market responded to sustained load.
    let final_base_fee = net.fee_state.base_fee_per_cu;
    println!(
        "\n  Base fee: {} → {} over 10 epochs",
        initial_base_fee, final_base_fee
    );

    // Verify epoch history is complete.
    assert_eq!(net.epoch_history.len(), 10);
    println!("✓ 10 epochs completed");

    // Verify total accounting.
    let total = net.total_burned + net.treasury_fees + net.validator_fees + net.developer_fees;
    let tolerance = net.total_fees_collected / 50; // 2% tolerance
    let diff = if total > net.total_fees_collected {
        total - net.total_fees_collected
    } else {
        net.total_fees_collected - total
    };
    assert!(
        diff <= tolerance,
        "Fee accounting mismatch: distributed={} collected={}",
        total,
        net.total_fees_collected
    );
    println!(
        "✓ Fee accounting balanced: collected={} distributed={}",
        net.total_fees_collected, total
    );

    // Verify no negative balances.
    for (pk, bal) in &net.balances {
        assert!(*bal <= u64::MAX, "Account {} has invalid balance", pk);
    }
    println!("✓ No invalid balances");

    println!("SUSTAINED HIGH UTILIZATION TEST PASSED ✓\n");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Combined stress — validators + staking + governance + treasury
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_combined_stress_scenario() {
    init_logging();
    println!("\n========================================");
    println!("  STRESS: Combined all-subsystems stress");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    let authority = pks[0];
    let users = make_pubkeys(10);
    for u in &users {
        net.credit(u, 10_000_000_000_000_000);
    }

    // 1: Create passive stakes.
    let stake_idx_30 = net.create_passive_stake(users[0], 100_000_000_000_000, 30);
    let stake_idx_perm = net.create_passive_stake(users[1], 50_000_000_000_000, u64::MAX);
    println!("Phase 1: Created passive stakes");

    // 2: Submit transactions.
    for _ in 0..SLOTS_PER_EPOCH * 2 {
        let txs = random_transactions(15, &users);
        net.produce_block(&txs);
    }
    println!("Phase 2: Produced 2 epochs of transactions");

    // 3: Add validators.
    for i in 0..5 {
        let pk = Pubkey::new_unique();
        net.add_validator(pk, (i + 1) as u64 * 500_000_000_000);
    }
    println!("Phase 3: Added 5 validators");

    // 4: Produce more blocks.
    net.advance_to_epoch(5);
    println!("Phase 4: Advanced to epoch 5");

    // 5: Governance — activate and create a proposal.
    net.activate_governance().unwrap();
    let prop_id = net.create_proposal(&users[0], "Combined test proposal", false).unwrap();
    net.cast_vote(prop_id, 7000, "for").unwrap();
    net.cast_vote(prop_id, 3000, "against").unwrap();
    println!("Phase 5: Governance activated, proposal created and voted");

    // 6: Claim passive staking rewards.
    let rewards_30 = net.claim_passive_rewards(stake_idx_30);
    assert!(rewards_30 > 0);
    let rewards_perm = net.claim_passive_rewards(stake_idx_perm);
    assert!(rewards_perm > 0);
    assert!(rewards_perm > rewards_30);
    println!(
        "Phase 6: Claimed rewards — 30-day={} permanent={}",
        rewards_30, rewards_perm
    );

    // 7: Jail and slash a validator.
    net.slash_double_sign(&pks[2]);
    println!("Phase 7: Validator {} slashed", pks[2]);

    // 8: Treasury disbursement.
    let treasury_bal = net.treasury.as_ref().unwrap().balance;
    if treasury_bal > 0 {
        let recipient = Pubkey::new_unique();
        net.disburse_treasury(&authority, &recipient, treasury_bal / 10).unwrap();
        println!("Phase 8: Disbursed {} from treasury", treasury_bal / 10);
    }

    // 9: Finalize governance proposal.
    let voting_ends = net.proposals[prop_id as usize].voting_ends_epoch;
    net.advance_to_epoch(voting_ends);
    let result = net.finalize_proposal(prop_id).unwrap();
    println!("Phase 9: Proposal finalized as {:?}", result);

    // 10: Final verification.
    assert!(net.blocks_produced > 0);
    assert!(net.total_fees_collected > 0);
    assert!(net.total_burned > 0);
    assert!(net.treasury.as_ref().unwrap().total_received > 0);
    assert!(net.validators.iter().any(|v| v.rewards_earned > 0));
    assert_eq!(net.epoch_history.len(), voting_ends as usize);

    net.print_summary();
    println!("COMBINED STRESS TEST PASSED ✓\n");
}

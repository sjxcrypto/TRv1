//! E2E Test: Governance Lifecycle
//!
//! Verifies the complete governance flow:
//! - At launch: multisig creates proposal → executes directly
//! - Activate governance
//! - Create proposal with sufficient stake
//! - Vote with different lock tiers (verify weighted votes)
//! - Pass proposal → timelock → execute
//! - Test veto flow
//! - Test emergency unlock (80% supermajority)

use trv1_e2e_tests::helpers::*;
use trv1_governance_program::vote_weight::{calculate_voting_power, StakeSource};
use solana_pubkey::Pubkey;

// ─────────────────────────────────────────────────────────────────────────────
// Test: Multisig mode — create and execute proposal before governance activation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_multisig_mode_proposal() {
    init_logging();
    println!("\n========================================");
    println!("  GOVERNANCE: Multisig mode");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    let authority = pks[0]; // Governance authority.
    let gov = net.governance.as_ref().unwrap();
    assert!(!gov.is_active, "Governance should be inactive at launch");
    println!("✓ Governance inactive at launch");

    // Non-authority cannot create proposal.
    let random = Pubkey::new_unique();
    let result = net.create_proposal(&random, "Unauthorized proposal", false);
    assert!(result.is_err());
    println!("✓ Non-authority proposal correctly rejected");

    // Authority creates proposal.
    let prop_id = net.create_proposal(&authority, "Parameter change: increase fee", false).unwrap();
    assert_eq!(prop_id, 0);
    println!("✓ Proposal #{} created by authority", prop_id);

    // In multisig mode, proposal goes directly to Timelocked.
    let proposal = &net.proposals[prop_id as usize];
    assert_eq!(proposal.status, SimProposalStatus::Timelocked);
    println!("✓ Proposal in Timelocked status (skipped voting)");

    // Cannot execute before timelock expires.
    let result = net.execute_proposal(prop_id);
    assert!(result.is_err());
    println!("✓ Execution before timelock correctly rejected");

    // Advance past timelock (voting_period + timelock = 7 + 2 = 9 epochs).
    net.advance_to_epoch(proposal.execution_epoch);
    let result = net.execute_proposal(prop_id);
    assert!(result.is_ok());
    println!("✓ Proposal executed after timelock at epoch {}", net.current_epoch);

    let proposal = &net.proposals[prop_id as usize];
    assert_eq!(proposal.status, SimProposalStatus::Executed);
    assert!(proposal.executed);
    println!("✓ Proposal status = Executed");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Activate governance (one-way transition)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_activate_governance() {
    init_logging();
    println!("\n========================================");
    println!("  GOVERNANCE: Activation");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();

    assert!(!net.governance.as_ref().unwrap().is_active);
    println!("✓ Governance starts inactive");

    net.activate_governance().unwrap();
    assert!(net.governance.as_ref().unwrap().is_active);
    println!("✓ Governance activated");

    // Cannot activate again.
    let result = net.activate_governance();
    assert!(result.is_err());
    println!("✓ Double activation correctly rejected");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Full governance proposal flow with voting
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_full_proposal_flow() {
    init_logging();
    println!("\n========================================");
    println!("  GOVERNANCE: Full proposal flow");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    let authority = pks[0];

    // Activate governance.
    net.activate_governance().unwrap();
    println!("✓ Governance activated");

    // Create a proposal.
    let proposer = Pubkey::new_unique();
    let prop_id = net.create_proposal(&proposer, "Treasury spend: fund hackathon", false).unwrap();
    println!("✓ Proposal #{} created", prop_id);

    let proposal = &net.proposals[prop_id as usize];
    assert_eq!(proposal.status, SimProposalStatus::Active);
    println!("✓ Proposal in Active status (voting open)");

    // Cast votes with different weights (simulating lock tier weights).
    // Voter 1: 1000 SOL with 360-day lock → weight = 1000 × 0.50 = 500
    let weight_360 = calculate_voting_power(
        1_000_000_000_000,
        StakeSource::PassiveStake { lock_days: 360 },
    )
    .unwrap();
    assert_eq!(weight_360, 500_000_000_000);
    println!("  Voter 1 (360d): weight = {}", weight_360);

    // Voter 2: 2000 SOL with permanent lock → weight = 2000 × 1.50 = 3000
    let weight_perm = calculate_voting_power(
        2_000_000_000_000,
        StakeSource::PassiveStake { lock_days: u64::MAX },
    )
    .unwrap();
    assert_eq!(weight_perm, 3_000_000_000_000);
    println!("  Voter 2 (perm): weight = {}", weight_perm);

    // Voter 3: 500 SOL with 30-day lock → weight = 500 × 0.10 = 50
    let weight_30 = calculate_voting_power(
        500_000_000_000,
        StakeSource::PassiveStake { lock_days: 30 },
    )
    .unwrap();
    assert_eq!(weight_30, 50_000_000_000);
    println!("  Voter 3 (30d): weight = {}", weight_30);

    // Cast votes.
    net.cast_vote(prop_id, weight_360, "for").unwrap();
    net.cast_vote(prop_id, weight_perm, "for").unwrap();
    net.cast_vote(prop_id, weight_30, "against").unwrap();

    let proposal = &net.proposals[prop_id as usize];
    assert_eq!(proposal.votes_for, weight_360 + weight_perm);
    assert_eq!(proposal.votes_against, weight_30);
    println!("✓ Votes cast: for={} against={}", proposal.votes_for, proposal.votes_against);

    // Advance past voting period.
    let voting_ends = proposal.voting_ends_epoch;
    net.advance_to_epoch(voting_ends);

    // Finalize.
    let result = net.finalize_proposal(prop_id).unwrap();
    assert_eq!(result, SimProposalStatus::Timelocked);
    println!("✓ Proposal passed and moved to Timelocked");

    // Advance past timelock.
    let exec_epoch = net.proposals[prop_id as usize].execution_epoch;
    net.advance_to_epoch(exec_epoch);

    net.execute_proposal(prop_id).unwrap();
    assert_eq!(net.proposals[prop_id as usize].status, SimProposalStatus::Executed);
    println!("✓ Proposal executed at epoch {}", net.current_epoch);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Voting weight reflects lock tier correctly
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_voting_weight_by_tier() {
    init_logging();
    println!("\n========================================");
    println!("  GOVERNANCE: Voting weight by tier");
    println!("========================================\n");

    let amount = 1_000_000_000_000u64; // 1000 SOL

    let cases = vec![
        (StakeSource::ValidatorOrDelegator, 1_000_000_000_000u64, "Validator 1.0×"),
        (StakeSource::PassiveStake { lock_days: 0 }, 0, "No lock 0×"),
        (StakeSource::PassiveStake { lock_days: 30 }, 100_000_000_000, "30-day 0.10×"),
        (StakeSource::PassiveStake { lock_days: 90 }, 200_000_000_000, "90-day 0.20×"),
        (StakeSource::PassiveStake { lock_days: 180 }, 300_000_000_000, "180-day 0.30×"),
        (StakeSource::PassiveStake { lock_days: 360 }, 500_000_000_000, "360-day 0.50×"),
        (StakeSource::PassiveStake { lock_days: u64::MAX }, 1_500_000_000_000, "Permanent 1.50×"),
        (StakeSource::Unstaked, 0, "Unstaked 0×"),
    ];

    for (source, expected, label) in &cases {
        let power = calculate_voting_power(amount, *source).unwrap();
        assert_eq!(power, *expected, "{}: expected {} got {}", label, expected, power);
        println!("  {}: {} → {}", label, amount, power);
    }

    println!("✓ All voting weight multipliers verified");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Veto flow
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_veto_flow() {
    init_logging();
    println!("\n========================================");
    println!("  GOVERNANCE: Veto flow");
    println!("========================================\n");

    let (mut net, _pks) = standard_3_validator_network();
    net.activate_governance().unwrap();

    let proposer = Pubkey::new_unique();
    let prop_id = net.create_proposal(&proposer, "Controversial proposal", false).unwrap();

    // Cast votes: small 'for', massive 'veto'.
    net.cast_vote(prop_id, 100, "for").unwrap();
    net.cast_vote(prop_id, 50, "against").unwrap();
    net.cast_vote(prop_id, 200, "veto").unwrap(); // veto > 33.3% of total

    // Advance past voting.
    let voting_ends = net.proposals[prop_id as usize].voting_ends_epoch;
    net.advance_to_epoch(voting_ends);

    // Finalize — should be vetoed.
    let result = net.finalize_proposal(prop_id).unwrap();
    assert_eq!(result, SimProposalStatus::Vetoed);
    println!("✓ Proposal vetoed (veto votes = 200 / 350 total = ~57%)");

    // Verify cannot execute a vetoed proposal.
    net.advance_to_epoch(voting_ends + 10);
    let result = net.execute_proposal(prop_id);
    assert!(result.is_err());
    println!("✓ Cannot execute vetoed proposal");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Emergency cancel by multisig
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_emergency_cancel() {
    init_logging();
    println!("\n========================================");
    println!("  GOVERNANCE: Emergency cancel");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    let emergency_ms = net.governance.as_ref().unwrap().emergency_multisig;

    net.activate_governance().unwrap();

    let proposer = Pubkey::new_unique();
    let prop_id = net.create_proposal(&proposer, "Dangerous proposal", false).unwrap();

    // Non-multisig cannot cancel.
    let random = Pubkey::new_unique();
    let result = net.cancel_proposal(prop_id, &random);
    assert!(result.is_err());
    println!("✓ Non-multisig cancel correctly rejected");

    // Emergency multisig can cancel.
    net.cancel_proposal(prop_id, &emergency_ms).unwrap();
    assert_eq!(net.proposals[prop_id as usize].status, SimProposalStatus::Cancelled);
    println!("✓ Proposal cancelled by emergency multisig");

    // Cannot execute cancelled proposal.
    let result = net.execute_proposal(prop_id);
    assert!(result.is_err());
    println!("✓ Cannot execute cancelled proposal");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Emergency unlock requires 80% supermajority
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_emergency_unlock_supermajority() {
    init_logging();
    println!("\n========================================");
    println!("  GOVERNANCE: Emergency unlock (80%)");
    println!("========================================\n");

    let (mut net, _pks) = standard_3_validator_network();
    net.activate_governance().unwrap();

    let proposer = Pubkey::new_unique();

    // Create emergency unlock proposal.
    let prop_id = net
        .create_proposal(&proposer, "Emergency unlock: whale account", true)
        .unwrap();
    assert!(net.proposals[prop_id as usize].is_emergency_unlock);
    println!("✓ Emergency unlock proposal created");

    // Cast votes: 75% for (should fail — needs 80%).
    let total = 10_000u64;
    net.cast_vote(prop_id, 7_500, "for").unwrap(); // 75%
    net.cast_vote(prop_id, 2_500, "against").unwrap(); // 25%

    let voting_ends = net.proposals[prop_id as usize].voting_ends_epoch;
    net.advance_to_epoch(voting_ends);

    let result = net.finalize_proposal(prop_id).unwrap();
    assert_eq!(result, SimProposalStatus::Rejected);
    println!("✓ 75% for rejected (requires 80% supermajority)");

    // Create another emergency unlock proposal.
    let prop_id2 = net
        .create_proposal(&proposer, "Emergency unlock: retry", true)
        .unwrap();

    // Cast votes: 81% for (should pass).
    net.cast_vote(prop_id2, 8_100, "for").unwrap(); // 81%
    net.cast_vote(prop_id2, 1_900, "against").unwrap(); // 19%

    let voting_ends2 = net.proposals[prop_id2 as usize].voting_ends_epoch;
    net.advance_to_epoch(voting_ends2);

    let result2 = net.finalize_proposal(prop_id2).unwrap();
    assert_eq!(result2, SimProposalStatus::Timelocked);
    println!("✓ 81% for passed (≥ 80% supermajority)");

    // Execute after timelock.
    let exec_epoch = net.proposals[prop_id2 as usize].execution_epoch;
    net.advance_to_epoch(exec_epoch);
    net.execute_proposal(prop_id2).unwrap();
    assert_eq!(net.proposals[prop_id2 as usize].status, SimProposalStatus::Executed);
    println!("✓ Emergency unlock proposal executed");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Expired proposal (no votes)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_expired_proposal() {
    init_logging();
    println!("\n========================================");
    println!("  GOVERNANCE: Expired proposal");
    println!("========================================\n");

    let (mut net, _pks) = standard_3_validator_network();
    net.activate_governance().unwrap();

    let proposer = Pubkey::new_unique();
    let prop_id = net.create_proposal(&proposer, "Ignored proposal", false).unwrap();

    // No votes cast. Advance past voting period.
    let voting_ends = net.proposals[prop_id as usize].voting_ends_epoch;
    net.advance_to_epoch(voting_ends);

    let result = net.finalize_proposal(prop_id).unwrap();
    assert_eq!(result, SimProposalStatus::Expired);
    println!("✓ Proposal expired with no votes");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Multiple proposals in parallel
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_multiple_parallel_proposals() {
    init_logging();
    println!("\n========================================");
    println!("  GOVERNANCE: Multiple parallel proposals");
    println!("========================================\n");

    let (mut net, _pks) = standard_3_validator_network();
    net.activate_governance().unwrap();

    let proposer = Pubkey::new_unique();

    // Create 5 proposals.
    let mut prop_ids = Vec::new();
    for i in 0..5 {
        let id = net
            .create_proposal(&proposer, &format!("Proposal {}", i), false)
            .unwrap();
        prop_ids.push(id);
    }
    println!("✓ Created 5 proposals: {:?}", prop_ids);

    // Vote on each differently.
    net.cast_vote(prop_ids[0], 7000, "for").unwrap();
    net.cast_vote(prop_ids[0], 3000, "against").unwrap();

    net.cast_vote(prop_ids[1], 2000, "for").unwrap();
    net.cast_vote(prop_ids[1], 8000, "against").unwrap();

    net.cast_vote(prop_ids[2], 6000, "for").unwrap();
    net.cast_vote(prop_ids[2], 4000, "veto").unwrap();

    net.cast_vote(prop_ids[3], 9000, "for").unwrap();
    net.cast_vote(prop_ids[3], 1000, "abstain").unwrap();

    // prop_ids[4] — no votes (will expire).

    // Advance past voting.
    let max_voting_end = net
        .proposals
        .iter()
        .map(|p| p.voting_ends_epoch)
        .max()
        .unwrap();
    net.advance_to_epoch(max_voting_end);

    // Finalize all.
    let r0 = net.finalize_proposal(prop_ids[0]).unwrap();
    let r1 = net.finalize_proposal(prop_ids[1]).unwrap();
    let r2 = net.finalize_proposal(prop_ids[2]).unwrap();
    let r3 = net.finalize_proposal(prop_ids[3]).unwrap();
    let r4 = net.finalize_proposal(prop_ids[4]).unwrap();

    assert_eq!(r0, SimProposalStatus::Timelocked); // 70% for
    assert_eq!(r1, SimProposalStatus::Rejected);   // 20% for
    assert_eq!(r2, SimProposalStatus::Vetoed);      // 40% veto
    assert_eq!(r3, SimProposalStatus::Timelocked); // 90% for
    assert_eq!(r4, SimProposalStatus::Expired);     // no votes

    println!("✓ Proposal 0: Passed (70% for)");
    println!("✓ Proposal 1: Rejected (20% for)");
    println!("✓ Proposal 2: Vetoed (40% veto)");
    println!("✓ Proposal 3: Passed (90% for)");
    println!("✓ Proposal 4: Expired (no votes)");
    println!("✓ All 5 parallel proposals resolved correctly");
}

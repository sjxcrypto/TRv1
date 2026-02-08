//! E2E Test: Treasury Lifecycle
//!
//! Verifies treasury operations:
//! - Verify treasury receives fee share each epoch
//! - Disburse from treasury (multisig authorized)
//! - Transfer authority
//! - Verify unauthorized disburse fails

use trv1_e2e_tests::helpers::*;
use solana_pubkey::Pubkey;

// ─────────────────────────────────────────────────────────────────────────────
// Test: Treasury accumulates fee share each epoch
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_treasury_receives_fee_share() {
    init_logging();
    println!("\n========================================");
    println!("  TREASURY: Fee share accumulation");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();

    // Fund users.
    let users = make_pubkeys(5);
    for u in &users {
        net.credit(u, 100_000_000_000_000);
    }

    // Produce blocks with transactions for several epochs.
    let mut epoch_treasury_balances = Vec::new();
    epoch_treasury_balances.push(net.treasury.as_ref().unwrap().balance);

    for epoch in 0..5 {
        for _ in 0..SLOTS_PER_EPOCH {
            let txs = random_transactions(15, &users);
            net.produce_block(&txs);
        }
        let treasury_balance = net.treasury.as_ref().unwrap().balance;
        epoch_treasury_balances.push(treasury_balance);
        println!(
            "  Epoch {}: treasury balance = {}",
            epoch, treasury_balance
        );
    }

    // Verify treasury balance increased each epoch.
    for i in 1..epoch_treasury_balances.len() {
        assert!(
            epoch_treasury_balances[i] > epoch_treasury_balances[i - 1],
            "Treasury should grow each epoch: {} → {}",
            epoch_treasury_balances[i - 1],
            epoch_treasury_balances[i]
        );
    }
    println!("✓ Treasury balance increased every epoch");

    // Verify treasury tracking fields.
    let treasury = net.treasury.as_ref().unwrap();
    assert!(treasury.total_received > 0);
    assert_eq!(treasury.total_disbursed, 0); // No disbursements yet.
    assert_eq!(treasury.balance, treasury.total_received);
    println!(
        "✓ Treasury received={} disbursed=0 balance={}",
        treasury.total_received, treasury.balance
    );

    // Verify treasury_fees matches launch split (45% of total).
    let expected_treasury_share = net.total_fees_collected * 4_500 / 10_000;
    let tolerance = net.total_fees_collected / 100; // 1% tolerance for rounding.
    let diff = if net.treasury_fees > expected_treasury_share {
        net.treasury_fees - expected_treasury_share
    } else {
        expected_treasury_share - net.treasury_fees
    };
    assert!(
        diff <= tolerance,
        "Treasury fees {} ≠ expected {} (45% of {})",
        net.treasury_fees,
        expected_treasury_share,
        net.total_fees_collected
    );
    println!(
        "✓ Treasury received ≈45% of total fees ({} / {})",
        net.treasury_fees, net.total_fees_collected
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Disburse from treasury with proper authorization
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_treasury_disburse_authorized() {
    init_logging();
    println!("\n========================================");
    println!("  TREASURY: Authorized disbursement");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    let authority = pks[0];

    // Fund treasury.
    let users = make_pubkeys(3);
    for u in &users {
        net.credit(u, 100_000_000_000_000);
    }
    for _ in 0..SLOTS_PER_EPOCH * 3 {
        let txs = random_transactions(20, &users);
        net.produce_block(&txs);
    }

    let pre_balance = net.treasury.as_ref().unwrap().balance;
    assert!(pre_balance > 0);
    println!("  Treasury balance before disbursement: {}", pre_balance);

    // Disburse to a recipient.
    let recipient = Pubkey::new_unique();
    let disburse_amount = pre_balance / 4; // Disburse 25%.

    let pre_recipient = net.balance(&recipient);
    net.disburse_treasury(&authority, &recipient, disburse_amount).unwrap();

    let post_balance = net.treasury.as_ref().unwrap().balance;
    assert_eq!(post_balance, pre_balance - disburse_amount);
    println!(
        "  Treasury after disbursement: {} (disbursed {})",
        post_balance, disburse_amount
    );

    // Verify recipient received funds.
    let post_recipient = net.balance(&recipient);
    assert_eq!(post_recipient, pre_recipient + disburse_amount);
    println!(
        "  Recipient balance: {} → {}",
        pre_recipient, post_recipient
    );

    // Verify tracking.
    let treasury = net.treasury.as_ref().unwrap();
    assert_eq!(treasury.total_disbursed, disburse_amount);
    println!("✓ Disbursement successful: {} lamports to {}", disburse_amount, recipient);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Unauthorized disburse fails
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_treasury_disburse_unauthorized() {
    init_logging();
    println!("\n========================================");
    println!("  TREASURY: Unauthorized disbursement");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    let authority = pks[0];

    // Seed some funds into treasury.
    {
        let treasury = net.treasury.as_mut().unwrap();
        treasury.balance = 10_000_000_000_000;
        treasury.total_received = 10_000_000_000_000;
    }

    let recipient = Pubkey::new_unique();
    let unauthorized = Pubkey::new_unique();

    // Try with wrong signer.
    let result = net.disburse_treasury(&unauthorized, &recipient, 1_000_000_000);
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Signer is not the treasury authority");
    println!("✓ Unauthorized signer correctly rejected");

    // Try with a validator that's not the authority.
    let result = net.disburse_treasury(&pks[1], &recipient, 1_000_000_000);
    assert!(result.is_err());
    println!("✓ Non-authority validator correctly rejected");

    // Verify balance unchanged.
    assert_eq!(net.treasury.as_ref().unwrap().balance, 10_000_000_000_000);
    println!("✓ Treasury balance unchanged after failed attempts");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Transfer treasury authority
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_treasury_authority_transfer() {
    init_logging();
    println!("\n========================================");
    println!("  TREASURY: Authority transfer");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    let original_authority = pks[0];
    let new_authority = Pubkey::new_unique();

    // Seed treasury.
    {
        let treasury = net.treasury.as_mut().unwrap();
        treasury.balance = 5_000_000_000_000;
        treasury.total_received = 5_000_000_000_000;
    }

    // Transfer authority.
    net.transfer_treasury_authority(&original_authority, &new_authority).unwrap();
    assert_eq!(net.treasury.as_ref().unwrap().authority, new_authority);
    println!("✓ Authority transferred from {} to {}", original_authority, new_authority);

    // Old authority can no longer disburse.
    let recipient = Pubkey::new_unique();
    let result = net.disburse_treasury(&original_authority, &recipient, 100);
    assert!(result.is_err());
    println!("✓ Old authority cannot disburse after transfer");

    // New authority can disburse.
    net.disburse_treasury(&new_authority, &recipient, 1_000_000_000).unwrap();
    println!("✓ New authority can disburse");

    // Unauthorized cannot transfer authority.
    let random = Pubkey::new_unique();
    let result = net.transfer_treasury_authority(&random, &Pubkey::new_unique());
    assert!(result.is_err());
    println!("✓ Unauthorized authority transfer rejected");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Insufficient funds disbursement
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_treasury_insufficient_funds() {
    init_logging();
    println!("\n========================================");
    println!("  TREASURY: Insufficient funds");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    let authority = pks[0];

    // Treasury starts empty.
    assert_eq!(net.treasury.as_ref().unwrap().balance, 0);

    let recipient = Pubkey::new_unique();
    let result = net.disburse_treasury(&authority, &recipient, 1);
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Insufficient treasury balance");
    println!("✓ Disbursement from empty treasury rejected");

    // Add some funds.
    {
        let treasury = net.treasury.as_mut().unwrap();
        treasury.balance = 1_000;
    }

    // Try to disburse more than available.
    let result = net.disburse_treasury(&authority, &recipient, 1_001);
    assert!(result.is_err());
    println!("✓ Over-disbursement rejected (1001 > 1000)");

    // Disburse exactly the balance.
    net.disburse_treasury(&authority, &recipient, 1_000).unwrap();
    assert_eq!(net.treasury.as_ref().unwrap().balance, 0);
    println!("✓ Exact balance disbursement succeeded, treasury now 0");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Multiple disbursements track correctly
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_treasury_multiple_disbursements() {
    init_logging();
    println!("\n========================================");
    println!("  TREASURY: Multiple disbursements");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    let authority = pks[0];

    // Seed treasury.
    {
        let treasury = net.treasury.as_mut().unwrap();
        treasury.balance = 100_000_000_000_000;
        treasury.total_received = 100_000_000_000_000;
    }

    let recipients = make_pubkeys(5);
    let amounts = [
        10_000_000_000_000u64,
        5_000_000_000_000,
        20_000_000_000_000,
        3_000_000_000_000,
        7_000_000_000_000,
    ];

    let mut total_disbursed = 0u64;
    for (recipient, amount) in recipients.iter().zip(amounts.iter()) {
        net.disburse_treasury(&authority, recipient, *amount).unwrap();
        total_disbursed += amount;

        let balance_received = net.balance(recipient);
        assert_eq!(balance_received, *amount);
        println!(
            "  Disbursed {} to {} (cumulative: {})",
            amount, recipient, total_disbursed
        );
    }

    let treasury = net.treasury.as_ref().unwrap();
    assert_eq!(treasury.total_disbursed, total_disbursed);
    assert_eq!(
        treasury.balance,
        100_000_000_000_000 - total_disbursed
    );
    println!(
        "✓ {} disbursements tracked: total_disbursed={} remaining={}",
        amounts.len(),
        treasury.total_disbursed,
        treasury.balance
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: Full treasury lifecycle
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_full_treasury_lifecycle() {
    init_logging();
    println!("\n========================================");
    println!("  TREASURY: Full lifecycle");
    println!("========================================\n");

    let (mut net, pks) = standard_3_validator_network();
    let authority = pks[0];

    // Step 1: Treasury starts empty.
    assert_eq!(net.treasury.as_ref().unwrap().balance, 0);
    assert!(!net.treasury.as_ref().unwrap().governance_active);
    println!("Step 1: Treasury initialized, empty, governance inactive");

    // Step 2: Accumulate fees over 5 epochs.
    let users = make_pubkeys(5);
    for u in &users {
        net.credit(u, 1_000_000_000_000_000);
    }
    for _ in 0..SLOTS_PER_EPOCH * 5 {
        let txs = random_transactions(10, &users);
        net.produce_block(&txs);
    }
    let accumulated = net.treasury.as_ref().unwrap().balance;
    assert!(accumulated > 0);
    println!("Step 2: Accumulated {} in treasury over 5 epochs", accumulated);

    // Step 3: Disburse to fund a project.
    let project = Pubkey::new_unique();
    let grant = accumulated / 10;
    net.disburse_treasury(&authority, &project, grant).unwrap();
    println!("Step 3: Disbursed {} to project {}", grant, project);

    // Step 4: Transfer authority to a governance PDA.
    let governance_pda = Pubkey::new_unique();
    net.transfer_treasury_authority(&authority, &governance_pda).unwrap();
    println!("Step 4: Authority transferred to governance PDA {}", governance_pda);

    // Step 5: Old authority can no longer disburse.
    let result = net.disburse_treasury(&authority, &project, 100);
    assert!(result.is_err());
    println!("Step 5: Old authority correctly rejected");

    // Step 6: New authority disburses.
    net.disburse_treasury(&governance_pda, &project, grant).unwrap();
    println!("Step 6: New authority disbursed {} to project", grant);

    // Step 7: Continue accumulating.
    for _ in 0..SLOTS_PER_EPOCH * 3 {
        let txs = random_transactions(10, &users);
        net.produce_block(&txs);
    }

    let final_balance = net.treasury.as_ref().unwrap().balance;
    let final_received = net.treasury.as_ref().unwrap().total_received;
    let final_disbursed = net.treasury.as_ref().unwrap().total_disbursed;
    println!(
        "Step 7: Final state: balance={} received={} disbursed={}",
        final_balance, final_received, final_disbursed
    );

    assert_eq!(final_balance, final_received - final_disbursed);
    println!("✓ balance = received - disbursed");

    net.print_summary();
    println!("FULL TREASURY LIFECYCLE TEST PASSED ✓\n");
}

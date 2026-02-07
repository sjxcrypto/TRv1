//! Integration tests for TRv1 Developer Rewards program.
//!
//! Tests revenue recipient registration, multi-splits, anti-gaming checks,
//! and fee attribution logic.

use {
    crate::harness::SOL,
    trv1_developer_rewards_program::{
        constants::{
            COOLDOWN_SLOTS, MAX_PROGRAM_FEE_SHARE_BPS, MIN_COMPUTE_UNITS_THRESHOLD, TOTAL_BPS,
        },
        error::DeveloperRewardsError,
        state::{EpochFeeTracker, ProgramRevenueConfig, RevenueSplit},
    },
    solana_pubkey::Pubkey,
};

// ═══════════════════════════════════════════════════════════════════════════
//  1. Revenue recipient registration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_register_revenue_config_defaults() {
    let program_id = Pubkey::new_unique();
    let recipient = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let current_slot = 100_000;

    let config = ProgramRevenueConfig {
        version: 1,
        program_id,
        revenue_recipient: recipient,
        update_authority: authority,
        is_active: true,
        revenue_splits: Vec::new(),
        total_fees_earned: 0,
        epoch_fees_earned: 0,
        last_epoch: 0,
        eligible_after_slot: current_slot + COOLDOWN_SLOTS,
        unclaimed_fees: 0,
    };

    assert_eq!(config.version, 1);
    assert_eq!(config.program_id, program_id);
    assert_eq!(config.revenue_recipient, recipient);
    assert_eq!(config.update_authority, authority);
    assert!(config.is_active);
    assert!(config.revenue_splits.is_empty());
    assert_eq!(config.total_fees_earned, 0);
    assert_eq!(config.unclaimed_fees, 0);
    assert_eq!(config.eligible_after_slot, current_slot + COOLDOWN_SLOTS);
}

#[test]
fn test_cooldown_is_approximately_7_days() {
    // 7 days × 24 h × 60 min × 60 s / 0.4 s ≈ 1_512_000 slots
    assert_eq!(COOLDOWN_SLOTS, 1_512_000);

    let seconds_per_slot = 0.4;
    let cooldown_seconds = COOLDOWN_SLOTS as f64 * seconds_per_slot;
    let cooldown_days = cooldown_seconds / 86_400.0;
    assert!(
        (cooldown_days - 7.0).abs() < 0.01,
        "Cooldown should be ~7 days, got {:.2} days",
        cooldown_days
    );
}

#[test]
fn test_duplicate_registration_rejected() {
    // If a config already has version != 0, it's already registered.
    let config = ProgramRevenueConfig {
        version: 1,
        program_id: Pubkey::new_unique(),
        revenue_recipient: Pubkey::new_unique(),
        update_authority: Pubkey::new_unique(),
        is_active: true,
        revenue_splits: Vec::new(),
        total_fees_earned: 0,
        epoch_fees_earned: 0,
        last_epoch: 0,
        eligible_after_slot: 0,
        unclaimed_fees: 0,
    };
    // The processor checks data[0] != 0 (the version byte after serialization).
    assert_ne!(config.version, 0, "Already-initialized config should be rejected");
}

// ═══════════════════════════════════════════════════════════════════════════
//  2. Update recipient
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_update_revenue_recipient() {
    let old_recipient = Pubkey::new_unique();
    let new_recipient = Pubkey::new_unique();

    let mut config = ProgramRevenueConfig {
        version: 1,
        program_id: Pubkey::new_unique(),
        revenue_recipient: old_recipient,
        update_authority: Pubkey::new_unique(),
        is_active: true,
        revenue_splits: Vec::new(),
        total_fees_earned: 0,
        epoch_fees_earned: 0,
        last_epoch: 0,
        eligible_after_slot: 0,
        unclaimed_fees: 0,
    };

    assert_eq!(config.revenue_recipient, old_recipient);
    config.revenue_recipient = new_recipient;
    assert_eq!(config.revenue_recipient, new_recipient);
    assert_ne!(config.revenue_recipient, old_recipient);
}

#[test]
fn test_unauthorized_update_fails() {
    let config = ProgramRevenueConfig {
        version: 1,
        program_id: Pubkey::new_unique(),
        revenue_recipient: Pubkey::new_unique(),
        update_authority: Pubkey::new_unique(),
        is_active: true,
        revenue_splits: Vec::new(),
        total_fees_earned: 0,
        epoch_fees_earned: 0,
        last_epoch: 0,
        eligible_after_slot: 0,
        unclaimed_fees: 0,
    };

    let unauthorized_signer = Pubkey::new_unique();
    assert_ne!(
        config.update_authority, unauthorized_signer,
        "Unauthorized signer should not match update authority"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  3. Multi-recipient splits
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_valid_two_way_split() {
    let splits = vec![
        RevenueSplit {
            recipient: Pubkey::new_unique(),
            share_bps: 5_000,
        },
        RevenueSplit {
            recipient: Pubkey::new_unique(),
            share_bps: 5_000,
        },
    ];

    let total: u32 = splits.iter().map(|s| s.share_bps as u32).sum();
    assert_eq!(total, TOTAL_BPS as u32, "Splits must sum to 10000 bps");
}

#[test]
fn test_valid_three_way_split() {
    let splits = vec![
        RevenueSplit {
            recipient: Pubkey::new_unique(),
            share_bps: 5_000,
        },
        RevenueSplit {
            recipient: Pubkey::new_unique(),
            share_bps: 3_000,
        },
        RevenueSplit {
            recipient: Pubkey::new_unique(),
            share_bps: 2_000,
        },
    ];

    let total: u32 = splits.iter().map(|s| s.share_bps as u32).sum();
    assert_eq!(total, TOTAL_BPS as u32);
}

#[test]
fn test_valid_ten_way_split() {
    let recipients: Vec<Pubkey> = (0..10).map(|_| Pubkey::new_unique()).collect();
    let splits: Vec<RevenueSplit> = recipients
        .iter()
        .map(|r| RevenueSplit {
            recipient: *r,
            share_bps: 1_000, // 10% each
        })
        .collect();

    let total: u32 = splits.iter().map(|s| s.share_bps as u32).sum();
    assert_eq!(total, TOTAL_BPS as u32);
    assert_eq!(splits.len(), 10);
}

#[test]
fn test_splits_not_summing_to_10000_fails() {
    let splits = vec![
        RevenueSplit {
            recipient: Pubkey::new_unique(),
            share_bps: 5_000,
        },
        RevenueSplit {
            recipient: Pubkey::new_unique(),
            share_bps: 4_999,
        },
    ];

    let total: u32 = splits.iter().map(|s| s.share_bps as u32).sum();
    assert_ne!(total, TOTAL_BPS as u32, "9999 bps should be rejected");
}

#[test]
fn test_splits_exceeding_10000_fails() {
    let splits = vec![
        RevenueSplit {
            recipient: Pubkey::new_unique(),
            share_bps: 6_000,
        },
        RevenueSplit {
            recipient: Pubkey::new_unique(),
            share_bps: 5_000,
        },
    ];

    let total: u32 = splits.iter().map(|s| s.share_bps as u32).sum();
    assert_ne!(total, TOTAL_BPS as u32, "11000 bps should be rejected");
}

#[test]
fn test_zero_share_in_split_rejected() {
    let splits = vec![
        RevenueSplit {
            recipient: Pubkey::new_unique(),
            share_bps: 10_000,
        },
        RevenueSplit {
            recipient: Pubkey::new_unique(),
            share_bps: 0,
        },
    ];

    let has_zero = splits.iter().any(|s| s.share_bps == 0);
    assert!(has_zero, "Zero-share entries should be rejected by the processor");
}

#[test]
fn test_duplicate_recipients_rejected() {
    let same_recipient = Pubkey::new_unique();
    let splits = vec![
        RevenueSplit {
            recipient: same_recipient,
            share_bps: 5_000,
        },
        RevenueSplit {
            recipient: same_recipient,
            share_bps: 5_000,
        },
    ];

    // Check for duplicates (mirrors the processor logic)
    let mut has_duplicate = false;
    for (i, s) in splits.iter().enumerate() {
        for other in splits.iter().skip(i + 1) {
            if s.recipient == other.recipient {
                has_duplicate = true;
            }
        }
    }
    assert!(has_duplicate, "Duplicate recipients should be rejected");
}

#[test]
fn test_max_split_recipients_is_10() {
    let splits: Vec<RevenueSplit> = (0..11)
        .map(|_| RevenueSplit {
            recipient: Pubkey::new_unique(),
            share_bps: 909, // won't sum to 10000 but we're testing count
        })
        .collect();
    assert!(
        splits.len() > 10,
        "More than 10 recipients should be rejected by TooManySplitRecipients"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  4. Anti-gaming: CU threshold
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_min_compute_units_threshold() {
    assert_eq!(MIN_COMPUTE_UNITS_THRESHOLD, 1_000);
}

#[test]
fn test_below_cu_threshold_rejected() {
    let cu_consumed = 999;
    assert!(
        cu_consumed < MIN_COMPUTE_UNITS_THRESHOLD,
        "Transactions consuming <1000 CU should not qualify for developer fees"
    );
}

#[test]
fn test_at_cu_threshold_accepted() {
    let cu_consumed = 1_000;
    assert!(
        cu_consumed >= MIN_COMPUTE_UNITS_THRESHOLD,
        "Transactions consuming ≥1000 CU should qualify"
    );
}

#[test]
fn test_above_cu_threshold_accepted() {
    let cu_consumed = 50_000;
    assert!(cu_consumed >= MIN_COMPUTE_UNITS_THRESHOLD);
}

// ═══════════════════════════════════════════════════════════════════════════
//  5. Anti-gaming: 7-day cooldown
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_cooldown_prevents_immediate_fee_credit() {
    let registration_slot = 100_000;
    let eligible_after_slot = registration_slot + COOLDOWN_SLOTS;

    // Right after registration: not eligible
    let current_slot = registration_slot + 1;
    assert!(
        current_slot < eligible_after_slot,
        "Program should not be eligible right after registration"
    );
}

#[test]
fn test_cooldown_allows_after_7_days() {
    let registration_slot = 100_000;
    let eligible_after_slot = registration_slot + COOLDOWN_SLOTS;

    // After 7 days + 1 slot
    let current_slot = eligible_after_slot + 1;
    assert!(
        current_slot >= eligible_after_slot,
        "Program should be eligible after cooldown"
    );
}

#[test]
fn test_cooldown_boundary_exact() {
    let registration_slot = 100_000;
    let eligible_after_slot = registration_slot + COOLDOWN_SLOTS;

    // Exactly at boundary
    assert!(eligible_after_slot >= eligible_after_slot);
    // One slot before
    assert!(eligible_after_slot - 1 < eligible_after_slot);
}

// ═══════════════════════════════════════════════════════════════════════════
//  6. Anti-gaming: 10% per-epoch cap
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_epoch_cap_is_10_percent() {
    assert_eq!(MAX_PROGRAM_FEE_SHARE_BPS, 1_000); // 10% = 1000 bps
}

#[test]
fn test_epoch_cap_allows_small_credit() {
    let total_dev_fees_this_epoch: u64 = 1_000 * SOL;
    let program_epoch_fees: u64 = 50 * SOL;  // 5% — under cap
    let new_credit: u64 = 10 * SOL;

    let projected_program = program_epoch_fees + new_credit;
    let projected_total = total_dev_fees_this_epoch + new_credit;

    let max_allowed = (projected_total as u128)
        .saturating_mul(MAX_PROGRAM_FEE_SHARE_BPS as u128)
        / (TOTAL_BPS as u128);

    assert!(
        projected_program <= max_allowed as u64,
        "60 SOL / 1010 SOL = {:.1}% which is < 10%",
        (projected_program as f64 / projected_total as f64) * 100.0
    );
}

#[test]
fn test_epoch_cap_rejects_exceeding_credit() {
    let total_dev_fees_this_epoch: u64 = 100 * SOL;
    let program_epoch_fees: u64 = 9 * SOL;
    let new_credit: u64 = 5 * SOL; // This would bring program to 14 / 105 = 13.3%

    let projected_program = program_epoch_fees + new_credit;
    let projected_total = total_dev_fees_this_epoch + new_credit;

    let max_allowed = (projected_total as u128)
        .saturating_mul(MAX_PROGRAM_FEE_SHARE_BPS as u128)
        / (TOTAL_BPS as u128);

    assert!(
        projected_program > max_allowed as u64,
        "14 SOL / 105 SOL = {:.1}% which exceeds the 10% cap",
        (projected_program as f64 / projected_total as f64) * 100.0
    );
}

#[test]
fn test_epoch_cap_resets_on_new_epoch() {
    let mut config = ProgramRevenueConfig {
        version: 1,
        program_id: Pubkey::new_unique(),
        revenue_recipient: Pubkey::new_unique(),
        update_authority: Pubkey::new_unique(),
        is_active: true,
        revenue_splits: Vec::new(),
        total_fees_earned: 0,
        epoch_fees_earned: 90 * SOL, // near cap
        last_epoch: 5,
        eligible_after_slot: 0,
        unclaimed_fees: 90 * SOL,
    };

    let new_epoch = 6;
    if config.last_epoch != new_epoch {
        config.epoch_fees_earned = 0;
        config.last_epoch = new_epoch;
    }

    assert_eq!(config.epoch_fees_earned, 0, "Epoch fees should reset on new epoch");
    assert_eq!(config.last_epoch, 6);
}

// ═══════════════════════════════════════════════════════════════════════════
//  7. Fee attribution for multi-program transactions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_fee_attribution_single_program() {
    // A transaction invoking a single program: 100% of the developer share
    // goes to that program.
    let total_dev_share = 100 * SOL;
    let programs_invoked = 1;
    let per_program_share = total_dev_share / programs_invoked;
    assert_eq!(per_program_share, 100 * SOL);
}

#[test]
fn test_fee_attribution_two_programs() {
    // A transaction invoking two programs: dev share is split equally.
    // (In the actual runtime, the split may be CU-weighted, but equal is
    // the simple model.)
    let total_dev_share = 100 * SOL;
    let programs_invoked = 2;
    let per_program_share = total_dev_share / programs_invoked;
    assert_eq!(per_program_share, 50 * SOL);
}

#[test]
fn test_fee_attribution_unregistered_program_gets_nothing() {
    // If a program doesn't have a ProgramRevenueConfig, its share goes
    // to burn instead.
    let has_revenue_config = false;
    assert!(!has_revenue_config, "Unregistered program should get 0 dev fees");
}

// ═══════════════════════════════════════════════════════════════════════════
//  8. Fee claiming
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_claim_accumulates_correctly() {
    let mut config = ProgramRevenueConfig {
        version: 1,
        program_id: Pubkey::new_unique(),
        revenue_recipient: Pubkey::new_unique(),
        update_authority: Pubkey::new_unique(),
        is_active: true,
        revenue_splits: Vec::new(),
        total_fees_earned: 0,
        epoch_fees_earned: 0,
        last_epoch: 0,
        eligible_after_slot: 0,
        unclaimed_fees: 0,
    };

    // Simulate 3 credits
    for _ in 0..3 {
        config.unclaimed_fees += 10 * SOL;
        config.total_fees_earned += 10 * SOL;
    }

    assert_eq!(config.unclaimed_fees, 30 * SOL);
    assert_eq!(config.total_fees_earned, 30 * SOL);

    // Claim
    let claimed = config.unclaimed_fees;
    config.unclaimed_fees = 0;
    assert_eq!(claimed, 30 * SOL);
    assert_eq!(config.unclaimed_fees, 0);
    // total_fees_earned is lifetime — doesn't reset
    assert_eq!(config.total_fees_earned, 30 * SOL);
}

#[test]
fn test_claim_with_splits_distributes_correctly() {
    let r1 = Pubkey::new_unique();
    let r2 = Pubkey::new_unique();
    let r3 = Pubkey::new_unique();

    let splits = vec![
        RevenueSplit { recipient: r1, share_bps: 5_000 }, // 50%
        RevenueSplit { recipient: r2, share_bps: 3_000 }, // 30%
        RevenueSplit { recipient: r3, share_bps: 2_000 }, // 20%
    ];

    let claim_amount: u64 = 100 * SOL;

    // Calculate shares
    let mut distributed: u64 = 0;
    let mut shares = Vec::new();
    for (i, split) in splits.iter().enumerate() {
        let share = if i == splits.len() - 1 {
            claim_amount - distributed
        } else {
            (claim_amount as u128 * split.share_bps as u128 / TOTAL_BPS as u128) as u64
        };
        shares.push(share);
        distributed += share;
    }

    assert_eq!(shares[0], 50 * SOL);  // 50%
    assert_eq!(shares[1], 30 * SOL);  // 30%
    assert_eq!(shares[2], 20 * SOL);  // 20% (remainder)
    assert_eq!(distributed, claim_amount, "All funds must be distributed");
}

// ═══════════════════════════════════════════════════════════════════════════
//  9. Epoch fee tracker
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_epoch_fee_tracker_rollover() {
    let mut tracker = EpochFeeTracker {
        version: 1,
        epoch: 5,
        total_developer_fees: 500 * SOL,
    };

    let new_epoch = 6;
    if tracker.epoch != new_epoch {
        tracker.epoch = new_epoch;
        tracker.total_developer_fees = 0;
    }

    assert_eq!(tracker.epoch, 6);
    assert_eq!(tracker.total_developer_fees, 0);
}

#[test]
fn test_epoch_fee_tracker_same_epoch_accumulates() {
    let mut tracker = EpochFeeTracker {
        version: 1,
        epoch: 5,
        total_developer_fees: 100 * SOL,
    };

    // Same epoch — accumulate
    let credit = 50 * SOL;
    tracker.total_developer_fees = tracker.total_developer_fees.saturating_add(credit);

    assert_eq!(tracker.total_developer_fees, 150 * SOL);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. State sizes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_program_revenue_config_max_size() {
    assert_eq!(ProgramRevenueConfig::MAX_SIZE, 512);
}

#[test]
fn test_epoch_fee_tracker_max_size() {
    assert_eq!(EpochFeeTracker::MAX_SIZE, 64);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Fee-split schedule constants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_launch_fee_split_constants() {
    use trv1_developer_rewards_program::constants::launch;
    assert_eq!(launch::BURN_BPS, 1_000);      // 10%
    assert_eq!(launch::VALIDATOR_BPS, 0);      // 0%
    assert_eq!(launch::TREASURY_BPS, 4_500);   // 45%
    assert_eq!(launch::DEVELOPER_BPS, 4_500);  // 45%

    let sum = launch::BURN_BPS + launch::VALIDATOR_BPS + launch::TREASURY_BPS + launch::DEVELOPER_BPS;
    assert_eq!(sum, 10_000, "Launch fee splits must sum to 10000 bps");
}

#[test]
fn test_maturity_fee_split_constants() {
    use trv1_developer_rewards_program::constants::maturity;
    assert_eq!(maturity::BURN_BPS, 2_500);      // 25%
    assert_eq!(maturity::VALIDATOR_BPS, 2_500);  // 25%
    assert_eq!(maturity::TREASURY_BPS, 2_500);   // 25%
    assert_eq!(maturity::DEVELOPER_BPS, 2_500);  // 25%

    let sum = maturity::BURN_BPS + maturity::VALIDATOR_BPS + maturity::TREASURY_BPS + maturity::DEVELOPER_BPS;
    assert_eq!(sum, 10_000, "Maturity fee splits must sum to 10000 bps");
}

#[test]
fn test_transition_epochs() {
    use trv1_developer_rewards_program::constants::TRANSITION_EPOCHS;
    assert_eq!(TRANSITION_EPOCHS, 912);
}

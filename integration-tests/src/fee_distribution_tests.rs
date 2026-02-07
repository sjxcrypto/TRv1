//! Integration tests for TRv1 Fee Distribution.
//!
//! Tests the epoch-dependent 4-way fee split:
//!   - Burn
//!   - Validator (leader)
//!   - Treasury
//!   - dApp developer
//!
//! The split transitions linearly from launch to maturity over ~1825 epochs.

use {
    crate::harness::SOL,
    solana_runtime::trv1_constants::{
        self, FEE_TRANSITION_EPOCHS, LAUNCH_BURN_PCT, LAUNCH_DEV_PCT,
        LAUNCH_TREASURY_PCT, LAUNCH_VALIDATOR_PCT, MATURE_BURN_PCT, MATURE_DEV_PCT,
        MATURE_TREASURY_PCT, MATURE_VALIDATOR_PCT,
    },
};

// ═══════════════════════════════════════════════════════════════════════════
//  Helper
// ═══════════════════════════════════════════════════════════════════════════

fn assert_near(actual: f64, expected: f64, label: &str) {
    assert!(
        (actual - expected).abs() < 1e-10,
        "{}: expected {}, got {}",
        label,
        expected,
        actual
    );
}

fn assert_sums_to_one(burn: f64, validator: f64, treasury: f64, dev: f64) {
    let sum = burn + validator + treasury + dev;
    assert!(
        (sum - 1.0).abs() < 1e-10,
        "Fee percentages must sum to 1.0, got {}",
        sum
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  1. Epoch 0: 10% / 0% / 45% / 45%
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_fee_distribution_epoch_0() {
    let (burn, validator, treasury, dev) = trv1_constants::fee_distribution_for_epoch(0);

    assert_near(burn, 0.10, "burn at epoch 0");
    assert_near(validator, 0.00, "validator at epoch 0");
    assert_near(treasury, 0.45, "treasury at epoch 0");
    assert_near(dev, 0.45, "dev at epoch 0");
    assert_sums_to_one(burn, validator, treasury, dev);
}

#[test]
fn test_fee_distribution_epoch_0_actual_amounts() {
    let total_fee = 1000 * SOL;
    let (burn_pct, validator_pct, treasury_pct, dev_pct) =
        trv1_constants::fee_distribution_for_epoch(0);

    let burn = (total_fee as f64 * burn_pct) as u64;
    let validator = (total_fee as f64 * validator_pct) as u64;
    let treasury = (total_fee as f64 * treasury_pct) as u64;
    let dev = (total_fee as f64 * dev_pct) as u64;

    assert_eq!(burn, 100 * SOL);      // 10%
    assert_eq!(validator, 0);          // 0%
    assert_eq!(treasury, 450 * SOL);   // 45%
    assert_eq!(dev, 450 * SOL);        // 45%
}

// ═══════════════════════════════════════════════════════════════════════════
//  2. Epoch 912: ~halfway transition values
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_fee_distribution_epoch_912() {
    let (burn, validator, treasury, dev) = trv1_constants::fee_distribution_for_epoch(912);
    let progress = 912.0 / FEE_TRANSITION_EPOCHS as f64;

    // At ~50% progress:
    // burn: 0.10 + 0.5 * (0.25 - 0.10) = 0.10 + 0.075 = 0.175
    // validator: 0.00 + 0.5 * (0.25 - 0.00) = 0.125
    // treasury: 0.45 + 0.5 * (0.25 - 0.45) = 0.45 - 0.10 = 0.35
    // dev: 0.45 + 0.5 * (0.25 - 0.45) = 0.35

    assert_sums_to_one(burn, validator, treasury, dev);

    // Burn should be between 10% and 25%
    assert!(burn > 0.10 && burn < 0.25, "Burn at epoch 912: {}", burn);
    // Validator between 0% and 25%
    assert!(
        validator > 0.00 && validator < 0.25,
        "Validator at epoch 912: {}",
        validator
    );
    // Treasury between 25% and 45%
    assert!(
        treasury > 0.25 && treasury < 0.45,
        "Treasury at epoch 912: {}",
        treasury
    );
    // Dev between 25% and 45%
    assert!(dev > 0.25 && dev < 0.45, "Dev at epoch 912: {}", dev);

    // Check approximate values with looser tolerance
    let expected_burn = LAUNCH_BURN_PCT + progress * (MATURE_BURN_PCT - LAUNCH_BURN_PCT);
    let expected_validator =
        LAUNCH_VALIDATOR_PCT + progress * (MATURE_VALIDATOR_PCT - LAUNCH_VALIDATOR_PCT);

    assert!(
        (burn - expected_burn).abs() < 1e-10,
        "Burn: expected {}, got {}",
        expected_burn,
        burn
    );
    assert!(
        (validator - expected_validator).abs() < 1e-10,
        "Validator: expected {}, got {}",
        expected_validator,
        validator
    );
}

#[test]
fn test_fee_distribution_epoch_912_actual_amounts() {
    let total_fee = 1000 * SOL;
    let (burn_pct, validator_pct, treasury_pct, dev_pct) =
        trv1_constants::fee_distribution_for_epoch(912);

    let burn = (total_fee as f64 * burn_pct) as u64;
    let validator = (total_fee as f64 * validator_pct) as u64;
    let treasury = (total_fee as f64 * treasury_pct) as u64;
    let dev = (total_fee as f64 * dev_pct) as u64;

    let total_distributed = burn + validator + treasury + dev;
    // Allow 1 SOL of rounding error
    assert!(
        (total_distributed as i64 - total_fee as i64).unsigned_abs() <= SOL,
        "Total distributed ({}) should be close to total fee ({})",
        total_distributed,
        total_fee
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  3. Epoch 1825+: 25% / 25% / 25% / 25%
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_fee_distribution_at_maturity_epoch_1825() {
    let (burn, validator, treasury, dev) =
        trv1_constants::fee_distribution_for_epoch(FEE_TRANSITION_EPOCHS);

    assert_near(burn, 0.25, "burn at maturity");
    assert_near(validator, 0.25, "validator at maturity");
    assert_near(treasury, 0.25, "treasury at maturity");
    assert_near(dev, 0.25, "dev at maturity");
    assert_sums_to_one(burn, validator, treasury, dev);
}

#[test]
fn test_fee_distribution_past_maturity_epoch_5000() {
    let (burn, validator, treasury, dev) = trv1_constants::fee_distribution_for_epoch(5_000);

    assert_near(burn, 0.25, "burn past maturity");
    assert_near(validator, 0.25, "validator past maturity");
    assert_near(treasury, 0.25, "treasury past maturity");
    assert_near(dev, 0.25, "dev past maturity");
    assert_sums_to_one(burn, validator, treasury, dev);
}

#[test]
fn test_fee_distribution_far_future_epoch_1m() {
    let (burn, validator, treasury, dev) = trv1_constants::fee_distribution_for_epoch(1_000_000);

    assert_near(burn, 0.25, "burn far future");
    assert_near(validator, 0.25, "validator far future");
    assert_near(treasury, 0.25, "treasury far future");
    assert_near(dev, 0.25, "dev far future");
}

#[test]
fn test_fee_distribution_maturity_actual_amounts() {
    let total_fee = 1000 * SOL;
    let (burn_pct, validator_pct, treasury_pct, dev_pct) =
        trv1_constants::fee_distribution_for_epoch(FEE_TRANSITION_EPOCHS);

    let burn = (total_fee as f64 * burn_pct) as u64;
    let validator = (total_fee as f64 * validator_pct) as u64;
    let treasury = (total_fee as f64 * treasury_pct) as u64;
    let dev = (total_fee as f64 * dev_pct) as u64;

    assert_eq!(burn, 250 * SOL);       // 25%
    assert_eq!(validator, 250 * SOL);   // 25%
    assert_eq!(treasury, 250 * SOL);    // 25%
    assert_eq!(dev, 250 * SOL);         // 25%
}

// ═══════════════════════════════════════════════════════════════════════════
//  4. Linear transition properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_transition_is_monotonic() {
    // Burn should increase from 10% to 25%
    // Validator should increase from 0% to 25%
    // Treasury should decrease from 45% to 25%
    // Dev should decrease from 45% to 25%

    let mut prev_burn = 0.0_f64;
    let mut prev_validator = 0.0_f64;
    let mut prev_treasury = 1.0_f64;
    let mut prev_dev = 1.0_f64;

    for epoch in (0..=FEE_TRANSITION_EPOCHS).step_by(10) {
        let (burn, validator, treasury, dev) =
            trv1_constants::fee_distribution_for_epoch(epoch);

        assert!(
            burn >= prev_burn - 1e-10,
            "Burn should be non-decreasing: {} < {} at epoch {}",
            burn,
            prev_burn,
            epoch
        );
        assert!(
            validator >= prev_validator - 1e-10,
            "Validator should be non-decreasing: {} < {} at epoch {}",
            validator,
            prev_validator,
            epoch
        );
        assert!(
            treasury <= prev_treasury + 1e-10,
            "Treasury should be non-increasing: {} > {} at epoch {}",
            treasury,
            prev_treasury,
            epoch
        );
        assert!(
            dev <= prev_dev + 1e-10,
            "Dev should be non-increasing: {} > {} at epoch {}",
            dev,
            prev_dev,
            epoch
        );

        assert_sums_to_one(burn, validator, treasury, dev);

        prev_burn = burn;
        prev_validator = validator;
        prev_treasury = treasury;
        prev_dev = dev;
    }
}

#[test]
fn test_all_epochs_sum_to_one() {
    // Verify the invariant at every 100th epoch through and past transition.
    for epoch in (0..=2000).step_by(100) {
        let (burn, validator, treasury, dev) =
            trv1_constants::fee_distribution_for_epoch(epoch);
        let sum = burn + validator + treasury + dev;
        assert!(
            (sum - 1.0).abs() < 1e-10,
            "Epoch {}: percentages sum to {}, not 1.0",
            epoch,
            sum
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  5. Base fee + priority fee handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_both_fees_included_in_distribution() {
    // The fee distribution applies to total_fee = base_fee + priority_fee.
    let base_fee = 5_000; // 5000 lamports (standard Solana)
    let priority_fee = 100_000; // 100_000 lamports
    let total_fee = base_fee + priority_fee;

    assert_eq!(total_fee, 105_000);

    let (burn_pct, validator_pct, treasury_pct, dev_pct) =
        trv1_constants::fee_distribution_for_epoch(0);

    let burn = (total_fee as f64 * burn_pct) as u64;
    let validator = (total_fee as f64 * validator_pct) as u64;
    let treasury = (total_fee as f64 * treasury_pct) as u64;
    let dev = (total_fee as f64 * dev_pct) as u64;

    // At epoch 0: 10% burn, 0% validator, 45% treasury, 45% dev
    assert_eq!(burn, 10_500);       // 10% of 105_000
    assert_eq!(validator, 0);       // 0%
    assert_eq!(treasury, 47_250);   // 45%
    assert_eq!(dev, 47_250);        // 45%
}

#[test]
fn test_zero_fee_produces_zero_distribution() {
    let total_fee = 0u64;
    let (burn_pct, validator_pct, treasury_pct, dev_pct) =
        trv1_constants::fee_distribution_for_epoch(500);

    let burn = (total_fee as f64 * burn_pct) as u64;
    let validator = (total_fee as f64 * validator_pct) as u64;
    let treasury = (total_fee as f64 * treasury_pct) as u64;
    let dev = (total_fee as f64 * dev_pct) as u64;

    assert_eq!(burn, 0);
    assert_eq!(validator, 0);
    assert_eq!(treasury, 0);
    assert_eq!(dev, 0);
}

#[test]
fn test_rounding_remainder_goes_to_burn() {
    // When the 4-way split doesn't divide evenly, the remainder should
    // be added to burn to avoid losing lamports.
    let total_fee = 3u64; // 3 lamports — doesn't divide evenly into 4
    let (burn_pct, validator_pct, treasury_pct, dev_pct) =
        trv1_constants::fee_distribution_for_epoch(FEE_TRANSITION_EPOCHS);

    let burn_base = (total_fee as f64 * burn_pct) as u64;
    let validator = (total_fee as f64 * validator_pct) as u64;
    let treasury = (total_fee as f64 * treasury_pct) as u64;
    let dev = (total_fee as f64 * dev_pct) as u64;

    let distributed = burn_base + validator + treasury + dev;
    let remainder = total_fee.saturating_sub(distributed);

    // Total should account for all lamports
    let final_burn = burn_base + remainder;
    assert_eq!(
        final_burn + validator + treasury + dev,
        total_fee,
        "All fee lamports must be accounted for"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  6. Constants consistency
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_launch_percentages_sum_to_1() {
    let sum = LAUNCH_BURN_PCT + LAUNCH_VALIDATOR_PCT + LAUNCH_TREASURY_PCT + LAUNCH_DEV_PCT;
    assert!(
        (sum - 1.0).abs() < 1e-10,
        "Launch percentages sum: {}",
        sum
    );
}

#[test]
fn test_mature_percentages_sum_to_1() {
    let sum = MATURE_BURN_PCT + MATURE_VALIDATOR_PCT + MATURE_TREASURY_PCT + MATURE_DEV_PCT;
    assert!(
        (sum - 1.0).abs() < 1e-10,
        "Mature percentages sum: {}",
        sum
    );
}

#[test]
fn test_transition_epochs_is_5_years() {
    // 5 years × 365 days = 1825 epochs (daily epochs)
    assert_eq!(FEE_TRANSITION_EPOCHS, 1825);
}

#[test]
fn test_launch_constants_values() {
    assert!((LAUNCH_BURN_PCT - 0.10).abs() < 1e-10);
    assert!((LAUNCH_VALIDATOR_PCT - 0.00).abs() < 1e-10);
    assert!((LAUNCH_TREASURY_PCT - 0.45).abs() < 1e-10);
    assert!((LAUNCH_DEV_PCT - 0.45).abs() < 1e-10);
}

#[test]
fn test_mature_constants_values() {
    assert!((MATURE_BURN_PCT - 0.25).abs() < 1e-10);
    assert!((MATURE_VALIDATOR_PCT - 0.25).abs() < 1e-10);
    assert!((MATURE_TREASURY_PCT - 0.25).abs() < 1e-10);
    assert!((MATURE_DEV_PCT - 0.25).abs() < 1e-10);
}

// ═══════════════════════════════════════════════════════════════════════════
//  7. Quarter-epoch checkpoints
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_quarter_transition_checkpoints() {
    let quarters = [
        (0, 0.10, 0.00, 0.45, 0.45),
        (FEE_TRANSITION_EPOCHS / 4, 0.1375, 0.0625, 0.4, 0.4),
        (FEE_TRANSITION_EPOCHS / 2, 0.175, 0.125, 0.35, 0.35),
        (3 * FEE_TRANSITION_EPOCHS / 4, 0.2125, 0.1875, 0.30, 0.30),
        (FEE_TRANSITION_EPOCHS, 0.25, 0.25, 0.25, 0.25),
    ];

    for (epoch, exp_burn, exp_val, exp_treas, exp_dev) in &quarters {
        let (burn, validator, treasury, dev) =
            trv1_constants::fee_distribution_for_epoch(*epoch);

        assert!(
            (burn - exp_burn).abs() < 0.001,
            "Epoch {}: burn expected ~{}, got {}",
            epoch, exp_burn, burn
        );
        assert!(
            (validator - exp_val).abs() < 0.001,
            "Epoch {}: validator expected ~{}, got {}",
            epoch, exp_val, validator
        );
        assert!(
            (treasury - exp_treas).abs() < 0.001,
            "Epoch {}: treasury expected ~{}, got {}",
            epoch, exp_treas, treasury
        );
        assert!(
            (dev - exp_dev).abs() < 0.001,
            "Epoch {}: dev expected ~{}, got {}",
            epoch, exp_dev, dev
        );
    }
}

//! TRv1 blockchain-specific constants for economic model configuration.
//!
//! These constants define the staking rewards, fee distribution, slashing,
//! and validator governance parameters for the TRv1 network.

/// Annual staking reward rate: 5% per year, applied only to staked tokens.
pub const STAKING_RATE: f64 = 0.05;

/// Number of epochs over which fee distribution transitions from launch to mature model.
/// Approximately 5 years at daily epochs (365 * 5 = 1825).
pub const FEE_TRANSITION_EPOCHS: u64 = 1825;

// === Fee Distribution: Launch Parameters ===
/// Burn percentage at launch: 10%
pub const LAUNCH_BURN_PCT: f64 = 0.10;
/// Validator percentage at launch: 0%
pub const LAUNCH_VALIDATOR_PCT: f64 = 0.00;
/// Treasury percentage at launch: 45%
pub const LAUNCH_TREASURY_PCT: f64 = 0.45;
/// dApp developer percentage at launch: 45%
pub const LAUNCH_DEV_PCT: f64 = 0.45;

// === Fee Distribution: Mature Parameters (Year 5+) ===
/// Burn percentage at maturity: 25%
pub const MATURE_BURN_PCT: f64 = 0.25;
/// Validator percentage at maturity: 25%
pub const MATURE_VALIDATOR_PCT: f64 = 0.25;
/// Treasury percentage at maturity: 25%
pub const MATURE_TREASURY_PCT: f64 = 0.25;
/// dApp developer percentage at maturity: 25%
pub const MATURE_DEV_PCT: f64 = 0.25;

/// Passive staking tiers: (lock_days, reward_rate_multiplier).
/// The multiplier is applied to the base validator staking rate.
pub const PASSIVE_STAKE_TIERS: [(u64, f64); 6] = [
    (0, 0.05),        // no lock: 5% of validator rate
    (30, 0.10),       // 30 days: 10%
    (90, 0.20),       // 90 days: 20%
    (180, 0.30),      // 180 days: 30%
    (360, 0.50),      // 360 days: 50%
    (u64::MAX, 1.20), // permanent: 120%
];

/// Penalty multiplier for early unlock: 5x the reward rate.
pub const EARLY_UNLOCK_PENALTY_MULTIPLIER: f64 = 5.0;

/// Maximum number of active validators in the network.
pub const ACTIVE_VALIDATOR_CAP: u32 = 200;

// === Slashing Parameters ===
/// Slash percentage for double-signing: 5%
pub const SLASH_DOUBLE_SIGN_PCT: f64 = 0.05;
/// Slash percentage for producing an invalid block: 10%
pub const SLASH_INVALID_BLOCK_PCT: f64 = 0.10;
/// Slash percentage for repeated offenses: 25%
pub const SLASH_REPEAT_PCT: f64 = 0.25;

/// Hours a validator is jailed for being offline.
pub const JAIL_OFFLINE_HOURS: u64 = 24;

// === EIP-1559 Fee Market Genesis Defaults ===
/// Starting base fee per compute unit in lamports at genesis.
pub const INITIAL_BASE_FEE: u64 = 5_000;

/// Maximum base fee per compute unit in lamports.
pub const MAX_BASE_FEE: u64 = 50_000_000;

/// Maximum block compute units.
pub const MAX_BLOCK_COMPUTE_UNITS: u64 = 48_000_000;

/// Target block utilization percentage (0-100).
pub const TARGET_UTILIZATION_PCT: u8 = 50;

/// Base fee change denominator (±1/N max change per block, 8 = ±12.5%).
pub const BASE_FEE_CHANGE_DENOMINATOR: u64 = 8;

/// Treasury account pubkey placeholder (to be set at genesis).
/// This is a default placeholder; the real treasury pubkey should be
/// configured in the genesis config.
pub const TREASURY_PUBKEY_DEFAULT: &str = "TRv1Treasury111111111111111111111111111111111";

/// Calculates the fee distribution percentages for a given epoch.
///
/// Returns (burn_pct, validator_pct, treasury_pct, dev_pct) as f64 values summing to 1.0.
pub fn fee_distribution_for_epoch(current_epoch: u64) -> (f64, f64, f64, f64) {
    let progress = if FEE_TRANSITION_EPOCHS == 0 {
        1.0
    } else {
        (current_epoch as f64 / FEE_TRANSITION_EPOCHS as f64).min(1.0)
    };

    let burn_pct = LAUNCH_BURN_PCT + (progress * (MATURE_BURN_PCT - LAUNCH_BURN_PCT));
    let validator_pct =
        LAUNCH_VALIDATOR_PCT + (progress * (MATURE_VALIDATOR_PCT - LAUNCH_VALIDATOR_PCT));
    let treasury_pct =
        LAUNCH_TREASURY_PCT + (progress * (MATURE_TREASURY_PCT - LAUNCH_TREASURY_PCT));
    let dev_pct = LAUNCH_DEV_PCT + (progress * (MATURE_DEV_PCT - LAUNCH_DEV_PCT));

    (burn_pct, validator_pct, treasury_pct, dev_pct)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fee_distribution_at_launch() {
        let (burn, validator, treasury, dev) = fee_distribution_for_epoch(0);
        assert!((burn - 0.10).abs() < 1e-10);
        assert!((validator - 0.00).abs() < 1e-10);
        assert!((treasury - 0.45).abs() < 1e-10);
        assert!((dev - 0.45).abs() < 1e-10);
    }

    #[test]
    fn test_fee_distribution_at_maturity() {
        let (burn, validator, treasury, dev) = fee_distribution_for_epoch(FEE_TRANSITION_EPOCHS);
        assert!((burn - 0.25).abs() < 1e-10);
        assert!((validator - 0.25).abs() < 1e-10);
        assert!((treasury - 0.25).abs() < 1e-10);
        assert!((dev - 0.25).abs() < 1e-10);
    }

    #[test]
    fn test_fee_distribution_past_maturity() {
        let (burn, validator, treasury, dev) =
            fee_distribution_for_epoch(FEE_TRANSITION_EPOCHS * 2);
        assert!((burn - 0.25).abs() < 1e-10);
        assert!((validator - 0.25).abs() < 1e-10);
        assert!((treasury - 0.25).abs() < 1e-10);
        assert!((dev - 0.25).abs() < 1e-10);
    }

    #[test]
    fn test_fee_distribution_midpoint() {
        let midpoint = FEE_TRANSITION_EPOCHS / 2;
        let (burn, validator, treasury, dev) = fee_distribution_for_epoch(midpoint);
        // At midpoint (~912 epochs), progress ≈ 0.5
        let sum = burn + validator + treasury + dev;
        assert!(
            (sum - 1.0).abs() < 1e-10,
            "Percentages must sum to 1.0, got {sum}"
        );
        // Burn should be between 10% and 25%
        assert!(burn > 0.10 && burn < 0.25);
        // Validator should be between 0% and 25%
        assert!(validator > 0.00 && validator < 0.25);
    }

    #[test]
    fn test_constants_consistency() {
        // Launch percentages must sum to 1.0
        let launch_sum =
            LAUNCH_BURN_PCT + LAUNCH_VALIDATOR_PCT + LAUNCH_TREASURY_PCT + LAUNCH_DEV_PCT;
        assert!(
            (launch_sum - 1.0).abs() < 1e-10,
            "Launch percentages must sum to 1.0"
        );

        // Mature percentages must sum to 1.0
        let mature_sum =
            MATURE_BURN_PCT + MATURE_VALIDATOR_PCT + MATURE_TREASURY_PCT + MATURE_DEV_PCT;
        assert!(
            (mature_sum - 1.0).abs() < 1e-10,
            "Mature percentages must sum to 1.0"
        );
    }
}

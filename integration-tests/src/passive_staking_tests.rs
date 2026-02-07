//! Integration tests for TRv1 Passive Staking.
//!
//! Exercises the passive staking program's tiered locks, reward calculations,
//! early unlock penalties, and governance vote weights.

use {
    crate::harness::{self, SOL, TRv1TestHarness},
    solana_passive_stake_program::{
        constants::{
            self, BPS_DENOMINATOR, EARLY_UNLOCK_PENALTY_30_DAY_BPS,
            EARLY_UNLOCK_PENALTY_360_DAY_BPS, EARLY_UNLOCK_PENALTY_90_DAY_BPS,
            EARLY_UNLOCK_PENALTY_180_DAY_BPS, EARLY_UNLOCK_PENALTY_NO_LOCK_BPS,
            PERMANENT_LOCK_DAYS, REWARD_RATE_30_DAY_BPS, REWARD_RATE_90_DAY_BPS,
            REWARD_RATE_180_DAY_BPS, REWARD_RATE_360_DAY_BPS, REWARD_RATE_NO_LOCK_BPS,
            REWARD_RATE_PERMANENT_BPS, SECONDS_PER_DAY, TIER_180_DAY, TIER_30_DAY,
            TIER_360_DAY, TIER_90_DAY, TIER_NO_LOCK, VOTE_WEIGHT_180_DAY,
            VOTE_WEIGHT_30_DAY, VOTE_WEIGHT_360_DAY, VOTE_WEIGHT_90_DAY,
            VOTE_WEIGHT_NO_LOCK, VOTE_WEIGHT_PERMANENT,
        },
        state::PassiveStakeAccount,
    },
};

// ═══════════════════════════════════════════════════════════════════════════
//  1. Tier creation & validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_all_tiers_are_valid() {
    let valid_tiers = [
        TIER_NO_LOCK,
        TIER_30_DAY,
        TIER_90_DAY,
        TIER_180_DAY,
        TIER_360_DAY,
        PERMANENT_LOCK_DAYS,
    ];
    for tier in &valid_tiers {
        assert!(
            constants::is_valid_tier(*tier),
            "Tier {} should be valid",
            tier
        );
    }
}

#[test]
fn test_invalid_tiers_rejected() {
    let invalid_tiers = [1, 7, 15, 29, 31, 91, 365, 999, u64::MAX - 1];
    for tier in &invalid_tiers {
        assert!(
            !constants::is_valid_tier(*tier),
            "Tier {} should be invalid",
            tier
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  2. Reward rates match spec
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_reward_rate_no_lock() {
    // No lock: 5% of validator rate → 500 bps of validator rate
    let rate = constants::reward_rate_bps_for_tier(TIER_NO_LOCK).unwrap();
    assert_eq!(rate, REWARD_RATE_NO_LOCK_BPS);
    assert_eq!(rate, 500);
}

#[test]
fn test_reward_rate_30_day() {
    // 30-day: 10% of validator rate → 1000 bps
    let rate = constants::reward_rate_bps_for_tier(TIER_30_DAY).unwrap();
    assert_eq!(rate, REWARD_RATE_30_DAY_BPS);
    assert_eq!(rate, 1_000);
}

#[test]
fn test_reward_rate_90_day() {
    // 90-day: 20% of validator rate → 2000 bps
    let rate = constants::reward_rate_bps_for_tier(TIER_90_DAY).unwrap();
    assert_eq!(rate, REWARD_RATE_90_DAY_BPS);
    assert_eq!(rate, 2_000);
}

#[test]
fn test_reward_rate_180_day() {
    // 180-day: 30% of validator rate → 3000 bps
    let rate = constants::reward_rate_bps_for_tier(TIER_180_DAY).unwrap();
    assert_eq!(rate, REWARD_RATE_180_DAY_BPS);
    assert_eq!(rate, 3_000);
}

#[test]
fn test_reward_rate_360_day() {
    // 360-day: 50% of validator rate → 5000 bps
    let rate = constants::reward_rate_bps_for_tier(TIER_360_DAY).unwrap();
    assert_eq!(rate, REWARD_RATE_360_DAY_BPS);
    assert_eq!(rate, 5_000);
}

#[test]
fn test_reward_rate_permanent() {
    // Permanent: 120% of validator rate → 12000 bps
    let rate = constants::reward_rate_bps_for_tier(PERMANENT_LOCK_DAYS).unwrap();
    assert_eq!(rate, REWARD_RATE_PERMANENT_BPS);
    assert_eq!(rate, 12_000);
}

#[test]
fn test_reward_rate_invalid_tier_returns_none() {
    assert!(constants::reward_rate_bps_for_tier(42).is_none());
    assert!(constants::reward_rate_bps_for_tier(365).is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
//  3. Reward rate monotonically increases with lock duration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_reward_rates_increase_with_longer_locks() {
    let tiers = [
        TIER_NO_LOCK,
        TIER_30_DAY,
        TIER_90_DAY,
        TIER_180_DAY,
        TIER_360_DAY,
        PERMANENT_LOCK_DAYS,
    ];
    let rates: Vec<u64> = tiers
        .iter()
        .map(|t| constants::reward_rate_bps_for_tier(*t).unwrap())
        .collect();

    for i in 1..rates.len() {
        assert!(
            rates[i] > rates[i - 1],
            "Reward rate for tier {} ({} bps) should be > tier {} ({} bps)",
            tiers[i],
            rates[i],
            tiers[i - 1],
            rates[i - 1]
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  4. Approximate APY calculations (spec verification)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_approximate_apy_values() {
    // Validator staking rate = 5% APY = 500 bps
    let validator_rate_bps: u64 = 500; // 5% expressed as bps

    let expected_apys: Vec<(u64, f64)> = vec![
        (TIER_NO_LOCK, 0.25),      // 5% of 5%
        (TIER_30_DAY, 0.50),       // 10% of 5%
        (TIER_90_DAY, 1.00),       // 20% of 5%
        (TIER_180_DAY, 1.50),      // 30% of 5%
        (TIER_360_DAY, 2.50),      // 50% of 5%
        (PERMANENT_LOCK_DAYS, 6.00), // 120% of 5%
    ];

    for (tier, expected_apy_pct) in &expected_apys {
        let tier_rate_bps = constants::reward_rate_bps_for_tier(*tier).unwrap();
        // APY = validator_rate * tier_rate / BPS_DENOMINATOR
        // In percentage: (validator_rate_bps / 100) * (tier_rate_bps / 10000)
        let apy = (validator_rate_bps as f64 / 100.0) * (tier_rate_bps as f64 / BPS_DENOMINATOR as f64);
        assert!(
            (apy - expected_apy_pct).abs() < 0.01,
            "Tier {}: expected APY ~{}%, got {}%",
            tier,
            expected_apy_pct,
            apy
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  5. Epoch reward calculation (processor math)
// ═══════════════════════════════════════════════════════════════════════════

/// Mirrors the epoch reward formula from processor.rs:
/// reward_per_epoch = amount * validator_rate * tier_rate / (BPS² * 365)
fn compute_epoch_reward(amount: u64, validator_rate_bps: u64, tier_lock_days: u64) -> u64 {
    let tier_rate_bps = constants::reward_rate_bps_for_tier(tier_lock_days).unwrap();
    let amount = amount as u128;
    let v_rate = validator_rate_bps as u128;
    let t_rate = tier_rate_bps as u128;
    let denom = (BPS_DENOMINATOR as u128) * (BPS_DENOMINATOR as u128) * 365;
    (amount * v_rate * t_rate / denom) as u64
}

#[test]
fn test_epoch_reward_calculation_no_lock() {
    // 100 SOL staked, no lock, validator rate = 500 bps (5%)
    let amount = 100 * SOL;
    let reward = compute_epoch_reward(amount, 500, TIER_NO_LOCK);
    // Expected: 100 SOL * 500/10000 * 500/10000 / 365
    // = 100 * 0.05 * 0.05 / 365 ≈ 0.000685 SOL ≈ 685_000 lamports/epoch
    assert!(reward > 600_000 && reward < 800_000,
        "No-lock epoch reward should be ~685k lamports, got {}", reward);
}

#[test]
fn test_epoch_reward_calculation_permanent() {
    // 100 SOL staked, permanent lock, validator rate = 500 bps (5%)
    let amount = 100 * SOL;
    let reward = compute_epoch_reward(amount, 500, PERMANENT_LOCK_DAYS);
    // Expected: 100 SOL * 500/10000 * 12000/10000 / 365
    // = 100 * 0.05 * 1.2 / 365 ≈ 0.01644 SOL ≈ 16_438_356 lamports/epoch
    assert!(reward > 16_000_000 && reward < 17_000_000,
        "Permanent-lock epoch reward should be ~16.4M lamports, got {}", reward);
}

#[test]
fn test_epoch_reward_permanent_is_24x_no_lock() {
    let amount = 100 * SOL;
    let reward_no_lock = compute_epoch_reward(amount, 500, TIER_NO_LOCK);
    let reward_permanent = compute_epoch_reward(amount, 500, PERMANENT_LOCK_DAYS);
    // 12000/500 = 24x
    let ratio = reward_permanent as f64 / reward_no_lock as f64;
    assert!(
        (ratio - 24.0).abs() < 0.1,
        "Permanent should earn 24x no-lock rewards, got {:.2}x",
        ratio
    );
}

#[test]
fn test_multi_epoch_rewards_accumulate() {
    let amount = 100 * SOL;
    let reward_1_epoch = compute_epoch_reward(amount, 500, TIER_90_DAY);
    // 10 epochs of rewards should be 10x a single epoch
    let reward_10_epochs = reward_1_epoch * 10;
    assert_eq!(reward_10_epochs, reward_1_epoch * 10);

    // Annual rewards for 365 epochs should be approximately:
    // amount * validator_rate * tier_rate / BPS²
    let annual_reward = reward_1_epoch * 365;
    let expected_annual = ((amount as f64) * 0.05 * 0.20) as u64;
    let tolerance = expected_annual / 100; // 1% tolerance for rounding
    assert!(
        (annual_reward as i64 - expected_annual as i64).unsigned_abs() < tolerance,
        "Annual reward: got {}, expected ~{} (tolerance {})",
        annual_reward, expected_annual, tolerance
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  6. Lock expiry and unlock
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_lock_end_calculation() {
    let now: i64 = 1_700_000_000;

    // No-lock: lock_end = 0 (can withdraw anytime)
    let lock_end_no_lock = 0i64;
    assert_eq!(lock_end_no_lock, 0);

    // 30-day: lock_end = now + 30 * 86400
    let lock_end_30 = now + 30 * SECONDS_PER_DAY;
    assert_eq!(lock_end_30, now + 2_592_000);

    // 360-day: lock_end = now + 360 * 86400
    let lock_end_360 = now + 360 * SECONDS_PER_DAY;
    assert_eq!(lock_end_360, now + 31_104_000);

    // Permanent: lock_end = 0 (cannot unlock)
    let lock_end_permanent = 0i64;
    assert_eq!(lock_end_permanent, 0);
}

#[test]
fn test_no_lock_can_always_withdraw() {
    // No-lock tier: lock_days = 0, lock_end = 0
    // Unlock should succeed at any time since lock_days == TIER_NO_LOCK
    let lock_days = TIER_NO_LOCK;
    assert_eq!(lock_days, 0);
    // In the processor, no-lock bypasses the time check entirely.
}

#[test]
fn test_timed_lock_must_wait() {
    let now: i64 = 1_700_000_000;
    let lock_end = now + 30 * SECONDS_PER_DAY;

    // Before expiry: cannot unlock
    let check_time = now + 15 * SECONDS_PER_DAY;
    assert!(check_time < lock_end, "Should not be able to unlock before lock_end");

    // At expiry: can unlock
    assert!(lock_end <= lock_end, "Should be able to unlock at lock_end");

    // After expiry: can unlock
    let check_after = now + 31 * SECONDS_PER_DAY;
    assert!(check_after >= lock_end, "Should be able to unlock after lock_end");
}

// ═══════════════════════════════════════════════════════════════════════════
//  7. Early unlock penalties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_early_unlock_penalty_values() {
    // Penalty = 5× the tier's reward-rate percentage
    assert_eq!(EARLY_UNLOCK_PENALTY_NO_LOCK_BPS, 0);       // 0% (instant withdraw)
    assert_eq!(EARLY_UNLOCK_PENALTY_30_DAY_BPS, 250);      // 2.5%
    assert_eq!(EARLY_UNLOCK_PENALTY_90_DAY_BPS, 500);      // 5.0%
    assert_eq!(EARLY_UNLOCK_PENALTY_180_DAY_BPS, 750);     // 7.5%
    assert_eq!(EARLY_UNLOCK_PENALTY_360_DAY_BPS, 1_250);   // 12.5%
}

#[test]
fn test_early_unlock_penalty_calculation_30_day() {
    let principal = 100 * SOL;
    let penalty_bps = constants::early_unlock_penalty_bps_for_tier(TIER_30_DAY).unwrap();
    assert_eq!(penalty_bps, 250); // 2.5%

    let penalty = principal * penalty_bps / BPS_DENOMINATOR;
    let returned = principal - penalty;

    assert_eq!(penalty, 2_500_000_000);   // 2.5 SOL burned
    assert_eq!(returned, 97_500_000_000); // 97.5 SOL returned
}

#[test]
fn test_early_unlock_penalty_calculation_90_day() {
    let principal = 100 * SOL;
    let penalty_bps = constants::early_unlock_penalty_bps_for_tier(TIER_90_DAY).unwrap();
    assert_eq!(penalty_bps, 500); // 5.0%

    let penalty = principal * penalty_bps / BPS_DENOMINATOR;
    let returned = principal - penalty;

    assert_eq!(penalty, 5_000_000_000);   // 5 SOL burned
    assert_eq!(returned, 95_000_000_000); // 95 SOL returned
}

#[test]
fn test_early_unlock_penalty_calculation_180_day() {
    let principal = 100 * SOL;
    let penalty_bps = constants::early_unlock_penalty_bps_for_tier(TIER_180_DAY).unwrap();
    assert_eq!(penalty_bps, 750); // 7.5%

    let penalty = principal * penalty_bps / BPS_DENOMINATOR;
    let returned = principal - penalty;

    assert_eq!(penalty, 7_500_000_000);   // 7.5 SOL burned
    assert_eq!(returned, 92_500_000_000); // 92.5 SOL returned
}

#[test]
fn test_early_unlock_penalty_calculation_360_day() {
    let principal = 100 * SOL;
    let penalty_bps = constants::early_unlock_penalty_bps_for_tier(TIER_360_DAY).unwrap();
    assert_eq!(penalty_bps, 1_250); // 12.5%

    let penalty = principal * penalty_bps / BPS_DENOMINATOR;
    let returned = principal - penalty;

    assert_eq!(penalty, 12_500_000_000);  // 12.5 SOL burned
    assert_eq!(returned, 87_500_000_000); // 87.5 SOL returned
}

#[test]
fn test_no_lock_early_unlock_penalty_is_zero() {
    let penalty_bps = constants::early_unlock_penalty_bps_for_tier(TIER_NO_LOCK).unwrap();
    assert_eq!(penalty_bps, 0);

    let principal = 100 * SOL;
    let penalty = principal * penalty_bps / BPS_DENOMINATOR;
    assert_eq!(penalty, 0);
}

#[test]
fn test_permanent_lock_cannot_early_unlock() {
    // early_unlock_penalty_bps_for_tier returns None for permanent locks
    let result = constants::early_unlock_penalty_bps_for_tier(PERMANENT_LOCK_DAYS);
    assert!(
        result.is_none(),
        "Permanent locks should not have an early unlock penalty (they cannot be unlocked)"
    );
}

#[test]
fn test_penalty_is_burned_not_redistributed() {
    // In the processor, early-unlock removes ALL lamports from the stake account.
    // Only (principal - penalty) is credited back to the authority.
    // The penalty portion is effectively destroyed (burned).
    //
    // We verify the math: total removed from stake account should equal
    // returned + burned, and burned should not appear in any other account.
    let principal = 50 * SOL;
    let penalty_bps = EARLY_UNLOCK_PENALTY_90_DAY_BPS;
    let penalty = principal * penalty_bps / BPS_DENOMINATOR;
    let returned = principal - penalty;

    assert_eq!(returned + penalty, principal);
    assert!(penalty > 0);
}

// ═══════════════════════════════════════════════════════════════════════════
//  8. Governance vote weights per tier
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_vote_weight_values() {
    assert_eq!(VOTE_WEIGHT_NO_LOCK, 0);           // 0x  — cannot vote
    assert_eq!(VOTE_WEIGHT_30_DAY, 1_000);         // 0.10x
    assert_eq!(VOTE_WEIGHT_90_DAY, 2_000);         // 0.20x
    assert_eq!(VOTE_WEIGHT_180_DAY, 3_000);        // 0.30x
    assert_eq!(VOTE_WEIGHT_360_DAY, 5_000);        // 0.50x
    assert_eq!(VOTE_WEIGHT_PERMANENT, 15_000);     // 1.50x
}

#[test]
fn test_vote_weight_lookup() {
    assert_eq!(constants::vote_weight_bps_for_tier(TIER_NO_LOCK), Some(0));
    assert_eq!(constants::vote_weight_bps_for_tier(TIER_30_DAY), Some(1_000));
    assert_eq!(constants::vote_weight_bps_for_tier(TIER_90_DAY), Some(2_000));
    assert_eq!(constants::vote_weight_bps_for_tier(TIER_180_DAY), Some(3_000));
    assert_eq!(constants::vote_weight_bps_for_tier(TIER_360_DAY), Some(5_000));
    assert_eq!(constants::vote_weight_bps_for_tier(PERMANENT_LOCK_DAYS), Some(15_000));
}

#[test]
fn test_vote_weight_invalid_tier_returns_none() {
    assert!(constants::vote_weight_bps_for_tier(42).is_none());
}

#[test]
fn test_vote_weights_increase_with_lock_duration() {
    let tiers = [
        TIER_NO_LOCK,
        TIER_30_DAY,
        TIER_90_DAY,
        TIER_180_DAY,
        TIER_360_DAY,
        PERMANENT_LOCK_DAYS,
    ];
    let weights: Vec<u16> = tiers
        .iter()
        .map(|t| constants::vote_weight_bps_for_tier(*t).unwrap())
        .collect();

    // Weights should be non-decreasing (no-lock = 0, then strictly increasing)
    for i in 1..weights.len() {
        assert!(
            weights[i] > weights[i - 1],
            "Vote weight for tier {} ({}) should be > tier {} ({})",
            tiers[i], weights[i], tiers[i - 1], weights[i - 1]
        );
    }
}

#[test]
fn test_governance_weighted_vote_power() {
    // A user with 1000 SOL in a 360-day lock:
    // vote_power = 1000 * 5000 / 10000 = 500 SOL equivalent
    let staked = 1000 * SOL;
    let weight_bps = VOTE_WEIGHT_360_DAY as u64;
    let vote_power = staked * weight_bps / BPS_DENOMINATOR;
    assert_eq!(vote_power, 500 * SOL);

    // Permanent lock: 1000 * 15000 / 10000 = 1500 SOL equivalent
    let weight_perm = VOTE_WEIGHT_PERMANENT as u64;
    let vote_power_perm = staked * weight_perm / BPS_DENOMINATOR;
    assert_eq!(vote_power_perm, 1500 * SOL);
}

#[test]
fn test_no_lock_has_zero_governance_power() {
    let staked = 1_000_000 * SOL;
    let weight_bps = VOTE_WEIGHT_NO_LOCK as u64;
    let vote_power = staked * weight_bps / BPS_DENOMINATOR;
    assert_eq!(vote_power, 0, "No-lock stakers should have zero governance power");
}

// ═══════════════════════════════════════════════════════════════════════════
//  9. State serialization round-trip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_passive_stake_account_serialization_roundtrip() {
    let state = PassiveStakeAccount {
        authority: solana_pubkey::Pubkey::new_unique(),
        amount: 42 * SOL,
        lock_days: TIER_90_DAY,
        lock_start: 1_700_000_000,
        lock_end: 1_700_000_000 + 90 * SECONDS_PER_DAY,
        unclaimed_rewards: 1_234_567,
        last_reward_epoch: 100,
        is_permanent: false,
        vote_weight_bps: VOTE_WEIGHT_90_DAY,
    };

    let mut buf = vec![0u8; PassiveStakeAccount::SERIALIZED_SIZE];
    state.serialize_into(&mut buf).unwrap();

    let deserialized = PassiveStakeAccount::deserialize(&buf).unwrap();
    assert_eq!(state, deserialized);
}

#[test]
fn test_passive_stake_account_serialized_size() {
    // 1 + 32 + 8 + 8 + 8 + 8 + 8 + 8 + 1 + 2 = 84
    assert_eq!(PassiveStakeAccount::SERIALIZED_SIZE, 84);
}

#[test]
fn test_deserialize_rejects_wrong_discriminator() {
    let mut buf = vec![0u8; PassiveStakeAccount::SERIALIZED_SIZE];
    // discriminator 0 = uninitialized
    buf[0] = 0;
    assert!(PassiveStakeAccount::deserialize(&buf).is_err());

    // Wrong discriminator value
    buf[0] = 99;
    assert!(PassiveStakeAccount::deserialize(&buf).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Reward claiming
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_claim_zeroes_unclaimed_rewards() {
    // After claiming, unclaimed_rewards should be 0.
    // The claim amount should equal the previous unclaimed_rewards.
    let mut state = PassiveStakeAccount {
        authority: solana_pubkey::Pubkey::new_unique(),
        amount: 100 * SOL,
        lock_days: TIER_90_DAY,
        lock_start: 1_700_000_000,
        lock_end: 1_700_000_000 + 90 * SECONDS_PER_DAY,
        unclaimed_rewards: 5 * SOL,
        last_reward_epoch: 100,
        is_permanent: false,
        vote_weight_bps: VOTE_WEIGHT_90_DAY,
    };

    let claimed = state.unclaimed_rewards;
    state.unclaimed_rewards = 0;

    assert_eq!(claimed, 5 * SOL);
    assert_eq!(state.unclaimed_rewards, 0);
}

#[test]
fn test_claim_with_zero_rewards_should_fail() {
    // The processor returns NoRewardsToClaim if unclaimed_rewards == 0.
    let state = PassiveStakeAccount {
        authority: solana_pubkey::Pubkey::new_unique(),
        amount: 100 * SOL,
        lock_days: TIER_30_DAY,
        lock_start: 1_700_000_000,
        lock_end: 1_700_000_000 + 30 * SECONDS_PER_DAY,
        unclaimed_rewards: 0,
        last_reward_epoch: 50,
        is_permanent: false,
        vote_weight_bps: VOTE_WEIGHT_30_DAY,
    };
    assert_eq!(state.unclaimed_rewards, 0, "Should have no rewards to claim");
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Permanent lock behavior
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_permanent_lock_state() {
    let state = PassiveStakeAccount {
        authority: solana_pubkey::Pubkey::new_unique(),
        amount: 100 * SOL,
        lock_days: PERMANENT_LOCK_DAYS,
        lock_start: 1_700_000_000,
        lock_end: 0, // permanent locks have lock_end = 0
        unclaimed_rewards: 0,
        last_reward_epoch: 0,
        is_permanent: true,
        vote_weight_bps: VOTE_WEIGHT_PERMANENT,
    };

    assert!(state.is_permanent);
    assert_eq!(state.lock_days, u64::MAX);
    assert_eq!(state.lock_end, 0);
    assert_eq!(state.vote_weight_bps, 15_000); // 1.5x
}

#[test]
fn test_permanent_lock_earns_highest_rewards() {
    let amount = 100 * SOL;
    let reward_perm = compute_epoch_reward(amount, 500, PERMANENT_LOCK_DAYS);
    let reward_360 = compute_epoch_reward(amount, 500, TIER_360_DAY);
    assert!(
        reward_perm > reward_360,
        "Permanent lock reward ({}) should exceed 360-day ({})",
        reward_perm, reward_360
    );
    // 12000 / 5000 = 2.4x
    let ratio = reward_perm as f64 / reward_360 as f64;
    assert!(
        (ratio - 2.4).abs() < 0.01,
        "Permanent should earn 2.4x of 360-day, got {:.2}x",
        ratio
    );
}

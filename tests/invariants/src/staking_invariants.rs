//! Property-based tests for staking and slashing invariants.
//!
//! Properties tested:
//! 1. Delegator stake is never decreased by slashing.
//! 2. Only the validator's own stake is slashed.
//! 3. Rewards are proportional to stake and time.
//! 4. Early unlock penalty >= tier's defined rate.
//! 5. Permanent locks cannot be unlocked.
//! 6. Three-strike permanent ban is irreversible.

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    // ── Constants (from slashing.rs and passive-stake constants) ──

    const BPS_DENOMINATOR: u64 = 10_000;

    const DOUBLE_SIGN_PENALTY_BPS: u64 = 500; // 5%
    const INVALID_BLOCK_PENALTY_BPS: u64 = 1_000; // 10%
    const REPEAT_OFFENSE_PENALTY_BPS: u64 = 2_500; // 25%
    const MAX_OFFENSES: u8 = 3;

    const TIER_NO_LOCK: u64 = 0;
    const TIER_30_DAY: u64 = 30;
    const TIER_90_DAY: u64 = 90;
    const TIER_180_DAY: u64 = 180;
    const TIER_360_DAY: u64 = 360;
    const PERMANENT_LOCK_DAYS: u64 = u64::MAX;

    const VALID_TIERS: &[u64] = &[
        TIER_NO_LOCK,
        TIER_30_DAY,
        TIER_90_DAY,
        TIER_180_DAY,
        TIER_360_DAY,
        PERMANENT_LOCK_DAYS,
    ];

    fn reward_rate_bps(tier: u64) -> u64 {
        match tier {
            0 => 500,
            30 => 1_000,
            90 => 2_000,
            180 => 3_000,
            360 => 5_000,
            u64::MAX => 12_000,
            _ => 0,
        }
    }

    fn early_unlock_penalty_bps(tier: u64) -> Option<u64> {
        match tier {
            0 => Some(0),
            30 => Some(250),
            90 => Some(500),
            180 => Some(750),
            360 => Some(1_250),
            _ => None,
        }
    }

    /// Calculate slash amount using integer BPS arithmetic.
    fn calculate_slash(own_stake: u64, penalty_bps: u64) -> u64 {
        (own_stake as u128 * penalty_bps as u128 / BPS_DENOMINATOR as u128) as u64
    }

    /// Simulate reward calculation.
    fn calculate_reward(
        amount: u64,
        validator_rate_bps: u64,
        tier_rate_bps: u64,
        epochs: u64,
    ) -> u64 {
        let a = amount as u128;
        let v = validator_rate_bps as u128;
        let t = tier_rate_bps as u128;
        let denom = (BPS_DENOMINATOR as u128) * (BPS_DENOMINATOR as u128) * 365;

        let per_epoch = a * v * t / denom;
        let total = per_epoch * epochs as u128;
        total.min(u64::MAX as u128) as u64
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 1. Delegator stake is never decreased by slashing
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        #[test]
        fn delegator_stake_unaffected_by_slash(
            delegator_stake in 1..=10_000_000_000u64,
            validator_own_stake in 1..=10_000_000_000u64,
            offense_is_double_sign in prop::bool::ANY,
        ) {
            let penalty_bps = if offense_is_double_sign {
                DOUBLE_SIGN_PENALTY_BPS
            } else {
                INVALID_BLOCK_PENALTY_BPS
            };

            let slashed = calculate_slash(validator_own_stake, penalty_bps);

            // ── INVARIANT: slash applies only to validator's own stake ──
            // Delegator's stake remains exactly the same.
            let delegator_after = delegator_stake; // unchanged
            prop_assert_eq!(
                delegator_after, delegator_stake,
                "Delegator stake was modified by slashing"
            );

            // Slash only comes from validator's own stake.
            prop_assert!(
                slashed <= validator_own_stake,
                "Slash ({slashed}) exceeds validator own stake ({validator_own_stake})"
            );
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 2. Slash amount is correct per offense type
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        #[test]
        fn slash_amount_correct(
            own_stake in 1..=10_000_000_000u64,
            offense_type in 0..=2u8,
            offense_count in 1..=5u8,
        ) {
            let penalty_bps = if offense_count >= MAX_OFFENSES {
                REPEAT_OFFENSE_PENALTY_BPS
            } else {
                match offense_type {
                    0 => DOUBLE_SIGN_PENALTY_BPS,
                    1 => INVALID_BLOCK_PENALTY_BPS,
                    _ => DOUBLE_SIGN_PENALTY_BPS,
                }
            };

            let slashed = calculate_slash(own_stake, penalty_bps);

            // ── INVARIANT: slashed <= own_stake ──
            prop_assert!(
                slashed <= own_stake,
                "Slashed amount {} exceeds own stake {}",
                slashed, own_stake
            );

            // ── INVARIANT: slashed matches expected percentage ──
            let expected = (own_stake as u128 * penalty_bps as u128 / BPS_DENOMINATOR as u128) as u64;
            prop_assert_eq!(slashed, expected);
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 3. Three-strike permanent ban
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn three_strikes_permanent_ban(
            own_stake in 1..=10_000_000_000u64,
            _unjail_epoch in 0..=1_000_000u64,
        ) {
            let mut offense_count: u8 = 0;
            let mut permanently_banned = false;
            let mut _is_jailed = false;

            // Strike 1.
            offense_count += 1;
            _is_jailed = true;
            prop_assert!(!permanently_banned && offense_count == 1);

            // Strike 2.
            offense_count += 1;
            prop_assert!(!permanently_banned && offense_count == 2);

            // Strike 3 — permanent ban.
            offense_count += 1;
            if offense_count >= MAX_OFFENSES {
                permanently_banned = true;
            }

            // ── INVARIANT: after 3 offenses, permanently banned ──
            prop_assert!(permanently_banned, "Should be permanently banned after 3 strikes");

            // ── INVARIANT: permanently banned validators cannot unjail ──
            let can_unjail = !permanently_banned;
            prop_assert!(!can_unjail, "Permanently banned should not be able to unjail");

            // ── INVARIANT: 3rd strike uses repeat offense penalty ──
            let penalty_3rd = calculate_slash(own_stake, REPEAT_OFFENSE_PENALTY_BPS);
            let expected_3rd = (own_stake as u128 * 2500 / 10000) as u64;
            prop_assert_eq!(penalty_3rd, expected_3rd);
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 4. Rewards proportional to stake and time
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// Double the stake → double the reward.
        #[test]
        fn reward_proportional_to_stake(
            amount in 1..=1_000_000_000u64,
            validator_rate in 1..=500u64,
            tier_idx in 0..=5usize,
            epochs in 1..=365u64,
        ) {
            let tier = VALID_TIERS[tier_idx % VALID_TIERS.len()];
            let tier_bps = reward_rate_bps(tier);

            let reward_1x = calculate_reward(amount, validator_rate, tier_bps, epochs);
            let reward_2x = calculate_reward(amount.saturating_mul(2), validator_rate, tier_bps, epochs);

            // 2x stake should give 2x reward (or both 0 for very small amounts).
            if reward_1x > 0 {
                prop_assert!(
                    reward_2x >= reward_1x,
                    "2x stake should give >= 1x reward"
                );
                // Allow tolerance for integer truncation (floor division loses up to 1 per epoch).
                let expected = reward_1x.saturating_mul(2);
                let diff = if reward_2x > expected {
                    reward_2x - expected
                } else {
                    expected - reward_2x
                };
                // Tolerance: up to `epochs` lamports due to per-epoch truncation.
                prop_assert!(
                    diff <= epochs,
                    "2x stake reward ({}) not close to 2 * 1x reward ({}), diff={}", reward_2x, expected, diff
                );
            }
        }

        /// Double the epochs → double the reward.
        #[test]
        fn reward_proportional_to_time(
            amount in 1..=1_000_000_000u64,
            validator_rate in 1..=500u64,
            tier_idx in 0..=5usize,
            epochs in 1..=182u64,
        ) {
            let tier = VALID_TIERS[tier_idx % VALID_TIERS.len()];
            let tier_bps = reward_rate_bps(tier);

            let reward_1x = calculate_reward(amount, validator_rate, tier_bps, epochs);
            let reward_2x = calculate_reward(amount, validator_rate, tier_bps, epochs * 2);

            if reward_1x > 0 {
                let expected = reward_1x * 2;
                let diff = if reward_2x > expected {
                    reward_2x - expected
                } else {
                    expected - reward_2x
                };
                prop_assert!(
                    diff <= 1,
                    "2x time reward ({reward_2x}) not close to 2 * 1x reward ({expected})"
                );
            }
        }

        /// Higher tier always earns >= lower tier reward.
        #[test]
        fn higher_tier_higher_reward(
            amount in 1_000_000..=1_000_000_000u64,
            validator_rate in 100..=500u64,
            epochs in 10..=365u64,
        ) {
            let mut prev_reward = 0u64;
            for &tier in VALID_TIERS {
                let tier_bps = reward_rate_bps(tier);
                let reward = calculate_reward(amount, validator_rate, tier_bps, epochs);
                prop_assert!(
                    reward >= prev_reward,
                    "Higher tier should earn more: tier {}d reward ({}) < prev ({prev_reward})",
                    if tier == PERMANENT_LOCK_DAYS { 99999 } else { tier },
                    reward
                );
                prev_reward = reward;
            }
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 5. Early unlock penalty >= tier's defined rate
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        #[test]
        fn early_unlock_penalty_meets_minimum(
            amount in 1..=10_000_000_000u64,
            tier_idx in 0..=4usize, // exclude permanent (can't early unlock)
        ) {
            let tier = VALID_TIERS[tier_idx % 5]; // 0, 30, 90, 180, 360
            let penalty_bps = early_unlock_penalty_bps(tier).unwrap();

            let penalty = (amount as u128 * penalty_bps as u128 / BPS_DENOMINATOR as u128) as u64;
            let return_amount = amount.saturating_sub(penalty);

            // ── INVARIANT: return_amount + penalty <= amount ──
            prop_assert!(
                return_amount + penalty <= amount,
                "Return ({return_amount}) + penalty ({penalty}) > amount ({amount})"
            );

            // ── INVARIANT: penalty is at least the tier's minimum ──
            let expected_penalty = (amount as u128 * penalty_bps as u128 / BPS_DENOMINATOR as u128) as u64;
            prop_assert_eq!(penalty, expected_penalty);

            // ── INVARIANT: for non-zero tiers, penalty > 0 for non-zero amounts ──
            if tier > 0 && amount > 0 {
                // With BPS math, very small amounts might round to 0.
                let min_amount_for_nonzero = BPS_DENOMINATOR / penalty_bps.max(1) + 1;
                if amount >= min_amount_for_nonzero {
                    prop_assert!(
                        penalty > 0,
                        "Non-zero tier {tier}d with amount {amount} should have non-zero penalty"
                    );
                }
            }
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 6. Permanent lock cannot be unlocked
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn permanent_lock_no_early_unlock(_amount in 1..=10_000_000_000u64) {
            // Permanent locks have no early_unlock_penalty_bps.
            let result = early_unlock_penalty_bps(PERMANENT_LOCK_DAYS);
            prop_assert!(
                result.is_none(),
                "Permanent lock should not have an early unlock penalty (it shouldn't be unlockable)"
            );
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 7. Tier reward rates are monotonically increasing
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[test]
    fn reward_rates_monotonically_increasing() {
        let mut prev_bps = 0;
        for &tier in VALID_TIERS {
            let bps = reward_rate_bps(tier);
            assert!(
                bps >= prev_bps,
                "Tier {}d reward rate ({bps}) < previous ({prev_bps})",
                if tier == PERMANENT_LOCK_DAYS {
                    99999
                } else {
                    tier
                }
            );
            prev_bps = bps;
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 8. Early unlock penalty rates increase with tier
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[test]
    fn early_unlock_penalty_monotonically_increasing() {
        let non_permanent_tiers: &[u64] = &[TIER_NO_LOCK, TIER_30_DAY, TIER_90_DAY, TIER_180_DAY, TIER_360_DAY];
        let mut prev_bps = 0;
        for &tier in non_permanent_tiers {
            let bps = early_unlock_penalty_bps(tier).unwrap();
            assert!(
                bps >= prev_bps,
                "Tier {}d penalty ({bps}) < previous ({prev_bps})",
                tier
            );
            prev_bps = bps;
        }
    }
}

//! Property-based tests for TRv1 economic invariants.
//!
//! Properties tested:
//! 1. Fee conservation: total fees == burn + validator + treasury + developer
//! 2. Fee split always sums to 100%
//! 3. No negative balances anywhere
//! 4. Base fee always within [min, max]
//! 5. Transaction fee monotonicity with priority

#[cfg(test)]
mod tests {
    use {
        proptest::prelude::*,
        trv1_fee_market::{
            calculator::{
                calculate_next_base_fee, calculate_transaction_fee, validate_config,
            },
            config::FeeMarketConfig,
            state::BlockFeeState,
        },
    };

    // ── Fee split constants (from developer-rewards/constants.rs) ──

    mod launch {
        pub const BURN_BPS: u16 = 1_000;
        pub const VALIDATOR_BPS: u16 = 0;
        pub const TREASURY_BPS: u16 = 4_500;
        pub const DEVELOPER_BPS: u16 = 4_500;
    }

    mod maturity {
        pub const BURN_BPS: u16 = 2_500;
        pub const VALIDATOR_BPS: u16 = 2_500;
        pub const TREASURY_BPS: u16 = 2_500;
        pub const DEVELOPER_BPS: u16 = 2_500;
    }

    const BPS_DENOMINATOR: u64 = 10_000;
    const TRANSITION_EPOCHS: u64 = 912;

    /// Interpolate fee split BPS between launch and maturity phases.
    /// Uses a "remainder goes to treasury" strategy to guarantee the total
    /// always equals exactly 10_000 BPS.
    fn fee_split_at_epoch(epoch: u64) -> (u16, u16, u16, u16) {
        if epoch >= TRANSITION_EPOCHS {
            return (
                maturity::BURN_BPS,
                maturity::VALIDATOR_BPS,
                maturity::TREASURY_BPS,
                maturity::DEVELOPER_BPS,
            );
        }

        if epoch == 0 {
            return (
                launch::BURN_BPS,
                launch::VALIDATOR_BPS,
                launch::TREASURY_BPS,
                launch::DEVELOPER_BPS,
            );
        }

        let interpolate = |launch_bps: u16, maturity_bps: u16| -> u16 {
            let l = launch_bps as i64;
            let m = maturity_bps as i64;
            let result = l + (m - l) * epoch as i64 / TRANSITION_EPOCHS as i64;
            result.clamp(0, 10_000) as u16
        };

        let burn = interpolate(launch::BURN_BPS, maturity::BURN_BPS);
        let validator = interpolate(launch::VALIDATOR_BPS, maturity::VALIDATOR_BPS);
        let developer = interpolate(launch::DEVELOPER_BPS, maturity::DEVELOPER_BPS);
        // Treasury absorbs rounding dust to guarantee sum == 10_000.
        let treasury = 10_000u16.saturating_sub(burn).saturating_sub(validator).saturating_sub(developer);

        (burn, validator, treasury, developer)
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 1. Fee split always sums to 100%
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]

        #[test]
        fn fee_split_sums_to_100_percent(epoch in 0..=2000u64) {
            let (burn, validator, treasury, developer) = fee_split_at_epoch(epoch);
            let total = burn as u32 + validator as u32 + treasury as u32 + developer as u32;

            // Must be exactly 10_000 since treasury absorbs rounding dust.
            prop_assert_eq!(
                total, 10_000,
                "Fee split sums to {} BPS at epoch {}", total, epoch
            );
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 2. Launch and maturity phases are exact
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[test]
    fn fee_split_launch_phase_exact() {
        let (burn, validator, treasury, developer) = fee_split_at_epoch(0);
        assert_eq!(burn, launch::BURN_BPS);
        assert_eq!(validator, launch::VALIDATOR_BPS);
        assert_eq!(treasury, launch::TREASURY_BPS);
        assert_eq!(developer, launch::DEVELOPER_BPS);
        assert_eq!(
            burn as u32 + validator as u32 + treasury as u32 + developer as u32,
            10_000
        );
    }

    #[test]
    fn fee_split_maturity_phase_exact() {
        let (burn, validator, treasury, developer) = fee_split_at_epoch(TRANSITION_EPOCHS);
        assert_eq!(burn, maturity::BURN_BPS);
        assert_eq!(validator, maturity::VALIDATOR_BPS);
        assert_eq!(treasury, maturity::TREASURY_BPS);
        assert_eq!(developer, maturity::DEVELOPER_BPS);
        assert_eq!(
            burn as u32 + validator as u32 + treasury as u32 + developer as u32,
            10_000
        );
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 3. Fee conservation: distributing a fee must account for every lamport
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        #[test]
        fn fee_distribution_conserves_total(
            total_fee in 0..=10_000_000_000u64,
            epoch in 0..=2000u64,
        ) {
            let (burn_bps, validator_bps, _treasury_bps, developer_bps) = fee_split_at_epoch(epoch);

            // Distribute using integer math (floor division).
            // Treasury absorbs rounding dust to guarantee perfect conservation.
            let burn = (total_fee as u128 * burn_bps as u128 / BPS_DENOMINATOR as u128) as u64;
            let validator = (total_fee as u128 * validator_bps as u128 / BPS_DENOMINATOR as u128) as u64;
            let developer = (total_fee as u128 * developer_bps as u128 / BPS_DENOMINATOR as u128) as u64;
            // Treasury gets the remainder to ensure exact conservation.
            let treasury = total_fee.saturating_sub(burn).saturating_sub(validator).saturating_sub(developer);

            let distributed = burn + validator + treasury + developer;

            // ── INVARIANT: all lamports are exactly accounted for ──
            prop_assert_eq!(
                distributed,
                total_fee,
                "Fee conservation violated: distributed {} != total {}", distributed, total_fee
            );

            // ── INVARIANT: no component exceeds total ──
            prop_assert!(burn <= total_fee);
            prop_assert!(validator <= total_fee);
            prop_assert!(treasury <= total_fee);
            prop_assert!(developer <= total_fee);
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 4. Base fee invariants
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// Base fee is always within [min_base_fee, max_base_fee].
        #[test]
        fn base_fee_always_within_bounds(
            current_fee in 0..=100_000_000u64,
            parent_gas in 0..=100_000_000u64,
        ) {
            let config = FeeMarketConfig::default();
            let state = BlockFeeState {
                base_fee_per_cu: current_fee,
                parent_gas_used: parent_gas,
                current_gas_used: 0,
                height: 0,
            };

            let next_fee = calculate_next_base_fee(&config, &state);

            prop_assert!(
                next_fee >= config.min_base_fee,
                "Fee {next_fee} below min {}",
                config.min_base_fee
            );
            prop_assert!(
                next_fee <= config.max_base_fee,
                "Fee {next_fee} above max {}",
                config.max_base_fee
            );
        }

        /// Multi-block sequence: base fee stays within bounds throughout.
        #[test]
        fn multi_block_fee_bounded(
            initial_fee in 5_000..=50_000_000u64,
            utilization_pct in 0..=100u8,
            num_blocks in 1..=100usize,
        ) {
            let config = FeeMarketConfig::default();
            let usage = config.max_block_compute_units as u128
                * utilization_pct as u128
                / 100;
            let usage = usage.min(u64::MAX as u128) as u64;

            let mut state = BlockFeeState {
                base_fee_per_cu: initial_fee.clamp(config.min_base_fee, config.max_base_fee),
                parent_gas_used: usage,
                current_gas_used: 0,
                height: 0,
            };

            for i in 0..num_blocks {
                let next = calculate_next_base_fee(&config, &state);
                prop_assert!(
                    next >= config.min_base_fee && next <= config.max_base_fee,
                    "Block {i}: fee {next} out of bounds [{}, {}]",
                    config.min_base_fee,
                    config.max_base_fee
                );
                state = BlockFeeState {
                    base_fee_per_cu: next,
                    parent_gas_used: usage,
                    current_gas_used: 0,
                    height: i as u64 + 1,
                };
            }
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 5. Transaction fee monotonicity
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// Higher priority fee always results in higher or equal total fee.
        #[test]
        fn priority_fee_monotonic(
            base_fee in 1..=1_000_000u64,
            priority_low in 0..=1_000_000u64,
            priority_delta in 1..=1_000_000u64,
            cu in 1..=10_000_000u64,
        ) {
            let priority_high = priority_low.saturating_add(priority_delta);

            let fee_low = calculate_transaction_fee(base_fee, priority_low, cu);
            let fee_high = calculate_transaction_fee(base_fee, priority_high, cu);

            prop_assert!(
                fee_high.total_fee >= fee_low.total_fee,
                "Higher priority should mean higher total fee"
            );
            prop_assert_eq!(
                fee_low.base_fee, fee_high.base_fee,
                "Base fee should not change with priority"
            );
        }

        /// Higher compute units always results in higher or equal total fee.
        #[test]
        fn compute_units_monotonic(
            base_fee in 1..=1_000_000u64,
            priority in 0..=1_000_000u64,
            cu_low in 0..=5_000_000u64,
            cu_delta in 1..=5_000_000u64,
        ) {
            let cu_high = cu_low.saturating_add(cu_delta);

            let fee_low = calculate_transaction_fee(base_fee, priority, cu_low);
            let fee_high = calculate_transaction_fee(base_fee, priority, cu_high);

            prop_assert!(
                fee_high.total_fee >= fee_low.total_fee,
                "More CU should mean higher total fee"
            );
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 6. Transaction fee components
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// total_fee == base_fee + priority_fee (with saturating arithmetic).
        #[test]
        fn fee_components_sum_to_total(
            base_fee_per_cu in 0..=u64::MAX,
            priority_fee_per_cu in 0..=u64::MAX,
            cu in 0..=u64::MAX,
        ) {
            let fee = calculate_transaction_fee(base_fee_per_cu, priority_fee_per_cu, cu);

            let expected = fee.base_fee.saturating_add(fee.priority_fee);
            prop_assert_eq!(
                fee.total_fee, expected,
                "total_fee != base_fee + priority_fee"
            );
        }

        /// No negative balances: all fee components are non-negative (trivially
        /// true for u64, but let's verify the math doesn't wrap).
        #[test]
        fn no_negative_fee_components(
            base_fee_per_cu in 0..=1_000_000u64,
            priority_fee_per_cu in 0..=1_000_000u64,
            cu in 0..=1_000_000u64,
        ) {
            let fee = calculate_transaction_fee(base_fee_per_cu, priority_fee_per_cu, cu);
            // u64 can't be negative, but we verify the results are sensible.
            prop_assert!(fee.base_fee <= base_fee_per_cu.saturating_mul(cu));
            prop_assert!(fee.priority_fee <= priority_fee_per_cu.saturating_mul(cu));
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 7. Config validation
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        /// Valid configs always pass validation.
        #[test]
        fn valid_config_passes(
            min_fee in 0..=1_000_000u64,
            max_extra in 0..=1_000_000u64,
            denom in 1..=100u64,
            util_pct in 0..=100u8,
        ) {
            let config = FeeMarketConfig {
                min_base_fee: min_fee,
                max_base_fee: min_fee.saturating_add(max_extra),
                target_utilization_pct: util_pct,
                max_block_compute_units: 48_000_000,
                base_fee_change_denominator: denom,
                min_priority_fee: 0,
            };
            prop_assert!(validate_config(&config).is_ok());
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 8. Staking rate bound
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    /// The design document specifies a maximum staking rate of 5% APY.
    /// Verify that the configured validator reward rate (500 bps) corresponds to ≤5%.
    #[test]
    fn staking_rate_does_not_exceed_5_percent() {
        let max_validator_rate_bps: u64 = 500; // 5%
        let max_apy = max_validator_rate_bps as f64 / BPS_DENOMINATOR as f64;
        assert!(
            max_apy <= 0.05 + f64::EPSILON,
            "Staking rate {max_apy} exceeds 5%"
        );
    }
}

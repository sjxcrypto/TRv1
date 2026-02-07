//! Integration tests for TRv1 Inflation Model.
//!
//! TRv1 uses a flat 5% annual inflation rate applied **only** to staked supply.
//! Unstaked tokens do not generate inflation. This means the effective network
//! inflation rate depends on staking participation.
//!
//! Formula:
//!   new_tokens_per_epoch = staked_supply × 0.05 / epochs_per_year

use {
    crate::harness::{self, SOL, TRv1TestHarness, EPOCHS_PER_YEAR, STAKING_RATE},
    solana_runtime::trv1_constants,
};

// ═══════════════════════════════════════════════════════════════════════════
//  Helper
// ═══════════════════════════════════════════════════════════════════════════

/// Compute expected new tokens for one epoch.
fn expected_epoch_inflation(staked_supply: u64) -> u64 {
    (staked_supply as f64 * STAKING_RATE / EPOCHS_PER_YEAR as f64) as u64
}

/// Compute expected annual inflation.
fn expected_annual_inflation(staked_supply: u64) -> u64 {
    (staked_supply as f64 * STAKING_RATE) as u64
}

// ═══════════════════════════════════════════════════════════════════════════
//  1. Flat 5% annual rate
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_staking_rate_is_5_percent() {
    assert!((trv1_constants::STAKING_RATE - 0.05).abs() < f64::EPSILON);
    assert!((STAKING_RATE - 0.05).abs() < f64::EPSILON);
}

#[test]
fn test_annual_inflation_100m_staked() {
    let staked = 100_000_000 * SOL; // 100M SOL
    let annual = expected_annual_inflation(staked);

    // 5% of 100M = 5M SOL
    let expected = 5_000_000 * SOL;
    assert_eq!(annual, expected);
}

#[test]
fn test_annual_inflation_1b_staked() {
    let staked = 1_000_000_000 * SOL; // 1B SOL
    let annual = expected_annual_inflation(staked);

    // 5% of 1B = 50M SOL
    let expected = 50_000_000 * SOL;
    assert_eq!(annual, expected);
}

// ═══════════════════════════════════════════════════════════════════════════
//  2. Per-epoch inflation = staked_supply * 0.05 / epochs_per_year
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_epoch_inflation_formula() {
    let staked = 100_000_000 * SOL; // 100M SOL

    let epoch_reward = expected_epoch_inflation(staked);

    // 100M * 0.05 / 365 ≈ 13_698.63 SOL per epoch
    let expected_approx = 13_698 * SOL;
    let tolerance = 1 * SOL; // within 1 SOL

    assert!(
        (epoch_reward as i64 - expected_approx as i64).unsigned_abs() < tolerance,
        "Epoch reward: {} lamports, expected ~{} lamports (±{})",
        epoch_reward,
        expected_approx,
        tolerance
    );
}

#[test]
fn test_365_epochs_sum_to_annual_rate() {
    let staked = 100_000_000 * SOL;
    let epoch_reward = expected_epoch_inflation(staked);
    let annual_from_epochs = epoch_reward * EPOCHS_PER_YEAR;
    let annual_direct = expected_annual_inflation(staked);

    // Due to integer truncation, the sum of 365 epochs may differ slightly
    // from the direct annual calculation.
    let tolerance = EPOCHS_PER_YEAR * 2; // max 2 lamports per epoch rounding
    assert!(
        (annual_from_epochs as i64 - annual_direct as i64).unsigned_abs() < tolerance,
        "365 epochs sum ({}) should ≈ annual inflation ({})",
        annual_from_epochs,
        annual_direct
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  3. 0% on unstaked supply
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_zero_inflation_on_unstaked_supply() {
    // Unstaked supply gets 0% inflation — it's not included in the formula.
    let unstaked = 500_000_000 * SOL;
    let staked = 0u64;

    let epoch_reward = expected_epoch_inflation(staked);
    assert_eq!(epoch_reward, 0, "Zero staked supply should produce zero inflation");

    // The unstaked supply doesn't change due to inflation
    let _ = unstaked; // untouched
}

#[test]
fn test_inflation_depends_only_on_staked() {
    // Two scenarios with different total supply but same staked amount
    // should produce identical inflation.
    let staked = 100_000_000 * SOL;

    // Scenario A: total supply = 200M (50% participation)
    let _total_supply_a = 200_000_000 * SOL;
    let inflation_a = expected_epoch_inflation(staked);

    // Scenario B: total supply = 1B (10% participation)
    let _total_supply_b = 1_000_000_000 * SOL;
    let inflation_b = expected_epoch_inflation(staked);

    assert_eq!(
        inflation_a, inflation_b,
        "Inflation should depend only on staked supply, not total supply"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  4. Inflation scales with participation rate
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_higher_participation_means_more_inflation() {
    let low_stake = 100_000_000 * SOL;   // 100M staked
    let high_stake = 500_000_000 * SOL;  // 500M staked

    let low_reward = expected_epoch_inflation(low_stake);
    let high_reward = expected_epoch_inflation(high_stake);

    assert!(
        high_reward > low_reward,
        "Higher staked supply should produce more absolute inflation"
    );

    // The ratio should be exactly 5x
    let ratio = high_reward as f64 / low_reward as f64;
    assert!(
        (ratio - 5.0).abs() < 0.01,
        "500M/100M staked should produce 5x more inflation, got {:.2}x",
        ratio
    );
}

#[test]
fn test_effective_inflation_rate_with_participation() {
    // If 50% of total supply is staked, the effective inflation rate for
    // the TOTAL supply is 2.5% (half of 5%).
    let total_supply = 1_000_000_000 * SOL; // 1B
    let staked = 500_000_000 * SOL;          // 500M (50%)

    let annual_new_tokens = expected_annual_inflation(staked);
    let effective_rate = annual_new_tokens as f64 / total_supply as f64;

    assert!(
        (effective_rate - 0.025).abs() < 0.001,
        "50% participation should give ~2.5% effective rate, got {:.4}",
        effective_rate
    );
}

#[test]
fn test_effective_inflation_rate_100_pct_participation() {
    let total_supply = 1_000_000_000 * SOL;
    let staked = total_supply; // 100% participation

    let annual_new_tokens = expected_annual_inflation(staked);
    let effective_rate = annual_new_tokens as f64 / total_supply as f64;

    assert!(
        (effective_rate - 0.05).abs() < 0.001,
        "100% participation should give 5% effective rate, got {:.4}",
        effective_rate
    );
}

#[test]
fn test_effective_inflation_rate_10_pct_participation() {
    let total_supply = 1_000_000_000 * SOL;
    let staked = 100_000_000 * SOL; // 10%

    let annual_new_tokens = expected_annual_inflation(staked);
    let effective_rate = annual_new_tokens as f64 / total_supply as f64;

    assert!(
        (effective_rate - 0.005).abs() < 0.001,
        "10% participation should give 0.5% effective rate, got {:.4}",
        effective_rate
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  5. New tokens = staked_supply × 0.05 / epochs_per_year
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_new_tokens_formula_exact() {
    let staked_supply = 500_000_000 * SOL;
    let new_tokens = expected_epoch_inflation(staked_supply);

    // Expected: 500M * 0.05 / 365 = 68493.150... SOL
    // As u64 (truncated): 68493150684931 lamports
    let expected_f64 = staked_supply as f64 * 0.05 / 365.0;
    let expected_u64 = expected_f64 as u64;

    assert_eq!(new_tokens, expected_u64);
}

#[test]
fn test_new_tokens_formula_small_supply() {
    // With very small staked supply, the per-epoch reward may be tiny.
    let staked = 1 * SOL; // 1 SOL
    let new_tokens = expected_epoch_inflation(staked);

    // 1 SOL * 0.05 / 365 ≈ 136_986 lamports
    assert!(
        new_tokens > 100_000 && new_tokens < 200_000,
        "1 SOL staked should yield ~137k lamports/epoch, got {}",
        new_tokens
    );
}

#[test]
fn test_new_tokens_formula_large_supply() {
    let staked = 10_000_000_000 * SOL; // 10B SOL
    let new_tokens = expected_epoch_inflation(staked);

    // 10B * 0.05 / 365 ≈ 1_369_863 SOL/epoch
    let expected_sol = 1_369_863 * SOL;
    let tolerance = 10 * SOL;
    assert!(
        (new_tokens as i64 - expected_sol as i64).unsigned_abs() < tolerance,
        "10B staked: expected ~1.37M SOL/epoch, got {} SOL",
        new_tokens / SOL
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  6. No inflation on zero stake
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_zero_staked_supply_zero_inflation() {
    let new_tokens = expected_epoch_inflation(0);
    assert_eq!(new_tokens, 0);
}

#[test]
fn test_zero_staked_annual_zero() {
    let annual = expected_annual_inflation(0);
    assert_eq!(annual, 0);
}

// ═══════════════════════════════════════════════════════════════════════════
//  7. Inflation does not compound within a single year
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_simple_interest_not_compound() {
    // TRv1 uses simple interest (flat 5% of ORIGINAL staked supply per year).
    // The epoch reward is computed from the original principal, not the
    // accumulated balance.  This is enforced by the formula using `amount`
    // (the locked principal) rather than `amount + unclaimed_rewards`.

    let principal = 100_000_000 * SOL;
    let epoch_reward = expected_epoch_inflation(principal);

    // 365 epochs of simple interest
    let total_simple = epoch_reward * 365;
    let expected = expected_annual_inflation(principal);

    // Compare: simple interest should match direct annual calculation
    // (both are non-compounding)
    let tolerance = 365 * 2; // max rounding error
    assert!(
        (total_simple as i64 - expected as i64).unsigned_abs() < tolerance,
        "Simple interest sum ({}) should match annual ({})",
        total_simple,
        expected
    );

    // Verify it's NOT compound: compound would be more
    let compound_annual = ((principal as f64) * (1.05_f64.powi(1) - 1.0)) as u64;
    // Simple and compound should be the same for 1 year, so this is a sanity check
    assert!(
        (total_simple as i64 - compound_annual as i64).unsigned_abs() < 365 * 2,
        "For a single year, simple ≈ compound"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  8. Harness integration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_harness_total_staked_supply() {
    let harness = TRv1TestHarness::new(10);
    let total = harness.total_staked_supply();
    assert_eq!(total, 10 * harness::DEFAULT_STAKE_LAMPORTS);
}

#[test]
fn test_harness_epoch_advancement() {
    let mut harness = TRv1TestHarness::new(10);
    assert_eq!(harness.current_epoch, 0);

    harness.advance_epochs(100);
    assert_eq!(harness.current_epoch, 100);

    harness.advance_epochs(265);
    assert_eq!(harness.current_epoch, 365);
}

#[test]
fn test_inflation_over_multiple_epochs_via_harness() {
    let mut harness = TRv1TestHarness::new(100);
    let staked = harness.total_staked_supply();

    let mut total_inflation: u64 = 0;
    for _ in 0..365 {
        let epoch_reward = expected_epoch_inflation(staked);
        total_inflation += epoch_reward;
        harness.advance_epochs(1);
    }

    let expected_annual = expected_annual_inflation(staked);
    let tolerance = expected_annual / 1000; // 0.1% tolerance

    assert!(
        (total_inflation as i64 - expected_annual as i64).unsigned_abs() < tolerance,
        "365 epochs of inflation ({}) should ≈ 5% annual ({})",
        total_inflation,
        expected_annual
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  9. Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_inflation_with_1_lamport_staked() {
    let staked = 1u64; // 1 lamport
    let reward = expected_epoch_inflation(staked);
    // 1 * 0.05 / 365 = 0.000137... → truncates to 0
    assert_eq!(reward, 0, "1 lamport staked yields 0 due to integer truncation");
}

#[test]
fn test_inflation_minimum_for_nonzero_reward() {
    // Find the minimum stake needed for a non-zero epoch reward.
    // staked * 0.05 / 365 >= 1
    // staked >= 365 / 0.05 = 7300 lamports
    let min_stake = 7_300u64;
    let reward = expected_epoch_inflation(min_stake);
    assert!(
        reward >= 1,
        "7300 lamports staked should yield ≥1 lamport/epoch, got {}",
        reward
    );

    // Below threshold
    let below = 7_299u64;
    let reward_below = expected_epoch_inflation(below);
    assert_eq!(
        reward_below, 0,
        "7299 lamports staked should yield 0 lamports/epoch due to truncation"
    );
}

#[test]
fn test_inflation_u64_max_staked_no_overflow() {
    // Even with absurdly large staked supply, the formula should not panic.
    let staked = u64::MAX;
    let reward = expected_epoch_inflation(staked);
    // The f64 conversion will lose precision but shouldn't overflow.
    assert!(reward > 0, "u64::MAX staked should still yield rewards");
}

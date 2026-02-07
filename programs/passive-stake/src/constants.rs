//! Passive staking tier definitions and constants.
//!
//! Defines reward rates, lock durations, early unlock penalties,
//! and governance voting weight multipliers for each passive staking tier.

/// Seconds per day, used for lock duration calculations.
pub const SECONDS_PER_DAY: i64 = 86_400;

/// Basis points denominator (10_000 bps = 100%).
pub const BPS_DENOMINATOR: u64 = 10_000;

/// Lock duration representing a permanent (irrevocable) lock.
pub const PERMANENT_LOCK_DAYS: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Tier lock durations (in days)
// ---------------------------------------------------------------------------

pub const TIER_NO_LOCK: u64 = 0;
pub const TIER_30_DAY: u64 = 30;
pub const TIER_90_DAY: u64 = 90;
pub const TIER_180_DAY: u64 = 180;
pub const TIER_360_DAY: u64 = 360;

// ---------------------------------------------------------------------------
// Reward rates — expressed as basis points of the validator staking rate.
//
//   Validator rate is assumed to be 5% APY.
//   "5% of validator rate" → 0.25% APY → 25 bps
//   Values below are applied per-epoch against the principal.
// ---------------------------------------------------------------------------

/// No lock: 5% of validator rate (≈ 0.25% APY) — 500 bps of validator rate.
pub const REWARD_RATE_NO_LOCK_BPS: u64 = 500;

/// 30-day: 10% of validator rate (≈ 0.50% APY) — 1_000 bps.
pub const REWARD_RATE_30_DAY_BPS: u64 = 1_000;

/// 90-day: 20% of validator rate (≈ 1.00% APY) — 2_000 bps.
pub const REWARD_RATE_90_DAY_BPS: u64 = 2_000;

/// 180-day: 30% of validator rate (≈ 1.50% APY) — 3_000 bps.
pub const REWARD_RATE_180_DAY_BPS: u64 = 3_000;

/// 360-day: 50% of validator rate (≈ 2.50% APY) — 5_000 bps.
pub const REWARD_RATE_360_DAY_BPS: u64 = 5_000;

/// Permanent: 120% of validator rate (≈ 6.00% APY) — 12_000 bps.
pub const REWARD_RATE_PERMANENT_BPS: u64 = 12_000;

// ---------------------------------------------------------------------------
// Governance voting weight multipliers (in basis points).
//
//   1x = 10_000 bps.  0 means no voting power.
// ---------------------------------------------------------------------------

pub const VOTE_WEIGHT_NO_LOCK: u16 = 0;
pub const VOTE_WEIGHT_30_DAY: u16 = 1_000;   // 0.10x
pub const VOTE_WEIGHT_90_DAY: u16 = 2_000;   // 0.20x
pub const VOTE_WEIGHT_180_DAY: u16 = 3_000;  // 0.30x
pub const VOTE_WEIGHT_360_DAY: u16 = 5_000;  // 0.50x
pub const VOTE_WEIGHT_PERMANENT: u16 = 15_000; // 1.50x

// ---------------------------------------------------------------------------
// Early-unlock penalty rates (in basis points of the principal).
//
//   Penalty = 5× the tier's reward-rate percentage.
//   No-lock has zero penalty; permanent cannot early-unlock.
// ---------------------------------------------------------------------------

pub const EARLY_UNLOCK_PENALTY_NO_LOCK_BPS: u64 = 0;
pub const EARLY_UNLOCK_PENALTY_30_DAY_BPS: u64 = 250;   // 2.5%
pub const EARLY_UNLOCK_PENALTY_90_DAY_BPS: u64 = 500;   // 5.0%
pub const EARLY_UNLOCK_PENALTY_180_DAY_BPS: u64 = 750;  // 7.5%
pub const EARLY_UNLOCK_PENALTY_360_DAY_BPS: u64 = 1_250; // 12.5%

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the reward-rate basis points for the given lock tier.
pub fn reward_rate_bps_for_tier(lock_days: u64) -> Option<u64> {
    match lock_days {
        TIER_NO_LOCK => Some(REWARD_RATE_NO_LOCK_BPS),
        TIER_30_DAY => Some(REWARD_RATE_30_DAY_BPS),
        TIER_90_DAY => Some(REWARD_RATE_90_DAY_BPS),
        TIER_180_DAY => Some(REWARD_RATE_180_DAY_BPS),
        TIER_360_DAY => Some(REWARD_RATE_360_DAY_BPS),
        PERMANENT_LOCK_DAYS => Some(REWARD_RATE_PERMANENT_BPS),
        _ => None,
    }
}

/// Returns the governance vote-weight (bps) for the given lock tier.
pub fn vote_weight_bps_for_tier(lock_days: u64) -> Option<u16> {
    match lock_days {
        TIER_NO_LOCK => Some(VOTE_WEIGHT_NO_LOCK),
        TIER_30_DAY => Some(VOTE_WEIGHT_30_DAY),
        TIER_90_DAY => Some(VOTE_WEIGHT_90_DAY),
        TIER_180_DAY => Some(VOTE_WEIGHT_180_DAY),
        TIER_360_DAY => Some(VOTE_WEIGHT_360_DAY),
        PERMANENT_LOCK_DAYS => Some(VOTE_WEIGHT_PERMANENT),
        _ => None,
    }
}

/// Returns the early-unlock penalty (bps of principal) for the given tier.
/// Returns `None` for invalid tiers or permanent locks (which cannot early-unlock).
pub fn early_unlock_penalty_bps_for_tier(lock_days: u64) -> Option<u64> {
    match lock_days {
        TIER_NO_LOCK => Some(EARLY_UNLOCK_PENALTY_NO_LOCK_BPS),
        TIER_30_DAY => Some(EARLY_UNLOCK_PENALTY_30_DAY_BPS),
        TIER_90_DAY => Some(EARLY_UNLOCK_PENALTY_90_DAY_BPS),
        TIER_180_DAY => Some(EARLY_UNLOCK_PENALTY_180_DAY_BPS),
        TIER_360_DAY => Some(EARLY_UNLOCK_PENALTY_360_DAY_BPS),
        _ => None, // permanent and invalid tiers cannot early-unlock
    }
}

/// Returns `true` if `lock_days` is a valid tier value.
pub fn is_valid_tier(lock_days: u64) -> bool {
    matches!(
        lock_days,
        TIER_NO_LOCK | TIER_30_DAY | TIER_90_DAY | TIER_180_DAY | TIER_360_DAY | PERMANENT_LOCK_DAYS
    )
}

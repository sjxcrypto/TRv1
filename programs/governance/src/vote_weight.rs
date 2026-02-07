//! Voting weight calculation for the TRv1 Governance program.
//!
//! Voting power is derived from a user's passive staking commitment.  Longer
//! lock periods grant proportionally higher voting weight, while unlocked or
//! unstaked tokens carry zero weight.
//!
//! ## Weight table
//!
//! | Commitment          | Multiplier | BPS    |
//! |---------------------|:----------:|:------:|
//! | Validators/Delegators | 1.00×    | 10 000 |
//! | No lock (0 days)    | 0×         | 0      |
//! | 30-day lock         | 0.10×      | 1 000  |
//! | 90-day lock         | 0.20×      | 2 000  |
//! | 180-day lock        | 0.30×      | 3 000  |
//! | 360-day lock        | 0.50×      | 5 000  |
//! | Permanent lock      | 1.50×      | 15 000 |
//! | Unstaked            | 0×         | 0      |

use crate::constants::{
    BPS_DENOMINATOR, PERMANENT_LOCK_DAYS, TIER_180_DAY, TIER_30_DAY, TIER_360_DAY, TIER_90_DAY,
    TIER_NO_LOCK, VOTE_WEIGHT_180_DAY_BPS, VOTE_WEIGHT_30_DAY_BPS, VOTE_WEIGHT_360_DAY_BPS,
    VOTE_WEIGHT_90_DAY_BPS, VOTE_WEIGHT_NO_LOCK_BPS, VOTE_WEIGHT_PERMANENT_BPS,
    VOTE_WEIGHT_VALIDATOR_BPS,
};

/// Source of a voter's staking commitment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StakeSource {
    /// Active validator or delegator — full 1.0× weight.
    ValidatorOrDelegator,
    /// Passive staker with a specific lock tier (in days).
    PassiveStake { lock_days: u64 },
    /// Not staked at all — zero weight.
    Unstaked,
}

/// Returns the voting-weight multiplier (in basis points) for the given
/// staking commitment.
///
/// Returns `0` for commitments that have no voting power (no-lock, unstaked).
pub fn vote_weight_bps(source: StakeSource) -> u64 {
    match source {
        StakeSource::ValidatorOrDelegator => VOTE_WEIGHT_VALIDATOR_BPS,
        StakeSource::Unstaked => 0,
        StakeSource::PassiveStake { lock_days } => match lock_days {
            TIER_NO_LOCK => VOTE_WEIGHT_NO_LOCK_BPS,
            TIER_30_DAY => VOTE_WEIGHT_30_DAY_BPS,
            TIER_90_DAY => VOTE_WEIGHT_90_DAY_BPS,
            TIER_180_DAY => VOTE_WEIGHT_180_DAY_BPS,
            TIER_360_DAY => VOTE_WEIGHT_360_DAY_BPS,
            PERMANENT_LOCK_DAYS => VOTE_WEIGHT_PERMANENT_BPS,
            // Unrecognised tier — no voting power.
            _ => 0,
        },
    }
}

/// Compute the effective voting power for a given principal amount and
/// staking commitment.
///
/// `effective_power = amount × weight_bps / 10_000`
///
/// Returns `None` on arithmetic overflow (should never happen for sane inputs).
pub fn calculate_voting_power(amount: u64, source: StakeSource) -> Option<u64> {
    let weight_bps = vote_weight_bps(source);
    if weight_bps == 0 {
        return Some(0);
    }

    let amount_128 = amount as u128;
    let weight_128 = weight_bps as u128;
    let denom = BPS_DENOMINATOR as u128;

    let power = amount_128.checked_mul(weight_128)?.checked_div(denom)?;
    u64::try_from(power).ok()
}

/// Reads the `vote_weight_bps` field from a passive-stake account's raw data
/// and computes the effective voting power.
///
/// The passive-stake account layout stores `vote_weight_bps` as a `u16` at a
/// known offset (byte 82–83 of the account data, after the discriminator).
/// The `amount` field is at bytes 33–40 (after discriminator + authority).
///
/// Returns `Some((amount, effective_power))` on success, `None` if the data is
/// too short or the weight is zero.
pub fn voting_power_from_passive_stake_data(data: &[u8]) -> Option<(u64, u64)> {
    // Passive stake account layout:
    //   [0]      discriminator (1 byte, must be 1)
    //   [1..33]  authority     (32 bytes)
    //   [33..41] amount        (8 bytes, little-endian u64)
    //   [41..49] lock_days     (8 bytes)
    //   [49..57] lock_start    (8 bytes)
    //   [57..65] lock_end      (8 bytes)
    //   [65..73] unclaimed_rewards (8 bytes)
    //   [73..81] last_reward_epoch (8 bytes)
    //   [81]     is_permanent  (1 byte)
    //   [82..84] vote_weight_bps (2 bytes, little-endian u16)
    const MIN_LEN: usize = 84;
    const DISCRIMINATOR_OFFSET: usize = 0;
    const AMOUNT_OFFSET: usize = 33;
    const VOTE_WEIGHT_OFFSET: usize = 82;

    if data.len() < MIN_LEN {
        return None;
    }
    // Check discriminator (passive-stake uses 1).
    if data[DISCRIMINATOR_OFFSET] != 1 {
        return None;
    }

    let amount = u64::from_le_bytes(
        data[AMOUNT_OFFSET..AMOUNT_OFFSET + 8].try_into().ok()?,
    );
    let weight_bps = u16::from_le_bytes(
        data[VOTE_WEIGHT_OFFSET..VOTE_WEIGHT_OFFSET + 2].try_into().ok()?,
    );

    if weight_bps == 0 {
        return None; // No voting power.
    }

    let amount_128 = amount as u128;
    let weight_128 = weight_bps as u128;
    let denom = BPS_DENOMINATOR as u128;

    let power = amount_128.checked_mul(weight_128)?.checked_div(denom)?;
    let power_u64 = u64::try_from(power).ok()?;

    Some((amount, power_u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vote_weight_bps_validators() {
        assert_eq!(vote_weight_bps(StakeSource::ValidatorOrDelegator), 10_000);
    }

    #[test]
    fn test_vote_weight_bps_passive_tiers() {
        assert_eq!(vote_weight_bps(StakeSource::PassiveStake { lock_days: 0 }), 0);
        assert_eq!(vote_weight_bps(StakeSource::PassiveStake { lock_days: 30 }), 1_000);
        assert_eq!(vote_weight_bps(StakeSource::PassiveStake { lock_days: 90 }), 2_000);
        assert_eq!(vote_weight_bps(StakeSource::PassiveStake { lock_days: 180 }), 3_000);
        assert_eq!(vote_weight_bps(StakeSource::PassiveStake { lock_days: 360 }), 5_000);
        assert_eq!(
            vote_weight_bps(StakeSource::PassiveStake { lock_days: u64::MAX }),
            15_000
        );
    }

    #[test]
    fn test_vote_weight_bps_unstaked() {
        assert_eq!(vote_weight_bps(StakeSource::Unstaked), 0);
    }

    #[test]
    fn test_vote_weight_bps_invalid_tier() {
        assert_eq!(vote_weight_bps(StakeSource::PassiveStake { lock_days: 42 }), 0);
    }

    #[test]
    fn test_calculate_voting_power() {
        // 1000 tokens with 30-day lock (0.10×) = 100
        assert_eq!(
            calculate_voting_power(1_000, StakeSource::PassiveStake { lock_days: 30 }),
            Some(100)
        );

        // 1000 tokens with permanent lock (1.50×) = 1500
        assert_eq!(
            calculate_voting_power(1_000, StakeSource::PassiveStake { lock_days: u64::MAX }),
            Some(1_500)
        );

        // 1000 tokens validator (1.0×) = 1000
        assert_eq!(
            calculate_voting_power(1_000, StakeSource::ValidatorOrDelegator),
            Some(1_000)
        );

        // Unstaked = 0
        assert_eq!(
            calculate_voting_power(1_000, StakeSource::Unstaked),
            Some(0)
        );
    }

    #[test]
    fn test_voting_power_from_passive_stake_data() {
        // Build a minimal passive-stake account buffer:
        //   discriminator=1, authority=zeros(32), amount=1000 LE, ..., vote_weight_bps=5000 LE
        let mut data = vec![0u8; 84];
        data[0] = 1; // discriminator
        // authority: 32 zero bytes (1..33)
        // amount at [33..41]: 1000 = 0xe8, 0x03, ...
        data[33..41].copy_from_slice(&1_000u64.to_le_bytes());
        // lock_days at [41..49]: 360
        data[41..49].copy_from_slice(&360u64.to_le_bytes());
        // lock_start, lock_end, unclaimed, last_reward: zeros (fine for this test)
        // is_permanent at [81]: false
        data[81] = 0;
        // vote_weight_bps at [82..84]: 5000
        data[82..84].copy_from_slice(&5_000u16.to_le_bytes());

        let result = voting_power_from_passive_stake_data(&data);
        assert_eq!(result, Some((1_000, 500))); // 1000 × 5000/10000 = 500
    }

    #[test]
    fn test_voting_power_from_passive_stake_no_weight() {
        let mut data = vec![0u8; 84];
        data[0] = 1;
        data[33..41].copy_from_slice(&1_000u64.to_le_bytes());
        // vote_weight_bps = 0 → no voting power
        data[82..84].copy_from_slice(&0u16.to_le_bytes());

        assert_eq!(voting_power_from_passive_stake_data(&data), None);
    }
}

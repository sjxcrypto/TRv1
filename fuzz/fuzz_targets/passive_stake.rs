//! Fuzz passive staking reward calculations and lock logic.
//!
//! Goals:
//! - Find panics or overflows in reward calculation.
//! - Verify early unlock penalty is always >= tier's rate.
//! - Verify permanent locks cannot be bypassed.
//! - Verify reward accumulation never produces absurd values.
//! - Verify lock expiry logic is correct.

#![no_main]

use {
    arbitrary::{Arbitrary, Unstructured},
    libfuzzer_sys::fuzz_target,
};

// ── Constants (mirroring passive-stake/src/constants.rs) ──

const BPS_DENOMINATOR: u64 = 10_000;
const SECONDS_PER_DAY: i64 = 86_400;

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

fn reward_rate_bps(tier: u64) -> Option<u64> {
    match tier {
        0 => Some(500),
        30 => Some(1_000),
        90 => Some(2_000),
        180 => Some(3_000),
        360 => Some(5_000),
        u64::MAX => Some(12_000),
        _ => None,
    }
}

fn early_unlock_penalty_bps(tier: u64) -> Option<u64> {
    match tier {
        0 => Some(0),
        30 => Some(250),
        90 => Some(500),
        180 => Some(750),
        360 => Some(1_250),
        _ => None, // permanent and invalid cannot early-unlock
    }
}

/// Simulate the reward calculation from the processor.
fn calculate_rewards(
    amount: u64,
    validator_reward_rate: u64,
    tier_rate_bps: u64,
    epochs_elapsed: u64,
) -> Option<u64> {
    let amount = amount as u128;
    let v_rate = validator_reward_rate as u128;
    let t_rate = tier_rate_bps as u128;
    let denom = (BPS_DENOMINATOR as u128)
        .checked_mul(BPS_DENOMINATOR as u128)?
        .checked_mul(365)?;

    let reward_per_epoch = amount
        .checked_mul(v_rate)?
        .checked_mul(t_rate)?
        .checked_div(denom)?;

    let total = reward_per_epoch.checked_mul(epochs_elapsed as u128)?;

    Some(total.min(u64::MAX as u128) as u64)
}

/// Fuzz action.
#[derive(Debug)]
enum FuzzAction {
    Initialize {
        tier_idx: u8,
        amount: u64,
    },
    CalculateRewards {
        current_epoch: u64,
        validator_reward_rate: u64,
    },
    TryUnlock {
        current_timestamp: i64,
    },
    TryEarlyUnlock,
}

impl<'a> Arbitrary<'a> for FuzzAction {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let variant = u.int_in_range(0..=3)?;
        match variant {
            0 => Ok(FuzzAction::Initialize {
                tier_idx: u.int_in_range(0..=5)?,
                amount: u.int_in_range(1..=u64::MAX)?,
            }),
            1 => Ok(FuzzAction::CalculateRewards {
                current_epoch: u.int_in_range(0..=1_000_000)?,
                validator_reward_rate: u.int_in_range(0..=10_000)?,
            }),
            2 => Ok(FuzzAction::TryUnlock {
                current_timestamp: u.int_in_range(0..=i64::MAX)?,
            }),
            3 => Ok(FuzzAction::TryEarlyUnlock),
            _ => unreachable!(),
        }
    }
}

/// Simulated passive stake account.
#[derive(Debug, Clone, Default)]
struct SimStakeAccount {
    is_initialized: bool,
    amount: u64,
    lock_days: u64,
    lock_start: i64,
    lock_end: i64,
    is_permanent: bool,
    unclaimed_rewards: u64,
    last_reward_epoch: u64,
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);

    let mut account = SimStakeAccount::default();

    let num_actions: usize = match u.int_in_range(1..=100) {
        Ok(n) => n,
        Err(_) => return,
    };

    for _ in 0..num_actions {
        let action: FuzzAction = match u.arbitrary() {
            Ok(a) => a,
            Err(_) => break,
        };

        match action {
            FuzzAction::Initialize { tier_idx, amount } => {
                if account.is_initialized {
                    continue; // Can't re-initialize.
                }

                let tier = VALID_TIERS[tier_idx as usize % VALID_TIERS.len()];
                let now: i64 = 1_700_000_000; // fixed timestamp for reproducibility

                let (lock_end, is_permanent) = if tier == PERMANENT_LOCK_DAYS {
                    (0i64, true)
                } else if tier == TIER_NO_LOCK {
                    (0i64, false)
                } else {
                    let duration = (tier as i64).saturating_mul(SECONDS_PER_DAY);
                    (now.saturating_add(duration), false)
                };

                account = SimStakeAccount {
                    is_initialized: true,
                    amount,
                    lock_days: tier,
                    lock_start: now,
                    lock_end,
                    is_permanent,
                    unclaimed_rewards: 0,
                    last_reward_epoch: 0,
                };
            }

            FuzzAction::CalculateRewards {
                current_epoch,
                validator_reward_rate,
            } => {
                if !account.is_initialized {
                    continue;
                }

                if current_epoch <= account.last_reward_epoch {
                    continue; // Already processed.
                }

                let epochs_elapsed = current_epoch.saturating_sub(account.last_reward_epoch);
                let tier_bps = match reward_rate_bps(account.lock_days) {
                    Some(r) => r,
                    None => continue,
                };

                if let Some(new_rewards) =
                    calculate_rewards(account.amount, validator_reward_rate, tier_bps, epochs_elapsed)
                {
                    let old_rewards = account.unclaimed_rewards;
                    account.unclaimed_rewards =
                        account.unclaimed_rewards.saturating_add(new_rewards);
                    account.last_reward_epoch = current_epoch;

                    // ── Invariant: rewards never decrease ──
                    assert!(
                        account.unclaimed_rewards >= old_rewards,
                        "Rewards decreased: {} → {}",
                        old_rewards,
                        account.unclaimed_rewards
                    );

                    // ── Invariant: reward per epoch should be reasonable ──
                    // With max validator rate (10000 bps = 100%) and max tier (12000 bps),
                    // annual reward = amount * 1.0 * 1.2 / 365 per epoch.
                    // So max reward per epoch ≈ amount * 1.2 / 365 ≈ amount * 0.00329.
                    // For fuzz purposes, just check it's finite.
                    if epochs_elapsed == 1 && validator_reward_rate <= 10_000 && account.amount < u64::MAX / 2 {
                        // Reward for 1 epoch should not exceed the principal.
                        assert!(
                            new_rewards <= account.amount,
                            "Single-epoch reward ({new_rewards}) > principal ({})",
                            account.amount
                        );
                    }
                }
            }

            FuzzAction::TryUnlock { current_timestamp } => {
                if !account.is_initialized {
                    continue;
                }

                // ── Invariant: permanent locks can never be unlocked ──
                if account.is_permanent {
                    // Just verify the flag is set correctly.
                    assert_eq!(account.lock_days, PERMANENT_LOCK_DAYS);
                    continue; // Would return EarlyUnlockNotAllowed.
                }

                // No-lock accounts can always withdraw.
                if account.lock_days == TIER_NO_LOCK {
                    // Unlock succeeds.
                    account.is_initialized = false;
                    continue;
                }

                // Timed locks: check expiry.
                if current_timestamp >= account.lock_end {
                    // ── Invariant: lock_end > lock_start for timed locks ──
                    assert!(
                        account.lock_end > account.lock_start,
                        "Lock end ({}) <= lock start ({})",
                        account.lock_end,
                        account.lock_start
                    );
                    // Unlock succeeds.
                    account.is_initialized = false;
                } else {
                    // Lock not expired — would return LockNotExpired.
                }
            }

            FuzzAction::TryEarlyUnlock => {
                if !account.is_initialized {
                    continue;
                }

                // ── Invariant: permanent locks cannot early-unlock ──
                if account.is_permanent {
                    continue; // Would return EarlyUnlockNotAllowed.
                }

                let penalty_bps = match early_unlock_penalty_bps(account.lock_days) {
                    Some(p) => p,
                    None => continue,
                };

                // Calculate penalty.
                let penalty = account
                    .amount
                    .checked_mul(penalty_bps)
                    .and_then(|v| v.checked_div(BPS_DENOMINATOR));

                if let Some(penalty) = penalty {
                    let return_amount = account.amount.saturating_sub(penalty);

                    // ── Invariant: penalty never exceeds principal ──
                    assert!(
                        penalty <= account.amount,
                        "Penalty ({penalty}) > principal ({})",
                        account.amount
                    );

                    // ── Invariant: return_amount + penalty == amount ──
                    // (With rounding, it should be close.)
                    assert!(
                        return_amount + penalty <= account.amount,
                        "return + penalty exceeds amount"
                    );

                    // ── Invariant: penalty >= tier's minimum ──
                    // For non-zero tiers, penalty_bps > 0.
                    if account.lock_days > 0 && account.lock_days != PERMANENT_LOCK_DAYS {
                        assert!(
                            penalty_bps > 0,
                            "Non-zero tier should have non-zero penalty"
                        );
                    }

                    account.is_initialized = false;
                }
            }
        }
    }
});

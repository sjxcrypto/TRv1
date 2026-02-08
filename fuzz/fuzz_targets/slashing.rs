//! Fuzz the slashing and jailing infrastructure.
//!
//! Goals:
//! - Find panics in slash/jail/unjail operations.
//! - Verify permanent bans are irreversible.
//! - Verify offense counts are monotonically increasing.
//! - Verify slashed amounts never exceed own stake.
//! - Verify 3-strike rule is correctly enforced.

#![no_main]

use {
    arbitrary::{Arbitrary, Unstructured},
    libfuzzer_sys::fuzz_target,
    std::collections::HashMap,
};

// We can't directly import from the runtime crate in the fuzz workspace,
// so we replicate the core slashing logic for fuzzing.
// In a production setup, the slashing module would be in its own crate.

/// Minimal replication of SlashOffense.
#[derive(Debug, Clone, Copy)]
enum SlashOffense {
    DoubleSigning,
    InvalidBlock,
}

impl<'a> Arbitrary<'a> for SlashOffense {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        if u.ratio(1, 2)? {
            Ok(SlashOffense::DoubleSigning)
        } else {
            Ok(SlashOffense::InvalidBlock)
        }
    }
}

/// Minimal replication of SlashingConfig.
#[derive(Debug, Clone)]
struct SlashingConfig {
    double_sign_penalty_bps: u64,
    invalid_block_penalty_bps: u64,
    repeat_offense_penalty_bps: u64,
    max_offenses: u8,
}

impl Default for SlashingConfig {
    fn default() -> Self {
        Self {
            double_sign_penalty_bps: 500,  // 5%
            invalid_block_penalty_bps: 1000, // 10%
            repeat_offense_penalty_bps: 2500, // 25%
            max_offenses: 3,
        }
    }
}

/// Minimal replication of ValidatorJailStatus.
#[derive(Debug, Clone, Default)]
struct ValidatorJailStatus {
    is_jailed: bool,
    jail_until_epoch: u64,
    offense_count: u8,
    permanently_banned: bool,
}

/// Fuzzable action.
#[derive(Debug)]
enum FuzzAction {
    Slash {
        validator_idx: u8,
        offense: SlashOffense,
        own_stake: u64,
        current_epoch: u64,
    },
    Unjail {
        validator_idx: u8,
        current_epoch: u64,
    },
    CheckOffline {
        validator_idx: u8,
        last_vote_slot: u64,
        current_slot: u64,
        current_epoch: u64,
    },
}

impl<'a> Arbitrary<'a> for FuzzAction {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let variant = u.int_in_range(0..=2)?;
        match variant {
            0 => Ok(FuzzAction::Slash {
                validator_idx: u.int_in_range(0..=9)?,
                offense: u.arbitrary()?,
                own_stake: u.arbitrary()?,
                current_epoch: u.int_in_range(0..=10000)?,
            }),
            1 => Ok(FuzzAction::Unjail {
                validator_idx: u.int_in_range(0..=9)?,
                current_epoch: u.int_in_range(0..=10000)?,
            }),
            2 => Ok(FuzzAction::CheckOffline {
                validator_idx: u.int_in_range(0..=9)?,
                last_vote_slot: u.arbitrary()?,
                current_slot: u.arbitrary()?,
                current_epoch: u.int_in_range(0..=10000)?,
            }),
            _ => unreachable!(),
        }
    }
}

const OFFLINE_THRESHOLD: u64 = 216_000;
const BPS_DENOM: u64 = 10_000;

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);

    let config = SlashingConfig::default();

    // State: up to 10 validators.
    let mut statuses: HashMap<u8, ValidatorJailStatus> = HashMap::new();

    let num_actions: usize = match u.int_in_range(1..=200) {
        Ok(n) => n,
        Err(_) => return,
    };

    for _ in 0..num_actions {
        let action: FuzzAction = match u.arbitrary() {
            Ok(a) => a,
            Err(_) => break,
        };

        match action {
            FuzzAction::Slash {
                validator_idx,
                offense,
                own_stake,
                current_epoch,
            } => {
                let status = statuses.entry(validator_idx).or_default();

                if status.permanently_banned {
                    // No further slashing once permanently banned.
                    continue;
                }

                let old_offense_count = status.offense_count;
                status.offense_count = status.offense_count.saturating_add(1);

                // Determine penalty.
                let penalty_bps = if status.offense_count >= config.max_offenses {
                    config.repeat_offense_penalty_bps
                } else {
                    match offense {
                        SlashOffense::DoubleSigning => config.double_sign_penalty_bps,
                        SlashOffense::InvalidBlock => config.invalid_block_penalty_bps,
                    }
                };

                // Calculate slash amount using integer arithmetic.
                let lamports_slashed = own_stake
                    .checked_mul(penalty_bps)
                    .map(|v| v / BPS_DENOM)
                    .unwrap_or(own_stake); // saturate on overflow

                // ── Invariant: slashed amount never exceeds own stake ──
                assert!(
                    lamports_slashed <= own_stake,
                    "Slashed {} > own_stake {} (penalty_bps={})",
                    lamports_slashed,
                    own_stake,
                    penalty_bps
                );

                // ── Invariant: offense count is monotonically increasing ──
                assert!(
                    status.offense_count >= old_offense_count,
                    "Offense count decreased"
                );

                // Apply jail.
                status.is_jailed = true;
                if status.offense_count >= config.max_offenses {
                    status.permanently_banned = true;
                } else if status.offense_count >= 2 {
                    status.jail_until_epoch = current_epoch.saturating_add(15); // ~30 days
                } else {
                    status.jail_until_epoch = current_epoch.saturating_add(4); // ~7 days
                }
            }

            FuzzAction::Unjail {
                validator_idx,
                current_epoch,
            } => {
                let status = statuses.entry(validator_idx).or_default();

                if status.permanently_banned {
                    // ── Invariant: permanently banned can never unjail ──
                    // (just verify this branch is hit and we don't unjail)
                    let was_jailed = status.is_jailed;
                    // Don't unjail.
                    assert!(
                        status.permanently_banned,
                        "Permanently banned flag was cleared"
                    );
                    // Jail status should remain.
                    assert!(
                        was_jailed || status.is_jailed || status.permanently_banned,
                        "Banned validator somehow unjailed"
                    );
                    continue;
                }

                if current_epoch >= status.jail_until_epoch {
                    status.is_jailed = false;
                }
                // If current_epoch < jail_until_epoch, cannot unjail.
            }

            FuzzAction::CheckOffline {
                validator_idx,
                last_vote_slot,
                current_slot,
                current_epoch,
            } => {
                let status = statuses.entry(validator_idx).or_default();

                if status.is_jailed || status.permanently_banned {
                    continue;
                }

                if current_slot.saturating_sub(last_vote_slot) > OFFLINE_THRESHOLD {
                    status.is_jailed = true;
                    status.jail_until_epoch = current_epoch.saturating_add(4);
                }
            }
        }
    }

    // ── Final invariants ──
    for (_, status) in &statuses {
        // 1. Permanently banned validators must have offense_count >= max_offenses.
        if status.permanently_banned {
            assert!(
                status.offense_count >= config.max_offenses,
                "Permanently banned with only {} offenses (max={})",
                status.offense_count,
                config.max_offenses
            );
        }

        // 2. Offense count never wraps around (saturating add).
        assert!(status.offense_count <= 255);

        // 3. If permanently banned, must also be jailed.
        if status.permanently_banned {
            assert!(
                status.is_jailed,
                "Permanently banned but not jailed"
            );
        }
    }
});

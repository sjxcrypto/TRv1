//! TRv1 Slashing & Jailing Infrastructure
//!
//! # Design Principles
//! - **Only the validator's OWN stake** is ever slashed.  Delegators' stake
//!   accounts are **never** touched.
//! - Jailing removes a validator from the active set for a fixed duration.
//! - Unjailing is free — it just proves the validator is back online.
//! - After 3 offenses a validator is **permanently banned**.
//!
//! # Offense Types
//! | Offense          | Penalty (% of own stake) | Jail duration       |
//! |------------------|--------------------------|---------------------|
//! | Double-sign      | 5%                       | 7 days (1st), 30 days (2nd) |
//! | Invalid block    | 10%                      | 7 days (1st), 30 days (2nd) |
//! | Repeated offense | 25%                      | permanent ban       |
//! | Offline > 24 h   | —                        | auto-jail (7 days)  |

use {
    solana_pubkey::Pubkey,
    std::collections::HashMap,
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Slot-based durations (assuming ~400ms slots → 2.5 slots/sec).
/// 7 days  ≈ 604_800 seconds ≈ 1_512_000 slots
/// 30 days ≈ 2_592_000 seconds ≈ 6_480_000 slots
/// 24 hours ≈ 86_400 seconds ≈ 216_000 slots
///
/// For simplicity and easy tuning we store these as epoch counts rather than
/// raw slot counts, since jail release is checked at epoch boundaries.
/// The values below are expressed in *slots* and converted to epochs at the
/// call site.

#[derive(Debug, Clone)]
pub struct SlashingConfig {
    /// Fraction of the validator's own stake slashed on a double-sign.
    pub double_sign_penalty: f64,
    /// Fraction slashed on producing an invalid block.
    pub invalid_block_penalty: f64,
    /// Fraction slashed when offense_count reaches max_offenses.
    pub repeat_offense_penalty: f64,
    /// After this many offenses the validator is permanently banned.
    pub max_offenses: u8,
    /// First-offense jail duration in slots (~7 days).
    pub jail_duration_first: u64,
    /// Second-offense jail duration in slots (~30 days).
    pub jail_duration_second: u64,
    /// Number of consecutive slots a validator may be offline before
    /// automatic jailing kicks in (~24 hours).
    pub offline_jail_threshold: u64,
}

impl Default for SlashingConfig {
    fn default() -> Self {
        Self {
            double_sign_penalty: 0.05,
            invalid_block_penalty: 0.10,
            repeat_offense_penalty: 0.25,
            max_offenses: 3,
            jail_duration_first: 1_512_000,   // ~7 days
            jail_duration_second: 6_480_000,  // ~30 days
            offline_jail_threshold: 216_000,  // ~24 hours
        }
    }
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The kind of offense a validator committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashOffense {
    /// Signed two different blocks at the same height.
    DoubleSigning,
    /// Proposed a block that failed validation.
    InvalidBlock,
}

/// How long a validator should be jailed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JailDuration {
    /// First offense: ~7 days.
    First,
    /// Second offense: ~30 days.
    Second,
    /// Third+ offense: permanent ban.
    Permanent,
    /// Auto-jail for going offline, treated as first-offense duration.
    Offline,
}

/// Per-validator jail/ban state, stored in the runtime.
#[derive(Debug, Clone, Default)]
pub struct ValidatorJailStatus {
    pub is_jailed: bool,
    /// Epoch at which the validator may unjail. Ignored if `permanently_banned`.
    pub jail_until_epoch: u64,
    /// Running count of slashable offenses.
    pub offense_count: u8,
    /// If true the validator can never re-enter the active set.
    pub permanently_banned: bool,
    /// Last slot at which the validator was seen voting (used for offline detection).
    pub last_seen_slot: u64,
}

/// The result of a slash operation.
#[derive(Debug, Clone)]
pub struct SlashResult {
    /// Lamports removed from the validator's own stake.
    pub lamports_slashed: u64,
    /// Updated jail status.
    pub new_status: ValidatorJailStatus,
}

// ---------------------------------------------------------------------------
// State container
// ---------------------------------------------------------------------------

/// Runtime-level container for all slashing/jail state.
///
/// In a production implementation this would be persisted as a sysvar or a
/// special account.  For Phase 1 we keep it in-memory as part of Bank.
#[derive(Debug, Clone, Default)]
pub struct SlashingState {
    pub config: SlashingConfig,
    /// Map from **validator node identity** pubkey to jail status.
    pub jail_statuses: HashMap<Pubkey, ValidatorJailStatus>,
}

impl SlashingState {
    pub fn new() -> Self {
        Self {
            config: SlashingConfig::default(),
            jail_statuses: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Core operations
    // -----------------------------------------------------------------------

    /// Slash a validator for the given offense.
    ///
    /// `own_stake_lamports` is the validator's **personal** stake — not
    /// delegated stake.  This function computes the penalty and returns the
    /// amount to deduct.  The caller is responsible for actually debiting the
    /// validator's own stake account.
    ///
    /// Returns `None` if the validator is already permanently banned.
    pub fn slash_validator(
        &mut self,
        validator: &Pubkey,
        offense: SlashOffense,
        own_stake_lamports: u64,
        current_epoch: u64,
    ) -> Option<SlashResult> {
        let status = self
            .jail_statuses
            .entry(*validator)
            .or_default();

        if status.permanently_banned {
            return None;
        }

        // Increment offense count.
        status.offense_count = status.offense_count.saturating_add(1);

        // Determine penalty fraction.
        let penalty_fraction = if status.offense_count >= self.config.max_offenses {
            self.config.repeat_offense_penalty
        } else {
            match offense {
                SlashOffense::DoubleSigning => self.config.double_sign_penalty,
                SlashOffense::InvalidBlock => self.config.invalid_block_penalty,
            }
        };

        let lamports_slashed =
            (own_stake_lamports as f64 * penalty_fraction).round() as u64;

        // Determine jail duration.
        let jail_duration = if status.offense_count >= self.config.max_offenses {
            JailDuration::Permanent
        } else if status.offense_count >= 2 {
            JailDuration::Second
        } else {
            JailDuration::First
        };

        self.apply_jail(validator, jail_duration, current_epoch);

        // Re-borrow after apply_jail to get the updated status.
        let new_status = self.jail_statuses.get(validator).cloned().unwrap_or_default();

        Some(SlashResult {
            lamports_slashed,
            new_status,
        })
    }

    /// Jail a validator for the specified duration relative to `current_epoch`.
    ///
    /// The `jail_until_epoch` is set to `current_epoch + duration_in_epochs`.
    /// For `Permanent` the validator is marked `permanently_banned`.
    pub fn jail_validator(
        &mut self,
        validator: &Pubkey,
        duration: JailDuration,
        current_epoch: u64,
    ) {
        self.apply_jail(validator, duration, current_epoch);
    }

    /// Unjail a validator.  This is free — the validator just needs to send a
    /// transaction proving it's back online.
    ///
    /// Returns `true` if the validator was successfully unjailed, `false` if
    /// it is permanently banned or still within the jail window.
    pub fn unjail_validator(
        &mut self,
        validator: &Pubkey,
        current_epoch: u64,
    ) -> bool {
        let Some(status) = self.jail_statuses.get_mut(validator) else {
            return false; // nothing to unjail
        };

        if status.permanently_banned {
            return false;
        }

        if current_epoch < status.jail_until_epoch {
            return false; // still serving sentence
        }

        status.is_jailed = false;
        true
    }

    /// Scan vote accounts to detect validators that have been offline for
    /// longer than `offline_jail_threshold` slots.
    ///
    /// `last_vote_slots` maps **node identity pubkey** → most-recent slot the
    /// node voted on.  The caller derives this from the vote-account state.
    pub fn check_offline_validators(
        &mut self,
        last_vote_slots: &HashMap<Pubkey, u64>,
        current_slot: u64,
        current_epoch: u64,
    ) -> Vec<Pubkey> {
        let threshold = self.config.offline_jail_threshold;
        let mut newly_jailed = Vec::new();

        for (node_pubkey, &last_slot) in last_vote_slots {
            let status = self.jail_statuses.entry(*node_pubkey).or_default();
            status.last_seen_slot = last_slot;

            if status.is_jailed || status.permanently_banned {
                continue; // already jailed
            }

            if current_slot.saturating_sub(last_slot) > threshold {
                self.apply_jail(node_pubkey, JailDuration::Offline, current_epoch);
                newly_jailed.push(*node_pubkey);
            }
        }

        newly_jailed
    }

    /// Returns true if the node is currently jailed or permanently banned.
    pub fn is_jailed_or_banned(&self, validator: &Pubkey) -> bool {
        self.jail_statuses
            .get(validator)
            .map(|s| s.is_jailed || s.permanently_banned)
            .unwrap_or(false)
    }

    /// Collect a set of all currently jailed/banned node identities.
    pub fn jailed_set(&self) -> std::collections::HashSet<Pubkey> {
        self.jail_statuses
            .iter()
            .filter(|(_, s)| s.is_jailed || s.permanently_banned)
            .map(|(k, _)| *k)
            .collect()
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn apply_jail(
        &mut self,
        validator: &Pubkey,
        duration: JailDuration,
        current_epoch: u64,
    ) {
        let status = self.jail_statuses.entry(*validator).or_default();
        status.is_jailed = true;

        match duration {
            JailDuration::Permanent => {
                status.permanently_banned = true;
                // jail_until_epoch is irrelevant when permanently banned.
            }
            JailDuration::First | JailDuration::Offline => {
                // Convert slot-based duration to a rough epoch estimate.
                // Assume ~432_000 slots per epoch (Solana mainnet default).
                let epochs =
                    (self.config.jail_duration_first + 431_999) / 432_000;
                status.jail_until_epoch = current_epoch.saturating_add(epochs);
            }
            JailDuration::Second => {
                let epochs =
                    (self.config.jail_duration_second + 431_999) / 432_000;
                status.jail_until_epoch = current_epoch.saturating_add(epochs);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = SlashingConfig::default();
        assert_eq!(cfg.max_offenses, 3);
        assert!((cfg.double_sign_penalty - 0.05).abs() < f64::EPSILON);
        assert!((cfg.invalid_block_penalty - 0.10).abs() < f64::EPSILON);
        assert!((cfg.repeat_offense_penalty - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn test_slash_first_offense() {
        let mut state = SlashingState::new();
        let validator = Pubkey::new_unique();
        let own_stake = 1_000_000_000; // 1 SOL

        let result = state
            .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10)
            .unwrap();

        // 5% of 1 SOL = 50_000_000 lamports
        assert_eq!(result.lamports_slashed, 50_000_000);
        assert_eq!(result.new_status.offense_count, 1);
        assert!(result.new_status.is_jailed);
        assert!(!result.new_status.permanently_banned);
    }

    #[test]
    fn test_slash_invalid_block() {
        let mut state = SlashingState::new();
        let validator = Pubkey::new_unique();
        let own_stake = 1_000_000_000;

        let result = state
            .slash_validator(&validator, SlashOffense::InvalidBlock, own_stake, 10)
            .unwrap();

        // 10% of 1 SOL = 100_000_000 lamports
        assert_eq!(result.lamports_slashed, 100_000_000);
    }

    #[test]
    fn test_three_strikes_permanent_ban() {
        let mut state = SlashingState::new();
        let validator = Pubkey::new_unique();
        let own_stake = 1_000_000_000;

        // Strike 1
        let r1 = state
            .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10)
            .unwrap();
        assert_eq!(r1.new_status.offense_count, 1);
        assert!(!r1.new_status.permanently_banned);

        // Strike 2
        let r2 = state
            .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10)
            .unwrap();
        assert_eq!(r2.new_status.offense_count, 2);
        assert!(!r2.new_status.permanently_banned);

        // Strike 3 — permanent ban + 25% penalty
        let r3 = state
            .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10)
            .unwrap();
        assert_eq!(r3.lamports_slashed, 250_000_000);
        assert!(r3.new_status.permanently_banned);

        // Further slashes return None
        let r4 = state.slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10);
        assert!(r4.is_none());
    }

    #[test]
    fn test_unjail_respects_time() {
        let mut state = SlashingState::new();
        let validator = Pubkey::new_unique();
        let own_stake = 1_000_000_000;

        state
            .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10)
            .unwrap();

        // Can't unjail immediately — jail window hasn't passed.
        assert!(!state.unjail_validator(&validator, 10));

        // After enough epochs (first offense ≈ 4 epochs), should succeed.
        assert!(state.unjail_validator(&validator, 100));
    }

    #[test]
    fn test_unjail_permanently_banned() {
        let mut state = SlashingState::new();
        let validator = Pubkey::new_unique();
        let own_stake = 1_000_000_000;

        // Get permanently banned (3 strikes).
        for _ in 0..3 {
            state.slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10);
        }

        // Can never unjail.
        assert!(!state.unjail_validator(&validator, 999_999));
    }

    #[test]
    fn test_offline_detection() {
        let mut state = SlashingState::new();
        let online_validator = Pubkey::new_unique();
        let offline_validator = Pubkey::new_unique();

        let mut last_vote_slots = HashMap::new();
        let current_slot = 500_000;
        // Online: voted recently.
        last_vote_slots.insert(online_validator, current_slot - 100);
        // Offline: hasn't voted in 250_000 slots (> threshold of 216_000).
        last_vote_slots.insert(offline_validator, current_slot - 250_000);

        let jailed = state.check_offline_validators(&last_vote_slots, current_slot, 10);

        assert_eq!(jailed.len(), 1);
        assert_eq!(jailed[0], offline_validator);
        assert!(state.is_jailed_or_banned(&offline_validator));
        assert!(!state.is_jailed_or_banned(&online_validator));
    }

    #[test]
    fn test_jailed_set() {
        let mut state = SlashingState::new();
        let v1 = Pubkey::new_unique();
        let v2 = Pubkey::new_unique();

        state.jail_validator(&v1, JailDuration::First, 0);
        let jailed = state.jailed_set();
        assert!(jailed.contains(&v1));
        assert!(!jailed.contains(&v2));
    }
}

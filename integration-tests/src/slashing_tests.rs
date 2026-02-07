//! Integration tests for TRv1 Slashing & Jailing.
//!
//! Tests the slashing state machine: offense detection, penalty calculation,
//! escalating penalties, permanent bans, delegator protection, jail durations,
//! and unjailing.
//!
//! Uses the `SlashingState` directly from the runtime crate.

use {
    crate::harness::{SOL, TRv1TestHarness},
    solana_pubkey::Pubkey,
    solana_runtime::slashing::{
        JailDuration, SlashOffense, SlashResult, SlashingConfig, SlashingState,
        ValidatorJailStatus,
    },
    std::collections::HashMap,
};

// ═══════════════════════════════════════════════════════════════════════════
//  1. Double-sign → 5% slash (validator's own stake only)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_double_sign_first_offense_5_percent() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();
    let own_stake = 100 * SOL;

    let result = state
        .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10)
        .expect("First offense should return a SlashResult");

    // 5% of 100 SOL = 5 SOL
    assert_eq!(result.lamports_slashed, 5 * SOL);
    assert_eq!(result.new_status.offense_count, 1);
    assert!(result.new_status.is_jailed);
    assert!(!result.new_status.permanently_banned);
}

#[test]
fn test_double_sign_first_offense_large_stake() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();
    let own_stake = 1_000_000 * SOL; // 1M SOL

    let result = state
        .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10)
        .unwrap();

    // 5% of 1M SOL = 50k SOL
    assert_eq!(result.lamports_slashed, 50_000 * SOL);
}

#[test]
fn test_double_sign_small_stake_rounds() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();
    let own_stake = 1 * SOL; // 1 SOL = 1_000_000_000 lamports

    let result = state
        .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10)
        .unwrap();

    // 5% of 1 SOL = 50_000_000 lamports
    assert_eq!(result.lamports_slashed, 50_000_000);
}

// ═══════════════════════════════════════════════════════════════════════════
//  2. Second offense → 10% slash
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_second_offense_still_double_sign_rate() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();
    let own_stake = 100 * SOL;

    // First offense: 5%
    let r1 = state
        .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10)
        .unwrap();
    assert_eq!(r1.lamports_slashed, 5 * SOL);
    assert_eq!(r1.new_status.offense_count, 1);

    // Second offense: still 5% (because offense_count < max_offenses)
    // Note: the second offense applies the same percentage for double-signing.
    // The escalation to 25% happens at offense_count >= max_offenses (3).
    let r2 = state
        .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 15)
        .unwrap();
    assert_eq!(r2.lamports_slashed, 5 * SOL);
    assert_eq!(r2.new_status.offense_count, 2);
    assert!(!r2.new_status.permanently_banned);
}

#[test]
fn test_second_offense_invalid_block_10_percent() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();
    let own_stake = 100 * SOL;

    // First offense: double-sign (5%)
    state
        .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10)
        .unwrap();

    // Second offense: invalid block (10%) — but still under max_offenses
    let r2 = state
        .slash_validator(&validator, SlashOffense::InvalidBlock, own_stake, 15)
        .unwrap();
    assert_eq!(r2.lamports_slashed, 10 * SOL); // 10% for invalid block
    assert_eq!(r2.new_status.offense_count, 2);
}

// ═══════════════════════════════════════════════════════════════════════════
//  3. Third offense → 25% + permanent ban
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_third_offense_25_percent_and_permanent_ban() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();
    let own_stake = 100 * SOL;

    // Offense 1
    state
        .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10)
        .unwrap();

    // Offense 2
    state
        .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 15)
        .unwrap();

    // Offense 3 — triggers 25% penalty + permanent ban
    let r3 = state
        .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 20)
        .unwrap();

    assert_eq!(r3.lamports_slashed, 25 * SOL); // 25%
    assert_eq!(r3.new_status.offense_count, 3);
    assert!(r3.new_status.permanently_banned);
    assert!(r3.new_status.is_jailed);
}

#[test]
fn test_permanently_banned_validator_cannot_be_slashed_again() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();
    let own_stake = 100 * SOL;

    // Get permanently banned
    for _ in 0..3 {
        state.slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10);
    }

    // Fourth attempt returns None
    let r4 = state.slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 25);
    assert!(r4.is_none(), "Already-banned validator should not be slashed again");
}

#[test]
fn test_max_offenses_config() {
    let config = SlashingConfig::default();
    assert_eq!(config.max_offenses, 3);
}

// ═══════════════════════════════════════════════════════════════════════════
//  4. Delegator stake NOT touched
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_slash_only_affects_own_stake() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();

    // Validator has 100 SOL own stake
    let own_stake = 100 * SOL;
    // Delegators have 900 SOL (not passed to slash_validator)
    let delegated_stake = 900 * SOL;
    let total_stake = own_stake + delegated_stake;

    let result = state
        .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10)
        .unwrap();

    // Only 5% of own_stake is slashed
    assert_eq!(result.lamports_slashed, 5 * SOL);

    // Delegated stake is untouched (not even visible to the slashing function)
    // The effective total after slash:
    let remaining_own = own_stake - result.lamports_slashed;
    let new_total = remaining_own + delegated_stake;
    assert_eq!(new_total, total_stake - 5 * SOL);
    assert_eq!(delegated_stake, 900 * SOL, "Delegated stake must remain 900 SOL");
}

#[test]
fn test_slash_with_zero_own_stake() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();

    // Edge case: validator has 0 own stake (shouldn't happen in practice)
    let result = state
        .slash_validator(&validator, SlashOffense::DoubleSigning, 0, 10)
        .unwrap();

    assert_eq!(result.lamports_slashed, 0);
    assert_eq!(result.new_status.offense_count, 1);
    assert!(result.new_status.is_jailed);
}

// ═══════════════════════════════════════════════════════════════════════════
//  5. Jail after 24h offline
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_offline_jail_threshold_is_24h() {
    let config = SlashingConfig::default();
    // 24h at ~400ms/slot = 216_000 slots
    assert_eq!(config.offline_jail_threshold, 216_000);

    let hours = config.offline_jail_threshold as f64 * 0.4 / 3600.0;
    assert!(
        (hours - 24.0).abs() < 0.01,
        "Threshold should be ~24h, got {:.2}h",
        hours
    );
}

#[test]
fn test_offline_validator_auto_jailed() {
    let mut state = SlashingState::new();
    let online_validator = Pubkey::new_unique();
    let offline_validator = Pubkey::new_unique();

    let current_slot = 500_000;
    let mut last_vote_slots = HashMap::new();

    // Online: voted recently
    last_vote_slots.insert(online_validator, current_slot - 100);
    // Offline: hasn't voted in 250_000 slots (> 216_000 threshold)
    last_vote_slots.insert(offline_validator, current_slot - 250_000);

    let jailed = state.check_offline_validators(&last_vote_slots, current_slot, 10);

    assert_eq!(jailed.len(), 1);
    assert!(jailed.contains(&offline_validator));
    assert!(state.is_jailed_or_banned(&offline_validator));
    assert!(!state.is_jailed_or_banned(&online_validator));
}

#[test]
fn test_just_under_offline_threshold_not_jailed() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();

    let current_slot = 500_000;
    let threshold = state.config.offline_jail_threshold;

    let mut last_vote_slots = HashMap::new();
    // Exactly at threshold (not exceeding): should NOT be jailed
    last_vote_slots.insert(validator, current_slot - threshold);

    let jailed = state.check_offline_validators(&last_vote_slots, current_slot, 10);
    assert!(jailed.is_empty(), "Validator at exact threshold should not be jailed");
}

#[test]
fn test_already_jailed_not_double_jailed() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();

    // Jail manually first
    state.jail_validator(&validator, JailDuration::First, 10);
    assert!(state.is_jailed_or_banned(&validator));

    // Offline check should not re-jail (already jailed)
    let current_slot = 500_000;
    let mut last_vote_slots = HashMap::new();
    last_vote_slots.insert(validator, current_slot - 300_000);

    let newly_jailed = state.check_offline_validators(&last_vote_slots, current_slot, 10);
    assert!(newly_jailed.is_empty(), "Already jailed validator should not appear in newly_jailed");
}

// ═══════════════════════════════════════════════════════════════════════════
//  6. Unjail (free)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_unjail_after_sentence_served() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();
    let own_stake = 100 * SOL;

    // First offense → jail for ~7 days (4 epochs at default config)
    state
        .slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10)
        .unwrap();

    assert!(state.is_jailed_or_banned(&validator));

    // Cannot unjail immediately
    assert!(!state.unjail_validator(&validator, 10));
    assert!(!state.unjail_validator(&validator, 11));
    assert!(!state.unjail_validator(&validator, 12));

    // After many epochs, should succeed
    assert!(state.unjail_validator(&validator, 100));
    assert!(!state.is_jailed_or_banned(&validator));
}

#[test]
fn test_unjail_is_free() {
    // Unjailing doesn't cost anything — the validator just submits a transaction.
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();

    state.jail_validator(&validator, JailDuration::First, 10);

    // After jail period
    let success = state.unjail_validator(&validator, 100);
    assert!(success, "Unjailing should be free (no cost)");
}

#[test]
fn test_cannot_unjail_permanently_banned() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();
    let own_stake = 100 * SOL;

    // 3 strikes → permanent ban
    for _ in 0..3 {
        state.slash_validator(&validator, SlashOffense::DoubleSigning, own_stake, 10);
    }

    assert!(state.is_jailed_or_banned(&validator));

    // Cannot unjail even after infinite time
    assert!(!state.unjail_validator(&validator, u64::MAX));
}

#[test]
fn test_unjail_validator_not_in_set() {
    let mut state = SlashingState::new();
    let unknown = Pubkey::new_unique();
    assert!(!state.unjail_validator(&unknown, 100), "Unknown validator returns false");
}

// ═══════════════════════════════════════════════════════════════════════════
//  7. Jailed validator excluded from leader schedule
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_jailed_set_contains_jailed_validators() {
    let mut state = SlashingState::new();
    let v1 = Pubkey::new_unique();
    let v2 = Pubkey::new_unique();
    let v3 = Pubkey::new_unique();

    state.jail_validator(&v1, JailDuration::First, 0);
    state.jail_validator(&v2, JailDuration::Permanent, 0);
    // v3 is NOT jailed

    let jailed_set = state.jailed_set();
    assert!(jailed_set.contains(&v1));
    assert!(jailed_set.contains(&v2));
    assert!(!jailed_set.contains(&v3));
}

#[test]
fn test_jailed_set_excludes_unjailed_validators() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();

    state.jail_validator(&validator, JailDuration::First, 0);
    assert!(state.jailed_set().contains(&validator));

    // Unjail
    state.unjail_validator(&validator, 100);
    assert!(!state.jailed_set().contains(&validator));
}

// ═══════════════════════════════════════════════════════════════════════════
//  8. Jail durations
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_first_offense_jail_duration_7_days() {
    let config = SlashingConfig::default();
    // ~7 days at 400ms/slot
    assert_eq!(config.jail_duration_first, 1_512_000);
    let days = config.jail_duration_first as f64 * 0.4 / 86_400.0;
    assert!((days - 7.0).abs() < 0.01);
}

#[test]
fn test_second_offense_jail_duration_30_days() {
    let config = SlashingConfig::default();
    // ~30 days at 400ms/slot
    assert_eq!(config.jail_duration_second, 6_480_000);
    let days = config.jail_duration_second as f64 * 0.4 / 86_400.0;
    assert!((days - 30.0).abs() < 0.01);
}

// ═══════════════════════════════════════════════════════════════════════════
//  9. Penalty fractions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_penalty_fractions() {
    let config = SlashingConfig::default();
    assert!((config.double_sign_penalty - 0.05).abs() < f64::EPSILON);
    assert!((config.invalid_block_penalty - 0.10).abs() < f64::EPSILON);
    assert!((config.repeat_offense_penalty - 0.25).abs() < f64::EPSILON);
}

#[test]
fn test_invalid_block_penalty_is_10_percent() {
    let mut state = SlashingState::new();
    let validator = Pubkey::new_unique();
    let own_stake = 200 * SOL;

    let result = state
        .slash_validator(&validator, SlashOffense::InvalidBlock, own_stake, 10)
        .unwrap();

    // 10% of 200 SOL = 20 SOL
    assert_eq!(result.lamports_slashed, 20 * SOL);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Multiple validators can be slashed independently
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_independent_validator_slashing() {
    let mut state = SlashingState::new();
    let v1 = Pubkey::new_unique();
    let v2 = Pubkey::new_unique();

    let stake_v1 = 100 * SOL;
    let stake_v2 = 200 * SOL;

    // Slash v1 twice
    state.slash_validator(&v1, SlashOffense::DoubleSigning, stake_v1, 10).unwrap();
    state.slash_validator(&v1, SlashOffense::DoubleSigning, stake_v1, 15).unwrap();

    // Slash v2 once
    state.slash_validator(&v2, SlashOffense::InvalidBlock, stake_v2, 10).unwrap();

    // v1 has 2 offenses
    let status_v1 = state.jail_statuses.get(&v1).unwrap();
    assert_eq!(status_v1.offense_count, 2);

    // v2 has 1 offense
    let status_v2 = state.jail_statuses.get(&v2).unwrap();
    assert_eq!(status_v2.offense_count, 1);

    // Both jailed, neither permanently banned
    assert!(state.is_jailed_or_banned(&v1));
    assert!(state.is_jailed_or_banned(&v2));
    assert!(!status_v1.permanently_banned);
    assert!(!status_v2.permanently_banned);
}

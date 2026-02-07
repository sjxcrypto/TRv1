//! Constants for the TRv1 Governance program.
//!
//! Defines basis-point thresholds, voting weight multipliers, and limits used
//! throughout governance processing.

/// Basis points denominator (10_000 bps = 100%).
pub const BPS_DENOMINATOR: u64 = 10_000;

// ---------------------------------------------------------------------------
// Default governance configuration values
// ---------------------------------------------------------------------------

/// Default proposal threshold: minimum tokens (lamports) required to create a
/// proposal.  Set high enough to prevent spam but low enough for meaningful
/// participation.  50_000 SOL equivalent in lamports.
pub const DEFAULT_PROPOSAL_THRESHOLD: u64 = 50_000_000_000_000;

/// Default voting period in epochs (≈ days on TRv1).
pub const DEFAULT_VOTING_PERIOD_EPOCHS: u64 = 7;

/// Default quorum: 30% of eligible voting power must participate.
pub const DEFAULT_QUORUM_BPS: u16 = 3_000;

/// Default pass threshold: simple majority (50% of votes cast).
pub const DEFAULT_PASS_THRESHOLD_BPS: u16 = 5_000;

/// Default veto threshold: 33.3% veto votes can block a proposal.
pub const DEFAULT_VETO_THRESHOLD_BPS: u16 = 3_333;

/// Default timelock: 2 epochs (≈ 2 days) delay after passing before execution.
pub const DEFAULT_TIMELOCK_EPOCHS: u64 = 2;

// ---------------------------------------------------------------------------
// Special thresholds
// ---------------------------------------------------------------------------

/// Supermajority threshold for `EmergencyUnlock` proposals: 80%.
pub const EMERGENCY_UNLOCK_PASS_THRESHOLD_BPS: u16 = 8_000;

// ---------------------------------------------------------------------------
// Voting weight multipliers (in basis points, 10_000 = 1.0×)
//
// These match the passive staking commitment tiers exactly.
// ---------------------------------------------------------------------------

/// Validators / Delegators: 1.0× voting weight.
pub const VOTE_WEIGHT_VALIDATOR_BPS: u64 = 10_000;

/// No lock (0 days): cannot vote.
pub const VOTE_WEIGHT_NO_LOCK_BPS: u64 = 0;

/// 30-day lock: 0.10× voting weight.
pub const VOTE_WEIGHT_30_DAY_BPS: u64 = 1_000;

/// 90-day lock: 0.20× voting weight.
pub const VOTE_WEIGHT_90_DAY_BPS: u64 = 2_000;

/// 180-day lock: 0.30× voting weight.
pub const VOTE_WEIGHT_180_DAY_BPS: u64 = 3_000;

/// 360-day lock: 0.50× voting weight.
pub const VOTE_WEIGHT_360_DAY_BPS: u64 = 5_000;

/// Permanent lock: 1.50× voting weight.
pub const VOTE_WEIGHT_PERMANENT_BPS: u64 = 15_000;

/// Unstaked: cannot vote.
pub const VOTE_WEIGHT_UNSTAKED_BPS: u64 = 0;

// ---------------------------------------------------------------------------
// Lock tier day constants (mirrored from passive-stake for self-containment)
// ---------------------------------------------------------------------------

pub const TIER_NO_LOCK: u64 = 0;
pub const TIER_30_DAY: u64 = 30;
pub const TIER_90_DAY: u64 = 90;
pub const TIER_180_DAY: u64 = 180;
pub const TIER_360_DAY: u64 = 360;
pub const PERMANENT_LOCK_DAYS: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Account sizes and limits
// ---------------------------------------------------------------------------

/// Maximum proposal title length in bytes.
pub const MAX_TITLE_LEN: usize = 64;

/// Maximum memo length in bytes (for TreasurySpend).
pub const MAX_MEMO_LEN: usize = 32;

/// Maximum number of active proposals at any given time.
/// Prevents state bloat.
pub const MAX_ACTIVE_PROPOSALS: u64 = 100;

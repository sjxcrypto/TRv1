//! On-chain account state for the TRv1 Developer Rewards program.

use {
    borsh::{BorshDeserialize, BorshSerialize},
    solana_pubkey::Pubkey,
};

// ── Per-program revenue configuration ────────────────────────────────────────

/// Stored per-program — tracks who receives the developer fee share.
///
/// Derived as a PDA: `[REVENUE_CONFIG_SEED, program_id]`.
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ProgramRevenueConfig {
    /// Discriminator / version tag (allows future migrations).
    pub version: u8,

    /// The program this config belongs to.
    pub program_id: Pubkey,

    /// Primary revenue recipient address.
    pub revenue_recipient: Pubkey,

    /// Authority that can update the recipient (usually the program's upgrade
    /// authority at registration time).
    pub update_authority: Pubkey,

    /// Whether this config is active.
    pub is_active: bool,

    /// Optional: split between multiple recipients.
    /// When empty, 100 % goes to `revenue_recipient`.
    pub revenue_splits: Vec<RevenueSplit>,

    /// Total fees earned (lifetime, lamports).
    pub total_fees_earned: u64,

    /// Fees earned in the current epoch (lamports).
    pub epoch_fees_earned: u64,

    /// The epoch number for which `epoch_fees_earned` was last reset.
    pub last_epoch: u64,

    /// Slot after which this program becomes eligible for fee revenue.
    /// Set to `registration_slot + COOLDOWN_SLOTS`.
    pub eligible_after_slot: u64,

    /// Accumulated unclaimed fees (lamports).
    pub unclaimed_fees: u64,
}

/// A single entry in a multi-recipient split.
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct RevenueSplit {
    /// Recipient wallet.
    pub recipient: Pubkey,
    /// Share in basis points — all splits for a config must sum to 10 000.
    pub share_bps: u16,
}

// ── Epoch-level tracker ──────────────────────────────────────────────────────

/// Singleton account that tracks aggregate developer-fee statistics per epoch.
///
/// Derived as a PDA: `[EPOCH_TRACKER_SEED]`.
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct EpochFeeTracker {
    /// Discriminator / version tag.
    pub version: u8,

    /// The epoch these counters apply to.
    pub epoch: u64,

    /// Total developer fees distributed this epoch (lamports).
    pub total_developer_fees: u64,
}

// ── Size helpers ─────────────────────────────────────────────────────────────

impl ProgramRevenueConfig {
    /// Conservative upper-bound account size.
    /// version(1) + program_id(32) + recipient(32) + authority(32) + is_active(1)
    /// + vec_len(4) + 10 * (32 + 2) + 5 * u64(8) = 1 + 32 + 32 + 32 + 1 + 4
    ///   + 340 + 40 = 482 bytes.  We round up for safety.
    pub const MAX_SIZE: usize = 512;
}

impl EpochFeeTracker {
    /// version(1) + epoch(8) + total_developer_fees(8) = 17 bytes; round up.
    pub const MAX_SIZE: usize = 64;
}

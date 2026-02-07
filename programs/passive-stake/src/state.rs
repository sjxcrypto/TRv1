//! Account state types for the Passive Stake program.

use {
    borsh::{BorshDeserialize, BorshSerialize},
    solana_pubkey::Pubkey,
};

/// Discriminator byte written at the start of every passive-stake account
/// to distinguish it from uninitialized or foreign account data.
pub const PASSIVE_STAKE_ACCOUNT_DISCRIMINATOR: u8 = 1;

/// On-chain state for a single passive stake position.
///
/// Serialised with Borsh; the first byte of account data is the discriminator.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PassiveStakeAccount {
    /// Owner / authority of this passive stake.
    pub authority: Pubkey,

    /// Lamports locked in this position.
    pub amount: u64,

    /// Lock tier expressed in days.
    ///   0        → no lock (instant withdraw)
    ///   30 / 90 / 180 / 360 → timed lock
    ///   u64::MAX → permanent (irrevocable)
    pub lock_days: u64,

    /// Unix timestamp (seconds) when the lock was created.
    pub lock_start: i64,

    /// Unix timestamp when the lock expires.
    ///   0 for permanent locks.
    pub lock_end: i64,

    /// Accumulated rewards that have not yet been claimed.
    pub unclaimed_rewards: u64,

    /// The last epoch at which rewards were calculated for this account.
    pub last_reward_epoch: u64,

    /// Whether the lock is permanent (convenience flag, derivable from `lock_days`).
    pub is_permanent: bool,

    /// Governance voting-weight multiplier in basis points.
    ///   10_000 bps = 1.00×
    pub vote_weight_bps: u16,
}

impl PassiveStakeAccount {
    /// Returns the serialised size of a `PassiveStakeAccount` (discriminator + borsh payload).
    ///
    /// Because every field is fixed-size, the layout is deterministic:
    ///   discriminator (1)
    ///   + authority (32)
    ///   + amount (8)
    ///   + lock_days (8)
    ///   + lock_start (8)
    ///   + lock_end (8)
    ///   + unclaimed_rewards (8)
    ///   + last_reward_epoch (8)
    ///   + is_permanent (1)
    ///   + vote_weight_bps (2)
    ///   = 84 bytes
    pub const SERIALIZED_SIZE: usize = 1 + 32 + 8 + 8 + 8 + 8 + 8 + 8 + 1 + 2;

    /// Deserialise from raw account data (expects leading discriminator byte).
    pub fn deserialize(data: &[u8]) -> Result<Self, std::io::Error> {
        if data.is_empty() || data[0] != PASSIVE_STAKE_ACCOUNT_DISCRIMINATOR {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing or invalid passive-stake discriminator",
            ));
        }
        let mut cursor = &data[1..];
        BorshDeserialize::deserialize_reader(&mut cursor)
    }

    /// Serialise into raw account data (prepends discriminator byte).
    pub fn serialize_into(&self, data: &mut [u8]) -> Result<(), std::io::Error> {
        if data.len() < Self::SERIALIZED_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "account data buffer too small",
            ));
        }
        data[0] = PASSIVE_STAKE_ACCOUNT_DISCRIMINATOR;
        let mut cursor = &mut data[1..];
        BorshSerialize::serialize(self, &mut cursor)
    }
}

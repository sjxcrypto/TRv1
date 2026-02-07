//! Account state types for the Treasury program.

use {
    borsh::{BorshDeserialize, BorshSerialize},
    solana_pubkey::Pubkey,
};

/// Discriminator byte written at the start of every treasury config account
/// to distinguish it from uninitialized or foreign account data.
pub const TREASURY_CONFIG_DISCRIMINATOR: u8 = 1;

/// On-chain configuration and accounting state for the TRv1 treasury.
///
/// Serialised with Borsh; the first byte of account data is the discriminator.
///
/// There is exactly **one** TreasuryConfig account per network, created at
/// genesis via `InitializeTreasury`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TreasuryConfig {
    /// Current treasury authority (multisig at launch, governance later).
    ///
    /// Every mutating instruction requires a signature from this key.
    pub authority: Pubkey,

    /// Treasury token account (where fees accumulate).
    ///
    /// This is the lamport-holding account that receives the fee split and
    /// from which disbursements are made.
    pub treasury_account: Pubkey,

    /// Whether governance controls this treasury (`false` at launch).
    ///
    /// Flipped to `true` by the `ActivateGovernance` instruction.  This flag
    /// is informational — authority checks are identical regardless of this
    /// flag's value.  However, downstream tooling and UIs can use it to
    /// distinguish the governance era.
    pub governance_active: bool,

    /// Total lamports ever received (cumulative tracking).
    pub total_received: u64,

    /// Total lamports ever disbursed (cumulative tracking).
    pub total_disbursed: u64,

    /// Epoch at which this config was last updated.
    pub last_updated_epoch: u64,
}

impl TreasuryConfig {
    /// Returns the serialised size of a `TreasuryConfig` (discriminator + borsh payload).
    ///
    /// Layout:
    ///   discriminator   (1)
    ///   authority        (32)
    ///   treasury_account (32)
    ///   governance_active(1)
    ///   total_received   (8)
    ///   total_disbursed  (8)
    ///   last_updated_epoch(8)
    ///   = 90 bytes
    pub const SERIALIZED_SIZE: usize = 1 + 32 + 32 + 1 + 8 + 8 + 8;

    /// Deserialise from raw account data (expects leading discriminator byte).
    pub fn deserialize(data: &[u8]) -> Result<Self, std::io::Error> {
        if data.is_empty() || data[0] != TREASURY_CONFIG_DISCRIMINATOR {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing or invalid treasury config discriminator",
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
        data[0] = TREASURY_CONFIG_DISCRIMINATOR;
        let mut cursor = &mut data[1..];
        BorshSerialize::serialize(self, &mut cursor)
    }
}

//! Error types for the TRv1 Developer Rewards program.

use {
    num_derive::{FromPrimitive, ToPrimitive},
    thiserror::Error,
};

#[derive(Error, Debug, Clone, PartialEq, Eq, FromPrimitive, ToPrimitive)]
pub enum DeveloperRewardsError {
    // ── Authority / access ───────────────────────────────────────────────
    #[error("Signer is not the upgrade authority of the target program")]
    UnauthorizedSigner = 0,

    #[error("Signer is not the update authority for this revenue config")]
    UnauthorizedUpdateAuthority = 1,

    // ── State validation ─────────────────────────────────────────────────
    #[error("Revenue config already exists for this program")]
    ConfigAlreadyExists = 2,

    #[error("Revenue config not found for this program")]
    ConfigNotFound = 3,

    #[error("Revenue config is not active")]
    ConfigNotActive = 4,

    // ── Revenue-split invariants ─────────────────────────────────────────
    #[error("Revenue split shares must sum to 10 000 basis points")]
    InvalidSplitTotal = 5,

    #[error("Revenue split contains a zero-share entry")]
    ZeroShareInSplit = 6,

    #[error("Too many revenue split recipients (max 10)")]
    TooManySplitRecipients = 7,

    #[error("Duplicate recipient in revenue splits")]
    DuplicateRecipient = 8,

    // ── Anti-gaming ──────────────────────────────────────────────────────
    #[error("Program has not passed the 7-day cooldown period")]
    CooldownNotElapsed = 9,

    #[error("Transaction consumed fewer than the minimum compute units")]
    BelowMinComputeUnits = 10,

    #[error("Program has reached the per-epoch fee cap (10 %)")]
    EpochFeeCapExceeded = 11,

    // ── Claim ────────────────────────────────────────────────────────────
    #[error("No fees available to claim")]
    NoFeesToClaim = 12,

    #[error("Insufficient funds in the developer fee pool")]
    InsufficientPoolFunds = 13,

    // ── Serialization / accounts ─────────────────────────────────────────
    #[error("Failed to deserialize account data")]
    DeserializationError = 14,

    #[error("Failed to serialize account data")]
    SerializationError = 15,

    #[error("Invalid account owner")]
    InvalidAccountOwner = 16,

    #[error("Account data too small")]
    AccountDataTooSmall = 17,

    #[error("Invalid PDA derivation")]
    InvalidPda = 18,
}

impl From<DeveloperRewardsError> for solana_program_error::ProgramError {
    fn from(e: DeveloperRewardsError) -> Self {
        solana_program_error::ProgramError::Custom(e as u32)
    }
}

// Note: InstructionError conversion is provided by the blanket
// `impl<T: ToPrimitive> From<T> for InstructionError` in solana_instruction_error.

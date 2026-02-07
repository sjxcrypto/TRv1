//! Custom errors for the Passive Stake program.

use {
    num_derive::{FromPrimitive, ToPrimitive},
    thiserror::Error,
};

#[derive(Error, Debug, Clone, PartialEq, Eq, FromPrimitive, ToPrimitive)]
pub enum PassiveStakeError {
    #[error("Invalid lock tier: must be 0, 30, 90, 180, 360, or permanent (u64::MAX)")]
    InvalidLockTier = 0,

    #[error("Lock period has not yet expired")]
    LockNotExpired,

    #[error("Early unlock is not allowed for permanent locks")]
    EarlyUnlockNotAllowed,

    #[error("Account is not owned by the passive stake program")]
    InvalidAccountOwner,

    #[error("Authority did not sign the transaction")]
    MissingAuthoritySignature,

    #[error("Insufficient lamports for the requested operation")]
    InsufficientLamports,

    #[error("Arithmetic overflow in reward calculation")]
    ArithmeticOverflow,

    #[error("Passive stake account data is invalid or corrupted")]
    InvalidAccountData,

    #[error("Stake amount must be greater than zero")]
    ZeroStakeAmount,

    #[error("No rewards available to claim")]
    NoRewardsToClaim,

    #[error("Account already initialized as a passive stake account")]
    AccountAlreadyInitialized,

    #[error("Account is not rent exempt")]
    NotRentExempt,
}

// Note: InstructionError conversion is provided by the blanket
// `impl<T: ToPrimitive> From<T> for InstructionError` in solana_instruction_error.

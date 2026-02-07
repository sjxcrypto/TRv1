//! Custom errors for the Treasury program.

use {
    num_derive::{FromPrimitive, ToPrimitive},
    thiserror::Error,
};

#[derive(Error, Debug, Clone, PartialEq, Eq, FromPrimitive, ToPrimitive)]
pub enum TreasuryError {
    #[error("Treasury config account is already initialised")]
    AlreadyInitialized = 0,

    #[error("Treasury config account is not initialised")]
    NotInitialized,

    #[error("Authority did not sign the transaction")]
    MissingAuthoritySignature,

    #[error("Signer does not match the treasury authority")]
    AuthorityMismatch,

    #[error("Account is not owned by the treasury program")]
    InvalidAccountOwner,

    #[error("Insufficient lamports in treasury for disbursement")]
    InsufficientFunds,

    #[error("Treasury config account data is invalid or corrupted")]
    InvalidAccountData,

    #[error("Recipient account mismatch: instruction data does not match account at index 3")]
    RecipientMismatch,

    #[error("Disbursement amount must be greater than zero")]
    ZeroDisbursement,

    #[error("Memo exceeds maximum length of 256 bytes")]
    MemoTooLong,

    #[error("Governance is already active")]
    GovernanceAlreadyActive,

    #[error("Arithmetic overflow")]
    ArithmeticOverflow,
}

// Note: InstructionError conversion is provided by the blanket
// `impl<T: ToPrimitive> From<T> for InstructionError` in solana_instruction_error.

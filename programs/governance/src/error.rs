//! Custom errors for the TRv1 Governance program.

use {
    num_derive::{FromPrimitive, ToPrimitive},
    thiserror::Error,
};

#[derive(Error, Debug, Clone, PartialEq, Eq, FromPrimitive, ToPrimitive)]
pub enum GovernanceError {
    #[error("Governance config account is not owned by the governance program")]
    InvalidAccountOwner = 0,

    #[error("Authority did not sign the transaction")]
    MissingAuthoritySignature,

    #[error("Signer does not match the governance authority")]
    AuthorityMismatch,

    #[error("Governance config is already initialised")]
    AlreadyInitialized,

    #[error("Governance config has not been initialised")]
    NotInitialized,

    #[error("Account data is invalid or corrupted")]
    InvalidAccountData,

    #[error("Arithmetic overflow")]
    ArithmeticOverflow,

    #[error("Governance is not active — only the authority can execute changes")]
    GovernanceNotActive,

    #[error("Governance is already active")]
    GovernanceAlreadyActive,

    #[error("Insufficient staked tokens to create a proposal")]
    InsufficientStakeForProposal,

    #[error("Proposal is not in the expected status for this operation")]
    InvalidProposalStatus,

    #[error("Voting period has not ended yet")]
    VotingPeriodNotEnded,

    #[error("Voting period has ended — no more votes accepted")]
    VotingPeriodEnded,

    #[error("Proposal is still in the timelock period")]
    TimelockNotExpired,

    #[error("Proposal has already been executed")]
    ProposalAlreadyExecuted,

    #[error("Quorum was not reached")]
    QuorumNotReached,

    #[error("Pass threshold was not met")]
    PassThresholdNotMet,

    #[error("Veto threshold was reached — proposal is vetoed")]
    VetoThresholdReached,

    #[error("Only the emergency multisig can cancel proposals")]
    NotEmergencyMultisig,

    #[error("Voter has already voted on this proposal")]
    AlreadyVoted,

    #[error("Voter has no voting power (no lock or unstaked)")]
    NoVotingPower,

    #[error("Invalid vote weight proof: stake account data mismatch")]
    InvalidWeightProof,

    #[error("Proposal title exceeds maximum length")]
    TitleTooLong,

    #[error("Maximum number of active proposals reached")]
    TooManyActiveProposals,

    #[error("Invalid configuration value")]
    InvalidConfigValue,

    #[error("Only the authority (multisig) can perform this action when governance is inactive")]
    MultisigOnly,

    #[error("Invalid proposal type for this operation")]
    InvalidProposalType,

    #[error("Proposal has expired without reaching quorum")]
    ProposalExpired,
}

// Note: `InstructionError` has a blanket `From<T: ToPrimitive>` impl,
// so `GovernanceError` converts automatically via the `ToPrimitive` derive.

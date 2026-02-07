//! Instruction definitions for the TRv1 Governance program.
//!
//! All instructions are serialised / deserialised via `bincode` to stay
//! consistent with the other Agave built-in programs.
//!
//! ## Design: Unified interface
//!
//! At launch, governance is **disabled** (`is_active == false`).  The team
//! multisig submits the *same* instruction variants (e.g. `CreateProposal`,
//! `ExecuteProposal`) that full governance will use later.  The processor
//! short-circuits the voting flow when inactive — requiring only the authority
//! signature — so the multisig experience is identical to what governance
//! participants will see post-activation.

use {
    crate::state::{ProposalType, Vote},
    serde::{Deserialize, Serialize},
    solana_hash::Hash,
    solana_pubkey::Pubkey,
};

/// Instructions supported by the TRv1 Governance program.
///
/// Note: Title is transmitted as `Vec<u8>` (max 64 bytes) because serde/bincode
/// does not natively support `[u8; 64]`.  The processor copies the bytes into
/// a fixed `[u8; 64]` array, zero-padding if shorter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GovernanceInstruction {
    /// One-time initialisation of the governance config account.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer, writable]` — Initialiser (becomes initial authority).
    /// 1. `[writable]`         — Governance config account (pre-allocated,
    ///                           owned by this program, uninitialised).
    ///
    /// # Data
    ///
    /// All fields of `GovernanceConfig` except `next_proposal_id` (starts at 0).
    InitializeGovernance {
        authority: Pubkey,
        proposal_threshold: u64,
        voting_period_epochs: u64,
        quorum_bps: u16,
        pass_threshold_bps: u16,
        veto_threshold_bps: u16,
        timelock_epochs: u64,
        emergency_multisig: Pubkey,
    },

    /// Create a new proposal.
    ///
    /// When governance is **active**: requires the proposer to have at least
    /// `proposal_threshold` commitment-weighted tokens.
    ///
    /// When governance is **inactive**: requires the authority (multisig) to sign.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer]`           — Proposer (or authority if governance inactive).
    /// 1. `[writable]`         — Governance config account.
    /// 2. `[writable]`         — Proposal account (pre-allocated, uninitialised).
    /// 3. `[]`                 — Proposer's passive stake account (for weight
    ///                           proof when governance is active; ignored when
    ///                           inactive).
    CreateProposal {
        title: Vec<u8>,
        description_hash: Hash,
        proposal_type: ProposalType,
    },

    /// Cast a vote on an active proposal.
    ///
    /// Only valid when governance is **active** and the proposal status is
    /// `Active` and the current epoch is within the voting period.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer]`           — Voter.
    /// 1. `[writable]`         — Proposal account.
    /// 2. `[]`                 — Governance config account.
    /// 3. `[]`                 — Voter's passive stake account (for weight proof).
    /// 4. `[writable]`         — Vote record account (PDA derived from
    ///                           proposal_id + voter; created on first vote).
    CastVote {
        proposal_id: u64,
        vote: Vote,
    },

    /// Execute a passed proposal after the timelock has expired.
    ///
    /// When governance is **active**: anyone can crank execution once the
    /// timelock epoch has been reached.
    ///
    /// When governance is **inactive**: only the authority (multisig) can
    /// execute.  The proposal must still have been created via `CreateProposal`
    /// so the interface is identical.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer]`           — Executor (anyone if active, authority if inactive).
    /// 1. `[writable]`         — Proposal account.
    /// 2. `[writable]`         — Governance config account.
    /// 3+. (varies)            — Additional accounts required by the proposal
    ///                           type (treasury account, program buffer, etc.).
    ExecuteProposal {
        proposal_id: u64,
    },

    /// Cancel a proposal.  Only the emergency multisig can do this.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer]`           — Emergency multisig.
    /// 1. `[writable]`         — Proposal account.
    /// 2. `[]`                 — Governance config account.
    CancelProposal {
        proposal_id: u64,
    },

    /// Veto a proposal if the veto threshold has been reached.
    ///
    /// Can be called by anyone once enough veto votes have accumulated.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer]`           — Caller (anyone).
    /// 1. `[writable]`         — Proposal account.
    /// 2. `[]`                 — Governance config account.
    VetoProposal {
        proposal_id: u64,
    },

    /// Activate on-chain governance, switching from multisig-only mode to
    /// full proposal/vote/execute flow.
    ///
    /// This is a one-way transition.  Once activated, governance cannot be
    /// deactivated.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer]`           — Current authority (multisig).
    /// 1. `[writable]`         — Governance config account.
    ActivateGovernance,

    /// Update governance configuration.  Only the authority can do this.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer]`           — Current authority.
    /// 1. `[writable]`         — Governance config account.
    UpdateConfig {
        proposal_threshold: u64,
        voting_period_epochs: u64,
        quorum_bps: u16,
        pass_threshold_bps: u16,
        veto_threshold_bps: u16,
        timelock_epochs: u64,
        emergency_multisig: Pubkey,
    },
}

//! Instruction definitions for the Treasury program.
//!
//! All instructions are serialised / deserialised via `bincode` to stay
//! consistent with the other Agave built-in programs.

use {
    serde::{Deserialize, Serialize},
    solana_pubkey::Pubkey,
};

/// Instructions supported by the Treasury program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreasuryInstruction {
    /// One-time initialisation of the treasury config account.
    ///
    /// Called during genesis bootstrap.  Creates the `TreasuryConfig` state
    /// with the specified authority and treasury token account.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer, writable]` — Initialiser (must also be the proposed authority
    ///                           or an authorised genesis bootstrap key).
    /// 1. `[writable]`         — Treasury config account (pre-allocated, owned
    ///                           by this program, uninitialised).
    ///
    /// # Data
    ///
    /// * `authority`        — Pubkey that will control the treasury (multisig at launch).
    /// * `treasury_account` — Pubkey of the lamport-holding treasury account.
    InitializeTreasury {
        authority: Pubkey,
        treasury_account: Pubkey,
    },

    /// Disburse lamports from the treasury to a recipient.
    ///
    /// Requires the current authority's signature.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer]`   — Authority (must match `TreasuryConfig.authority`).
    /// 1. `[writable]`  — Treasury config account.
    /// 2. `[writable]`  — Treasury token account (source of lamports).
    /// 3. `[writable]`  — Recipient account.
    ///
    /// # Data
    ///
    /// * `amount`    — Lamports to disburse.
    /// * `recipient` — Pubkey of the recipient (must match account at index 3).
    /// * `memo`      — Human-readable reason for the disbursement (up to 256 bytes).
    Disburse {
        amount: u64,
        recipient: Pubkey,
        memo: String,
    },

    /// Transfer control of the treasury to a new authority.
    ///
    /// This is the mechanism for the multisig → governance transition.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer]`   — Current authority.
    /// 1. `[writable]`  — Treasury config account.
    ///
    /// # Data
    ///
    /// * `new_authority` — Pubkey of the new authority.
    UpdateAuthority {
        new_authority: Pubkey,
    },

    /// Flip the `governance_active` flag to `true`.
    ///
    /// Once activated, governance is considered the canonical authority.
    /// This is a one-way switch — it cannot be deactivated.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer]`   — Current authority.
    /// 1. `[writable]`  — Treasury config account.
    ActivateGovernance,
}

//! Instruction definitions for the TRv1 Developer Rewards program.

use {
    crate::state::RevenueSplit,
    borsh::{BorshDeserialize, BorshSerialize},
    solana_pubkey::Pubkey,
};

/// Instructions supported by the Developer Rewards program.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum DeveloperRewardsInstruction {
    /// Register a revenue recipient for a deployed program.
    ///
    /// Only callable by the program's **upgrade authority**.
    ///
    /// Accounts expected:
    ///   0. `[signer]`   Upgrade authority of the target program.
    ///   1. `[writable]` ProgramRevenueConfig PDA
    ///                    (`[REVENUE_CONFIG_SEED, program_id]`).
    ///   2. `[]`         The target program's programdata account
    ///                    (to verify upgrade authority).
    ///   3. `[]`         System program (for account creation).
    ///   4. `[]`         Clock sysvar.
    RegisterRevenueRecipient {
        /// The program to register.
        program_id: Pubkey,
        /// The initial revenue recipient wallet.
        recipient: Pubkey,
    },

    /// Update the primary revenue recipient for a program.
    ///
    /// Only callable by the current `update_authority`.
    ///
    /// Accounts expected:
    ///   0. `[signer]`   Current update authority.
    ///   1. `[writable]` ProgramRevenueConfig PDA.
    UpdateRevenueRecipient {
        /// The program whose recipient to change.
        program_id: Pubkey,
        /// New recipient wallet.
        new_recipient: Pubkey,
    },

    /// Set up multi-recipient revenue splits for a program.
    ///
    /// Shares must sum to 10 000 bps. Replaces any existing splits.
    /// Only callable by `update_authority`.
    ///
    /// Accounts expected:
    ///   0. `[signer]`   Current update authority.
    ///   1. `[writable]` ProgramRevenueConfig PDA.
    AddRevenueSplit {
        /// The program whose splits to update.
        program_id: Pubkey,
        /// New set of revenue splits.
        splits: Vec<RevenueSplit>,
    },

    /// Claim accumulated developer fees for a program.
    ///
    /// Transfers lamports from the developer fee pool to the configured
    /// recipient(s).
    ///
    /// Accounts expected:
    ///   0. `[signer]`   Revenue recipient (or any signer — permissionless
    ///                    claiming is safe since funds always go to the
    ///                    configured recipient).
    ///   1. `[writable]` ProgramRevenueConfig PDA.
    ///   2. `[writable]` Developer fee pool account (PDA).
    ///   3. `[writable]` Recipient account(s) — one per split entry, or the
    ///                    primary `revenue_recipient` if no splits.
    ClaimDeveloperFees {
        /// The program whose fees to claim.
        program_id: Pubkey,
    },

    /// (Runtime-only) Credit developer fees to a program after a qualifying
    /// transaction. This is invoked by the runtime fee-distribution logic,
    /// **not** by external users.
    ///
    /// Accounts expected:
    ///   0. `[signer]`   Fee-payer (runtime-injected).
    ///   1. `[writable]` ProgramRevenueConfig PDA.
    ///   2. `[writable]` EpochFeeTracker PDA.
    ///   3. `[]`         Clock sysvar.
    CreditDeveloperFees {
        /// The program being credited.
        program_id: Pubkey,
        /// Lamports to credit.
        amount: u64,
        /// Compute units the program consumed in this transaction.
        compute_units_consumed: u64,
    },
}

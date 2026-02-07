//! TRv1 Governance Program
//!
//! Manages on-chain governance for the TRv1 network: proposal creation, voting,
//! and execution with commitment-weighted voting power.
//!
//! ## Lifecycle
//!
//! Governance is **built but DISABLED at launch**.  A 5-of-7 team multisig
//! controls network parameters initially, using the *same* instruction interface
//! that governance will use later.  This guarantees a clean, zero-migration swap
//! when governance is activated.
//!
//! ### Pre-activation (multisig mode)
//!
//! - `is_active == false`
//! - Only the authority (multisig) can create proposals and execute them
//! - Proposals skip the voting phase and go straight to Timelocked → Executed
//! - The instruction format is identical to post-activation
//!
//! ### Post-activation (governance mode)
//!
//! - `is_active == true` (one-way flip via `ActivateGovernance`)
//! - Anyone with enough staked tokens can create proposals
//! - Voting is open for `voting_period_epochs`
//! - Votes are weighted by passive staking commitment
//! - Passed proposals enter a timelock before execution
//! - Emergency multisig can cancel dangerous proposals
//!
//! ## Voting Weight
//!
//! | Commitment            | Multiplier |
//! |-----------------------|:----------:|
//! | Validators/Delegators | 1.00×      |
//! | No lock               | 0× (cannot vote) |
//! | 30-day lock           | 0.10×      |
//! | 90-day lock           | 0.20×      |
//! | 180-day lock          | 0.30×      |
//! | 360-day lock          | 0.50×      |
//! | Permanent lock        | 1.50×      |
//! | Unstaked              | 0× (cannot vote) |
//!
//! ## Proposal Types
//!
//! - **ParameterChange**: modify a network parameter
//! - **TreasurySpend**: disburse funds from the treasury
//! - **EmergencyUnlock**: unlock a permanently locked account (80% supermajority)
//! - **ProgramUpgrade**: upgrade a program binary
//! - **FeatureToggle**: activate/deactivate a runtime feature
//! - **TextProposal**: signaling only, no on-chain effect

#![cfg(feature = "agave-unstable-api")]
#![allow(clippy::arithmetic_side_effects)]

pub mod constants;
pub mod error;
pub mod instruction;
pub mod processor;
pub mod state;
pub mod vote_weight;

/// Re-export the program ID.
pub use processor::id;

//! TRv1 Treasury Program
//!
//! Manages the network treasury that accumulates a portion of all transaction
//! fees.  At launch, 45% of fees flow to the treasury (declining to 25% at
//! network maturity over ~1825 daily epochs / ~5 years).
//!
//! ## Authority model
//!
//! The treasury is controlled by a single authority key:
//!
//! - **At launch**: a 5-of-7 multisig signs as the authority.
//! - **At maturity**: governance assumes control via `UpdateAuthority` followed
//!   by `ActivateGovernance`.
//!
//! The program itself is authority-agnostic — it only checks that the
//! `authority` pubkey stored in `TreasuryConfig` has signed the transaction.
//!
//! ## Instructions
//!
//! | Instruction          | Description                                      |
//! |----------------------|--------------------------------------------------|
//! | InitializeTreasury   | One-time setup: sets authority and token account  |
//! | Disburse             | Send lamports from treasury to a recipient        |
//! | UpdateAuthority      | Transfer control to a new authority key            |
//! | ActivateGovernance   | Flip the governance_active flag                   |

#![cfg(feature = "agave-unstable-api")]
#![allow(clippy::arithmetic_side_effects)]

pub mod error;
pub mod instruction;
pub mod processor;
pub mod state;

/// Re-export the program ID.
pub use processor::id;

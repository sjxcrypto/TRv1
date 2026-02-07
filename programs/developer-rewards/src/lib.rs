//! TRv1 Developer Rewards Program
//!
//! Revenue-sharing system that distributes a portion of transaction fees to the
//! deployers (or designated recipients) of smart contracts that a transaction
//! interacted with.
//!
//! # Overview
//!
//! When a transaction executes, a configurable share of its fees (the
//! "developer share") is attributed to the programs the transaction invoked.
//! Program deployers register a [`ProgramRevenueConfig`] to specify where their
//! share should be sent.  Fees accumulate on-chain and can be claimed at any
//! time.
//!
//! # Anti-gaming
//!
//! * **Minimum compute-units threshold** — a program call must consume >1 000
//!   CU to qualify.
//! * **7-day cooldown** — newly registered programs are ineligible for the
//!   first ~7 days (≈ 1 512 000 slots).
//! * **10 % per-epoch cap** — no single program may receive more than 10 % of
//!   total developer fees in one epoch.

#![cfg(feature = "agave-unstable-api")]
#![allow(clippy::arithmetic_side_effects)]

pub mod constants;
pub mod error;
pub mod instruction;
pub mod processor;
pub mod state;

/// Re-export the program ID.
pub use processor::id;

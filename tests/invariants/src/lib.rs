//! TRv1 Property-Based Invariant Tests
//!
//! Uses proptest to verify critical system invariants across:
//! - Consensus safety and liveness properties
//! - Economic supply and fee conservation
//! - Staking slash and reward correctness

pub mod consensus_invariants;
pub mod economic_invariants;
pub mod staking_invariants;

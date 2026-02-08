//! TRv1 End-to-End Test Suite
//!
//! Simulates real network behaviour by orchestrating multiple validators,
//! consensus rounds, fee dynamics, staking, governance, treasury operations,
//! and stress scenarios — all without a running validator process.
//!
//! Each test file can be run independently:
//!
//! ```bash
//! cargo test -p trv1-e2e-tests --test basic_network -- --nocapture
//! cargo test -p trv1-e2e-tests --test fee_lifecycle -- --nocapture
//! cargo test -p trv1-e2e-tests --test passive_staking_lifecycle -- --nocapture
//! cargo test -p trv1-e2e-tests --test validator_lifecycle -- --nocapture
//! cargo test -p trv1-e2e-tests --test governance_lifecycle -- --nocapture
//! cargo test -p trv1-e2e-tests --test treasury_lifecycle -- --nocapture
//! cargo test -p trv1-e2e-tests --test network_stress -- --nocapture
//! ```

pub mod helpers;

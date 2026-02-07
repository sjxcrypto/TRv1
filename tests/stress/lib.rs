//! TRv1 Stress Test Suite
//!
//! Standalone stress tests for TRv1 subsystems.
//! Each test file can be run independently.
//!
//! ```bash
//! cargo test -p trv1-stress-tests --test high_tx_throughput -- --nocapture
//! cargo test -p trv1-stress-tests --test validator_churn -- --nocapture
//! cargo test -p trv1-stress-tests --test state_growth -- --nocapture
//! cargo test -p trv1-stress-tests --test fee_spike -- --nocapture
//! cargo test -p trv1-stress-tests --test epoch_transition -- --nocapture
//! ```

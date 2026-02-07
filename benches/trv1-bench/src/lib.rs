//! TRv1 Benchmark Suite
//!
//! This crate contains performance benchmarks for all TRv1 subsystems.
//!
//! Run all benchmarks:
//! ```bash
//! cargo bench -p trv1-bench
//! ```
//!
//! Run a specific benchmark group:
//! ```bash
//! cargo bench -p trv1-bench --bench consensus_bench
//! cargo bench -p trv1-bench --bench fee_market_bench
//! cargo bench -p trv1-bench --bench cache_bench
//! cargo bench -p trv1-bench --bench staking_bench
//! cargo bench -p trv1-bench --bench rent_bench
//! ```

pub mod helpers;

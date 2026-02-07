//! # TRv1 Fee Market
//!
//! An **EIP-1559-style dynamic fee market** for the TRv1 blockchain.
//!
//! Instead of Solana's fixed `lamports_per_signature` model, TRv1 adjusts a
//! **base fee per compute unit** every block based on network utilization.
//! Users additionally specify a **priority fee** (tip) that goes to the block
//! producer, exactly like Ethereum's `maxPriorityFeePerGas`.
//!
//! ## Quick start
//!
//! ```rust
//! use trv1_fee_market::{FeeMarketConfig, BlockFeeState, calculator};
//!
//! let config = FeeMarketConfig::default();
//!
//! // Simulate: the parent block used 36 M CU (above the 24 M target).
//! let state = BlockFeeState {
//!     base_fee_per_cu: config.min_base_fee,
//!     parent_gas_used: 36_000_000,
//!     current_gas_used: 0,
//!     height: 0,
//! };
//!
//! // Derive the next block's base fee.
//! let next_fee = calculator::calculate_next_base_fee(&config, &state);
//! assert!(next_fee > state.base_fee_per_cu, "base fee should rise");
//!
//! // Price a transaction.
//! let tx_fee = calculator::calculate_transaction_fee(next_fee, /*priority*/ 100, /*cu*/ 200_000);
//! println!("total fee = {} lamports", tx_fee.total_fee);
//! ```
//!
//! See [`calculator`] for the full EIP-1559 formula and [`config`] for tunables.

pub mod calculator;
pub mod config;
pub mod error;
pub mod state;

#[cfg(test)]
mod tests;

// Re-exports for convenience.
pub use config::FeeMarketConfig;
pub use error::FeeError;
pub use state::{BlockFeeState, TransactionFee};

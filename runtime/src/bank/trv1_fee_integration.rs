//! TRv1 EIP-1559 fee market integration with the Bank.
//!
//! This module wires the standalone `trv1-fee-market` crate into the transaction
//! processing pipeline. It provides three entry points called at different
//! stages of block processing:
//!
//! 1. **`update_base_fee_for_new_block`** — called when a new bank/slot is
//!    created (from parent). Derives the next base fee from the parent's
//!    utilization and stores it in the bank.
//!
//! 2. **`validate_trv1_fee`** — called during transaction validation to ensure
//!    the payer can afford the EIP-1559 base fee + priority fee.
//!
//! 3. **`record_transaction_compute`** — called after a transaction executes to
//!    record its compute-unit usage into the block's running total.
//!
//! 4. **`finalize_block_fees`** — called at end of block (freeze) to seal the
//!    fee state for serialization.

use {
    super::Bank,
    trv1_fee_market::{
        calculator::{calculate_next_base_fee, calculate_transaction_fee, validate_transaction_fee},
        BlockFeeState, FeeMarketConfig,
        state::TransactionFee,
        FeeError,
    },
    log::info,
};

/// Called at the start of each new block to update the base fee.
///
/// Reads the parent bank's fee state (gas used, base fee) and computes
/// the new base fee using the EIP-1559 algorithm. Stores the result
/// in the child bank's `trv1_fee_state`.
pub fn update_base_fee_for_new_block(bank: &Bank, parent: &Bank) {
    let config = FeeMarketConfig::default();
    let parent_state = parent.trv1_fee_state.read().unwrap();

    let next_base_fee = calculate_next_base_fee(&config, &parent_state);
    let next_state = parent_state.next_block(next_base_fee, bank.slot());

    info!(
        "TRv1 fee market: slot={} base_fee={} (parent_gas_used={}, parent_base_fee={})",
        bank.slot(),
        next_base_fee,
        parent_state.current_gas_used,
        parent_state.base_fee_per_cu,
    );

    let mut fee_state = bank.trv1_fee_state.write().unwrap();
    *fee_state = next_state;
}

/// Get the current base fee per compute unit from the bank.
pub fn get_current_base_fee(bank: &Bank) -> u64 {
    bank.trv1_fee_state.read().unwrap().base_fee_per_cu
}

/// Get a copy of the full block fee state.
pub fn get_block_fee_state(bank: &Bank) -> BlockFeeState {
    *bank.trv1_fee_state.read().unwrap()
}

/// Validate that a transaction can afford the EIP-1559 fee at the current base fee.
///
/// Parameters:
/// - `bank`: the current bank (to read base fee)
/// - `offered_lamports`: the fee payer's available balance (or declared max fee)
/// - `priority_fee_per_cu`: the user's chosen priority fee per CU
/// - `requested_cu`: compute units the transaction requests
///
/// Returns a `TransactionFee` breakdown on success, or a `FeeError` on failure.
pub fn validate_trv1_fee(
    bank: &Bank,
    offered_lamports: u64,
    priority_fee_per_cu: u64,
    requested_cu: u64,
) -> Result<TransactionFee, FeeError> {
    let config = FeeMarketConfig::default();
    let base_fee_per_cu = get_current_base_fee(bank);

    validate_transaction_fee(
        offered_lamports,
        priority_fee_per_cu,
        base_fee_per_cu,
        requested_cu,
        &config,
    )
}

/// Record compute units consumed by a transaction in the current block.
///
/// Called after a transaction successfully executes to update the running
/// gas tally used for the next block's base fee calculation.
pub fn record_transaction_compute(bank: &Bank, compute_units_used: u64) {
    let mut fee_state = bank.trv1_fee_state.write().unwrap();
    fee_state.record_gas(compute_units_used);
}

/// Called at end of block to finalize fee state.
///
/// Currently a no-op since the fee state is already updated incrementally.
/// Reserved for future use (e.g., writing fee state to a sysvar, emitting
/// metrics, etc.).
pub fn finalize_block_fees(bank: &Bank) {
    let fee_state = bank.trv1_fee_state.read().unwrap();
    info!(
        "TRv1 fee market finalize: slot={} base_fee={} gas_used={}",
        bank.slot(),
        fee_state.base_fee_per_cu,
        fee_state.current_gas_used,
    );
    // Future: write fee state to sysvar, emit metrics, etc.
}

/// Calculate what a transaction would pay at the current base fee.
///
/// Convenience method for RPC and wallet queries.
pub fn estimate_transaction_fee(
    bank: &Bank,
    priority_fee_per_cu: u64,
    compute_units: u64,
) -> TransactionFee {
    let base_fee_per_cu = get_current_base_fee(bank);
    calculate_transaction_fee(base_fee_per_cu, priority_fee_per_cu, compute_units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_block_fee_state() {
        let state = BlockFeeState::genesis(5_000);
        assert_eq!(state.base_fee_per_cu, 5_000);
        assert_eq!(state.current_gas_used, 0);
        assert_eq!(state.height, 0);
    }

    #[test]
    fn test_transaction_fee_calculation() {
        let fee = calculate_transaction_fee(5_000, 100, 200_000);
        // base: 5_000 * 200_000 = 1_000_000_000
        // priority: 100 * 200_000 = 20_000_000
        assert_eq!(fee.base_fee, 1_000_000_000);
        assert_eq!(fee.priority_fee, 20_000_000);
        assert_eq!(fee.total_fee, 1_020_000_000);
    }

    #[test]
    fn test_validate_fee_sufficient() {
        let config = FeeMarketConfig::default();
        let result = validate_transaction_fee(
            2_000_000_000, // 2 SOL offered
            100,           // priority fee per CU
            5_000,         // base fee per CU
            200_000,       // 200k CU
            &config,
        );
        assert!(result.is_ok());
        let fee = result.unwrap();
        assert_eq!(fee.total_fee, 1_020_000_000);
    }

    #[test]
    fn test_validate_fee_insufficient() {
        let config = FeeMarketConfig::default();
        let result = validate_transaction_fee(
            100, // way too little
            100,
            5_000,
            200_000,
            &config,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            FeeError::InsufficientFee { .. } => {}
            other => panic!("expected InsufficientFee, got {:?}", other),
        }
    }

    #[test]
    fn test_next_base_fee_increases_on_congestion() {
        let config = FeeMarketConfig::default();
        let mut state = BlockFeeState::genesis(5_000);
        // Simulate above-target usage (target is 24M, max is 48M)
        state.record_gas(36_000_000);
        let parent_state = state;

        let next_fee = calculate_next_base_fee(&config, &parent_state);
        assert!(
            next_fee > 5_000,
            "base fee should increase when above target"
        );
    }

    #[test]
    fn test_next_base_fee_decreases_on_low_usage() {
        let config = FeeMarketConfig::default();
        let mut state = BlockFeeState {
            base_fee_per_cu: 10_000,
            parent_gas_used: 12_000_000, // below target
            current_gas_used: 12_000_000,
            height: 5,
        };

        let next_fee = calculate_next_base_fee(&config, &state);
        assert!(
            next_fee < 10_000,
            "base fee should decrease when below target"
        );
    }
}

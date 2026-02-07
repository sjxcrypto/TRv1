use {borsh::{BorshDeserialize, BorshSerialize}, serde::{Deserialize, Serialize}};

/// Per-block fee state that tracks the dynamic base fee and utilization.
///
/// Each block carries a `BlockFeeState` that records:
/// - The **current base fee** (set when the block was created, based on the parent).
/// - The **parent's gas usage** (used to derive this block's base fee).
/// - A running tally of **current gas used** (updated as transactions are added).
/// - The **block height** for audit / indexing.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct BlockFeeState {
    /// Current base fee per compute unit (lamports).
    /// Set at block creation time from `calculate_next_base_fee`.
    pub base_fee_per_cu: u64,

    /// Compute units consumed by the *parent* block.
    /// This is the value that was used to derive `base_fee_per_cu`.
    pub parent_gas_used: u64,

    /// Running total of compute units consumed in the *current* block so far.
    /// Updated every time a transaction lands.
    pub current_gas_used: u64,

    /// Slot / block height.
    pub height: u64,
}

impl BlockFeeState {
    /// Create the genesis (block-0) fee state with a given initial base fee.
    pub fn genesis(initial_base_fee: u64) -> Self {
        Self {
            base_fee_per_cu: initial_base_fee,
            parent_gas_used: 0,
            current_gas_used: 0,
            height: 0,
        }
    }

    /// Record that `cu` compute units were consumed by a transaction in the
    /// current block.  Returns the new running total.
    #[inline]
    pub fn record_gas(&mut self, cu: u64) -> u64 {
        self.current_gas_used = self.current_gas_used.saturating_add(cu);
        self.current_gas_used
    }

    /// Derive the child block's fee state given the *next* base fee.
    ///
    /// The caller is responsible for computing `next_base_fee` via
    /// [`crate::calculator::calculate_next_base_fee`].
    pub fn next_block(&self, next_base_fee: u64, next_height: u64) -> Self {
        Self {
            base_fee_per_cu: next_base_fee,
            parent_gas_used: self.current_gas_used,
            current_gas_used: 0,
            height: next_height,
        }
    }

    /// Block utilization as a ratio (0.0 – …).
    /// Values above 1.0 should not happen under normal operation but are not
    /// clamped here to aid debugging.
    pub fn utilization(&self, max_block_cu: u64) -> f64 {
        if max_block_cu == 0 {
            return 0.0;
        }
        self.current_gas_used as f64 / max_block_cu as f64
    }
}

/// Breakdown of a single transaction's fee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TransactionFee {
    /// Base fee component: `base_fee_per_cu × compute_units_used`.
    pub base_fee: u64,
    /// Priority fee component: `priority_fee_per_cu × compute_units_used`.
    pub priority_fee: u64,
    /// Total fee (`base_fee + priority_fee`).
    pub total_fee: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_state() {
        let state = BlockFeeState::genesis(5_000);
        assert_eq!(state.base_fee_per_cu, 5_000);
        assert_eq!(state.parent_gas_used, 0);
        assert_eq!(state.current_gas_used, 0);
        assert_eq!(state.height, 0);
    }

    #[test]
    fn test_record_gas() {
        let mut state = BlockFeeState::genesis(5_000);
        assert_eq!(state.record_gas(1_000_000), 1_000_000);
        assert_eq!(state.record_gas(2_000_000), 3_000_000);
        assert_eq!(state.current_gas_used, 3_000_000);
    }

    #[test]
    fn test_record_gas_saturates() {
        let mut state = BlockFeeState::genesis(5_000);
        state.current_gas_used = u64::MAX - 10;
        assert_eq!(state.record_gas(100), u64::MAX);
    }

    #[test]
    fn test_next_block() {
        let mut state = BlockFeeState::genesis(5_000);
        state.record_gas(10_000_000);
        let child = state.next_block(6_000, 1);
        assert_eq!(child.base_fee_per_cu, 6_000);
        assert_eq!(child.parent_gas_used, 10_000_000);
        assert_eq!(child.current_gas_used, 0);
        assert_eq!(child.height, 1);
    }

    #[test]
    fn test_utilization() {
        let mut state = BlockFeeState::genesis(5_000);
        state.record_gas(24_000_000);
        let util = state.utilization(48_000_000);
        assert!((util - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_utilization_zero_max() {
        let state = BlockFeeState::genesis(5_000);
        assert_eq!(state.utilization(0), 0.0);
    }

    #[test]
    fn test_borsh_roundtrip() {
        let state = BlockFeeState {
            base_fee_per_cu: 12345,
            parent_gas_used: 999_999,
            current_gas_used: 500_000,
            height: 42,
        };
        let bytes = borsh::to_vec(&state).unwrap();
        let decoded: BlockFeeState = borsh::from_slice(&bytes).unwrap();
        assert_eq!(state, decoded);
    }
}

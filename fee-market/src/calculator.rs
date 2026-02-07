use crate::{
    config::FeeMarketConfig,
    error::FeeError,
    state::{BlockFeeState, TransactionFee},
};

/// Calculate the next block's base fee using the EIP-1559 algorithm.
///
/// # Formula
///
/// ```text
/// target = max_block_compute_units × target_utilization_pct / 100
///
/// if parent_gas_used == target:
///     next_base_fee = current_base_fee          (no change)
///
/// if parent_gas_used > target:
///     delta = current_base_fee × (parent_gas_used - target) / target / denominator
///     next_base_fee = current_base_fee + max(delta, 1)
///
/// if parent_gas_used < target:
///     delta = current_base_fee × (target - parent_gas_used) / target / denominator
///     next_base_fee = current_base_fee - delta
/// ```
///
/// The result is clamped to `[min_base_fee, max_base_fee]`.
///
/// # Note on `max(delta, 1)` when above target
///
/// Ethereum's EIP-1559 (go-ethereum) enforces a minimum increase of 1 wei
/// when the block is above target.  We replicate this to ensure the base fee
/// always rises under sustained congestion, even when the current base fee is
/// very small.
pub fn calculate_next_base_fee(config: &FeeMarketConfig, state: &BlockFeeState) -> u64 {
    let target = config.target_gas();

    // Edge case: target = 0 means every non-empty block is "above target".
    // We handle this by going straight to max when there is any usage, or
    // returning min_base_fee when parent was empty.
    if target == 0 {
        return if state.parent_gas_used > 0 {
            config.max_base_fee
        } else {
            clamp(state.base_fee_per_cu, config.min_base_fee, config.max_base_fee)
        };
    }

    let current = state.base_fee_per_cu;

    let next = if state.parent_gas_used == target {
        // Exactly at target — no adjustment.
        current
    } else if state.parent_gas_used > target {
        // Above target — increase base fee.
        let excess = state.parent_gas_used.saturating_sub(target);
        // delta = current * excess / target / denominator
        // Use u128 to avoid intermediate overflow.
        let numerator = (current as u128).saturating_mul(excess as u128);
        let denominator = (target as u128).saturating_mul(config.base_fee_change_denominator as u128);
        let delta = if denominator == 0 {
            current // degenerate config; double the fee
        } else {
            let d = numerator / denominator;
            // Ensure at least +1 when above target (matches go-ethereum).
            let d = d.max(1);
            // Clamp to u64.
            d.min(u64::MAX as u128) as u64
        };
        current.saturating_add(delta)
    } else {
        // Below target — decrease base fee.
        let deficit = target.saturating_sub(state.parent_gas_used);
        let numerator = (current as u128).saturating_mul(deficit as u128);
        let denominator = (target as u128).saturating_mul(config.base_fee_change_denominator as u128);
        let delta = if denominator == 0 {
            0
        } else {
            let d = numerator / denominator;
            d.min(u64::MAX as u128) as u64
        };
        current.saturating_sub(delta)
    };

    clamp(next, config.min_base_fee, config.max_base_fee)
}

/// Calculate the fee breakdown for a single transaction.
///
/// Returns a [`TransactionFee`] with the base, priority, and total components.
/// All arithmetic saturates to `u64::MAX`.
pub fn calculate_transaction_fee(
    base_fee_per_cu: u64,
    priority_fee_per_cu: u64,
    compute_units_used: u64,
) -> TransactionFee {
    let base_fee = base_fee_per_cu.saturating_mul(compute_units_used);
    let priority_fee = priority_fee_per_cu.saturating_mul(compute_units_used);
    let total_fee = base_fee.saturating_add(priority_fee);
    TransactionFee {
        base_fee,
        priority_fee,
        total_fee,
    }
}

/// Validate that a transaction can afford the fees for the requested compute
/// units at the current base fee.
///
/// * `offered_lamports` — the maximum the user is willing to pay (balance or
///   declared maxFee).
/// * `priority_fee_per_cu` — the user's chosen priority fee.
/// * `base_fee_per_cu` — current network base fee.
/// * `requested_cu` — compute units the transaction requests.
/// * `config` — fee market config (for min priority fee and block CU cap).
pub fn validate_transaction_fee(
    offered_lamports: u64,
    priority_fee_per_cu: u64,
    base_fee_per_cu: u64,
    requested_cu: u64,
    config: &FeeMarketConfig,
) -> Result<TransactionFee, FeeError> {
    // Check CU limit.
    if requested_cu > config.max_block_compute_units {
        return Err(FeeError::ComputeUnitsExceedMax {
            requested: requested_cu,
            max_block_cu: config.max_block_compute_units,
        });
    }

    // Check minimum priority fee.
    if priority_fee_per_cu < config.min_priority_fee {
        return Err(FeeError::PriorityFeeTooLow {
            offered: priority_fee_per_cu,
            minimum: config.min_priority_fee,
        });
    }

    // Calculate required fee.
    let fee = calculate_transaction_fee(base_fee_per_cu, priority_fee_per_cu, requested_cu);

    // Check affordability.
    if offered_lamports < fee.total_fee {
        return Err(FeeError::InsufficientFee {
            offered: offered_lamports,
            required: fee.total_fee,
            base_fee_per_cu,
            compute_units: requested_cu,
        });
    }

    Ok(fee)
}

/// Validate that a `FeeMarketConfig` is internally consistent.
pub fn validate_config(config: &FeeMarketConfig) -> Result<(), FeeError> {
    if config.min_base_fee > config.max_base_fee {
        return Err(FeeError::InvalidConfig {
            reason: format!(
                "min_base_fee ({}) > max_base_fee ({})",
                config.min_base_fee, config.max_base_fee
            ),
        });
    }
    if config.base_fee_change_denominator == 0 {
        return Err(FeeError::InvalidConfig {
            reason: "base_fee_change_denominator must be > 0".to_string(),
        });
    }
    if config.target_utilization_pct > 100 {
        return Err(FeeError::InvalidConfig {
            reason: format!(
                "target_utilization_pct ({}) must be 0–100",
                config.target_utilization_pct
            ),
        });
    }
    Ok(())
}

#[inline]
fn clamp(value: u64, min: u64, max: u64) -> u64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

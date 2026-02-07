use thiserror::Error;

/// Errors produced by the fee-market subsystem.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum FeeError {
    /// The transaction's `max_fee` (or balance) is insufficient to cover the
    /// required base fee for the requested compute units.
    #[error(
        "Insufficient fee: transaction offers {offered} lamports but requires \
         at least {required} lamports ({base_fee_per_cu} lamports/CU × {compute_units} CU)"
    )]
    InsufficientFee {
        offered: u64,
        required: u64,
        base_fee_per_cu: u64,
        compute_units: u64,
    },

    /// The priority fee supplied is below the configured minimum.
    #[error(
        "Priority fee too low: offered {offered} lamports/CU, minimum is {minimum} lamports/CU"
    )]
    PriorityFeeTooLow { offered: u64, minimum: u64 },

    /// The configuration is invalid (e.g. min > max, denominator = 0).
    #[error("Invalid fee market configuration: {reason}")]
    InvalidConfig { reason: String },

    /// Compute-unit request exceeds the block maximum.
    #[error(
        "Requested compute units ({requested}) exceed block maximum ({max_block_cu})"
    )]
    ComputeUnitsExceedMax {
        requested: u64,
        max_block_cu: u64,
    },

    /// Arithmetic overflow during fee calculation.
    #[error("Fee calculation overflow")]
    Overflow,
}

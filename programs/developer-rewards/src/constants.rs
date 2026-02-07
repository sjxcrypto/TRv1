//! Constants for the TRv1 Developer Rewards program.

/// Minimum compute units a program invocation must consume for the transaction
/// to qualify for developer fee attribution. Prevents trivial/spam programs
/// from siphoning fees.
pub const MIN_COMPUTE_UNITS_THRESHOLD: u64 = 1_000;

/// Number of slots in the cooldown period before a newly registered program
/// becomes eligible for fee revenue. At ~400 ms/slot this is roughly 7 days.
///
/// 7 days × 24 h × 60 min × 60 s / 0.4 s ≈ 1_512_000 slots
pub const COOLDOWN_SLOTS: u64 = 1_512_000;

/// Maximum share of total developer fees any single program may receive in a
/// single epoch, expressed in basis points (10_000 = 100%).
/// 10% cap prevents a single dApp from monopolising the developer pool.
pub const MAX_PROGRAM_FEE_SHARE_BPS: u16 = 1_000; // 10%

/// Total basis points — all revenue splits for a program must sum to this.
pub const TOTAL_BPS: u16 = 10_000;

// ── Fee-split schedule (basis points) ────────────────────────────────────────
// Each constant set represents one phase of the 5-year transition.

/// Launch-phase fee split (years 0–1).
pub mod launch {
    pub const BURN_BPS: u16 = 1_000;      // 10%
    pub const VALIDATOR_BPS: u16 = 0;      //  0%
    pub const TREASURY_BPS: u16 = 4_500;   // 45%
    pub const DEVELOPER_BPS: u16 = 4_500;  // 45%
}

/// Maturity-phase fee split (year 5+).
pub mod maturity {
    pub const BURN_BPS: u16 = 2_500;       // 25%
    pub const VALIDATOR_BPS: u16 = 2_500;   // 25%
    pub const TREASURY_BPS: u16 = 2_500;    // 25%
    pub const DEVELOPER_BPS: u16 = 2_500;   // 25%
}

/// Number of epochs over which the transition from launch → maturity occurs.
/// Assuming ~2-day epochs this covers roughly 5 years (≈ 912 epochs).
pub const TRANSITION_EPOCHS: u64 = 912;

/// Seed prefix for deriving `ProgramRevenueConfig` PDAs.
pub const REVENUE_CONFIG_SEED: &[u8] = b"program_revenue_config";

/// Seed prefix for the developer fee pool account.
pub const FEE_POOL_SEED: &[u8] = b"developer_fee_pool";

/// Seed prefix for the epoch tracker account.
pub const EPOCH_TRACKER_SEED: &[u8] = b"epoch_tracker";

use {borsh::{BorshDeserialize, BorshSerialize}, serde::{Deserialize, Serialize}};

/// Configuration for the EIP-1559-style dynamic fee market.
///
/// This mirrors Ethereum's EIP-1559 mechanism adapted for Solana's compute-unit model:
/// - Instead of gas, we use **compute units (CU)**.
/// - The base fee per CU adjusts each block based on utilization vs. target.
/// - Users set a **priority fee** (tip) on top of the base fee for ordering preference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct FeeMarketConfig {
    /// Minimum base fee per compute unit in lamports (floor).
    /// Prevents the base fee from dropping to zero even under sustained low usage.
    pub min_base_fee: u64,

    /// Maximum base fee per compute unit in lamports (ceiling).
    /// Caps the base fee to prevent runaway costs during extreme congestion.
    pub max_base_fee: u64,

    /// Target block utilization as a percentage (0–100).
    /// The base fee is calibrated so that average blocks use this fraction of capacity.
    /// 50 means the target is half the maximum block compute units (like Ethereum).
    pub target_utilization_pct: u8,

    /// Maximum block capacity in compute units.
    /// This is the hard ceiling for a single block; no more CU can be consumed.
    pub max_block_compute_units: u64,

    /// Denominator for the base fee adjustment fraction.
    /// Each block, the base fee can change by at most `1 / base_fee_change_denominator`.
    /// A value of 8 means ±12.5 % per block, matching Ethereum's EIP-1559.
    pub base_fee_change_denominator: u64,

    /// Minimum priority fee per compute unit that a transaction must include.
    /// Set to 0 to allow free-priority transactions.
    pub min_priority_fee: u64,
}

impl FeeMarketConfig {
    /// Target compute units per block, derived from max capacity and utilization %.
    ///
    /// ```text
    /// target_gas = max_block_compute_units * target_utilization_pct / 100
    /// ```
    #[inline]
    pub fn target_gas(&self) -> u64 {
        self.max_block_compute_units
            .saturating_mul(self.target_utilization_pct as u64)
            / 100
    }
}

impl Default for FeeMarketConfig {
    /// Sensible genesis defaults for TRv1.
    fn default() -> Self {
        Self {
            min_base_fee: 5_000,                // 5 000 lamports  — same order as Solana's 5k/sig
            max_base_fee: 50_000_000,           // 50 M lamports   — hard ceiling
            target_utilization_pct: 50,         // 50 % target
            max_block_compute_units: 48_000_000, // 48 M CU per block
            base_fee_change_denominator: 8,     // ±12.5 % max change per block
            min_priority_fee: 0,                // no forced tip
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = FeeMarketConfig::default();
        assert_eq!(cfg.min_base_fee, 5_000);
        assert_eq!(cfg.max_base_fee, 50_000_000);
        assert_eq!(cfg.target_utilization_pct, 50);
        assert_eq!(cfg.max_block_compute_units, 48_000_000);
        assert_eq!(cfg.base_fee_change_denominator, 8);
        assert_eq!(cfg.min_priority_fee, 0);
    }

    #[test]
    fn test_target_gas() {
        let cfg = FeeMarketConfig::default();
        // 48_000_000 * 50 / 100 = 24_000_000
        assert_eq!(cfg.target_gas(), 24_000_000);
    }

    #[test]
    fn test_target_gas_custom() {
        let cfg = FeeMarketConfig {
            target_utilization_pct: 75,
            max_block_compute_units: 100_000_000,
            ..Default::default()
        };
        // 100_000_000 * 75 / 100 = 75_000_000
        assert_eq!(cfg.target_gas(), 75_000_000);
    }

    #[test]
    fn test_target_gas_zero_pct() {
        let cfg = FeeMarketConfig {
            target_utilization_pct: 0,
            ..Default::default()
        };
        assert_eq!(cfg.target_gas(), 0);
    }

    #[test]
    fn test_target_gas_100_pct() {
        let cfg = FeeMarketConfig {
            target_utilization_pct: 100,
            ..Default::default()
        };
        assert_eq!(cfg.target_gas(), cfg.max_block_compute_units);
    }

    #[test]
    fn test_borsh_roundtrip() {
        let cfg = FeeMarketConfig::default();
        let bytes = borsh::to_vec(&cfg).unwrap();
        let decoded: FeeMarketConfig = borsh::from_slice(&bytes).unwrap();
        assert_eq!(cfg, decoded);
    }

    #[test]
    fn test_serde_roundtrip() {
        let cfg = FeeMarketConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let decoded: FeeMarketConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, decoded);
    }
}

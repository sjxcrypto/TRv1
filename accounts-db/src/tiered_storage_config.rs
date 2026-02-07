//! TRv1 Tiered Storage Configuration
//!
//! This module defines the configuration for TRv1's three-tier account storage
//! architecture, which replaces Solana's memory-mapped Cloudbreak approach.
//!
//! # Architecture
//!
//! Solana's Cloudbreak requires ~512GB RAM because it memory-maps the entire
//! accounts database. TRv1 reduces this to ~32GB by organizing accounts into
//! three tiers based on access recency:
//!
//! - **Hot tier**: LRU cache in RAM (configurable, default 16GB)
//! - **Warm tier**: NVMe SSD storage (~100μs random reads)
//! - **Cold tier**: Archived accounts on HDD/disk (inactive >1 year)
//!
//! # Integration
//!
//! This configuration is consumed by [`AccountCache`](crate::account_cache::AccountCache)
//! and [`StateRentExpiry`](crate::state_rent_expiry) to manage account lifecycle
//! across tiers. Full integration with [`AccountsDb`](crate::accounts_db::AccountsDb)
//! is planned for a subsequent phase.

use std::path::PathBuf;

// ── Size Constants ──────────────────────────────────────────────────────────

/// Default hot cache size: 16 GiB
pub const DEFAULT_HOT_CACHE_SIZE: u64 = 16 * 1024 * 1024 * 1024;

/// Maximum recommended hot cache size: 64 GiB
pub const MAX_HOT_CACHE_SIZE: u64 = 64 * 1024 * 1024 * 1024;

/// Default number of slots of inactivity before an account moves to the warm tier.
/// At ~400ms per slot, 1_000_000 slots ≈ ~4.6 days.
pub const DEFAULT_WARM_THRESHOLD_SLOTS: u64 = 1_000_000;

/// Default number of days of inactivity before an account is archived to cold storage.
pub const DEFAULT_COLD_THRESHOLD_DAYS: u64 = 365;

/// Default warm storage path
pub const DEFAULT_WARM_STORAGE_DIR: &str = "trv1-warm-storage";

/// Default cold storage path
pub const DEFAULT_COLD_STORAGE_DIR: &str = "trv1-cold-storage";

// ── Eviction Policy ─────────────────────────────────────────────────────────

/// Defines the eviction policy used when the hot cache exceeds its size limit.
///
/// The eviction policy determines which accounts are moved from the hot tier
/// (RAM) to the warm tier (NVMe SSD) when the cache is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionPolicy {
    /// Least Recently Used — evicts the account that was accessed least recently.
    /// Best for workloads with temporal locality (most common case).
    LRU,

    /// Least Frequently Used — evicts the account with the fewest total accesses.
    /// Better when some accounts are accessed in bursts but remain important.
    LFU,

    /// Adaptive Replacement Cache — dynamically balances between LRU and LFU
    /// by maintaining ghost lists to track recently evicted entries.
    /// Provides the best overall hit rate but has higher per-operation overhead.
    ARC,
}

impl Default for EvictionPolicy {
    fn default() -> Self {
        EvictionPolicy::LRU
    }
}

impl std::fmt::Display for EvictionPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvictionPolicy::LRU => write!(f, "LRU"),
            EvictionPolicy::LFU => write!(f, "LFU"),
            EvictionPolicy::ARC => write!(f, "ARC"),
        }
    }
}

// ── Tiered Storage Config ───────────────────────────────────────────────────

/// Configuration for TRv1's three-tier account storage system.
///
/// This config controls cache sizing, eviction behavior, storage paths,
/// and the thresholds that govern when accounts migrate between tiers.
///
/// # Example
///
/// ```rust,ignore
/// use solana_accounts_db::tiered_storage_config::TieredStorageConfig;
///
/// let config = TieredStorageConfig {
///     hot_cache_size: 32 * 1024 * 1024 * 1024, // 32 GiB
///     ..TieredStorageConfig::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct TieredStorageConfig {
    /// Maximum RAM cache size in bytes for the hot tier.
    ///
    /// This is the primary knob for controlling memory usage. Larger values
    /// improve cache hit rates but require more RAM.
    ///
    /// Default: 16 GiB (`DEFAULT_HOT_CACHE_SIZE`)
    pub hot_cache_size: u64,

    /// Eviction policy to use when the hot cache is full.
    ///
    /// Default: `EvictionPolicy::LRU`
    pub eviction_policy: EvictionPolicy,

    /// Filesystem path for warm storage (NVMe SSD recommended).
    ///
    /// Accounts evicted from the hot cache are serialized to this path.
    /// For best performance, this should be on a low-latency NVMe drive
    /// capable of ~100μs random reads.
    pub warm_storage_path: PathBuf,

    /// Filesystem path for cold storage (HDD/archive).
    ///
    /// Accounts that have been inactive for `cold_threshold_days` are
    /// archived here. Cold storage can be on slower media since it is
    /// rarely accessed.
    pub cold_storage_path: PathBuf,

    /// Number of slots of inactivity before an account is demoted
    /// from the hot tier to the warm tier.
    ///
    /// At Solana's ~400ms slot time, 1_000_000 slots ≈ 4.6 days.
    ///
    /// Default: `DEFAULT_WARM_THRESHOLD_SLOTS` (1,000,000)
    pub warm_threshold_slots: u64,

    /// Days of inactivity before an account is archived from warm to cold.
    ///
    /// Default: `DEFAULT_COLD_THRESHOLD_DAYS` (365)
    pub cold_threshold_days: u64,

    /// Whether to enable state rent expiry for inactive accounts.
    ///
    /// When enabled, accounts that have been inactive for longer than
    /// `cold_threshold_days` and have insufficient rent balance may be
    /// archived (with revival possible if `allow_revival` is true in
    /// the `StateRentConfig`).
    pub enable_state_rent_expiry: bool,

    /// Maximum number of accounts to evict in a single batch.
    ///
    /// Larger batches amortize I/O overhead but may cause latency spikes.
    /// Default: 4096
    pub eviction_batch_size: usize,

    /// Target utilization ratio for the hot cache (0.0 - 1.0).
    ///
    /// When the cache exceeds `hot_cache_size * target_utilization`,
    /// background eviction begins. This provides headroom to avoid
    /// synchronous eviction on the critical path.
    ///
    /// Default: 0.90 (eviction starts at 90% full)
    pub target_utilization: f64,
}

impl Default for TieredStorageConfig {
    fn default() -> Self {
        Self {
            hot_cache_size: DEFAULT_HOT_CACHE_SIZE,
            eviction_policy: EvictionPolicy::default(),
            warm_storage_path: PathBuf::from(DEFAULT_WARM_STORAGE_DIR),
            cold_storage_path: PathBuf::from(DEFAULT_COLD_STORAGE_DIR),
            warm_threshold_slots: DEFAULT_WARM_THRESHOLD_SLOTS,
            cold_threshold_days: DEFAULT_COLD_THRESHOLD_DAYS,
            enable_state_rent_expiry: false,
            eviction_batch_size: 4096,
            target_utilization: 0.90,
        }
    }
}

impl TieredStorageConfig {
    /// Create a config suitable for testing with smaller sizes.
    pub fn for_testing() -> Self {
        Self {
            hot_cache_size: 64 * 1024 * 1024, // 64 MiB
            warm_storage_path: PathBuf::from("/tmp/trv1-test-warm"),
            cold_storage_path: PathBuf::from("/tmp/trv1-test-cold"),
            warm_threshold_slots: 100,
            cold_threshold_days: 1,
            enable_state_rent_expiry: true,
            eviction_batch_size: 64,
            target_utilization: 0.80,
            ..Default::default()
        }
    }

    /// Create a config optimized for validators with 32GB RAM.
    pub fn for_32gb_validator() -> Self {
        Self {
            hot_cache_size: 24 * 1024 * 1024 * 1024, // 24 GiB — leaves 8 GiB for OS + runtime
            eviction_policy: EvictionPolicy::LRU,
            ..Default::default()
        }
    }

    /// Create a config optimized for high-performance validators with 64GB+ RAM.
    pub fn for_64gb_validator() -> Self {
        Self {
            hot_cache_size: 48 * 1024 * 1024 * 1024, // 48 GiB
            eviction_policy: EvictionPolicy::ARC,
            eviction_batch_size: 8192,
            ..Default::default()
        }
    }

    /// Validate the configuration, returning an error message if invalid.
    pub fn validate(&self) -> Result<(), String> {
        if self.hot_cache_size == 0 {
            return Err("hot_cache_size must be > 0".to_string());
        }
        if self.hot_cache_size > MAX_HOT_CACHE_SIZE {
            return Err(format!(
                "hot_cache_size {} exceeds maximum {} (64 GiB)",
                self.hot_cache_size, MAX_HOT_CACHE_SIZE
            ));
        }
        if self.target_utilization <= 0.0 || self.target_utilization > 1.0 {
            return Err(format!(
                "target_utilization must be in (0.0, 1.0], got {}",
                self.target_utilization
            ));
        }
        if self.eviction_batch_size == 0 {
            return Err("eviction_batch_size must be > 0".to_string());
        }
        if self.cold_threshold_days == 0 {
            return Err("cold_threshold_days must be > 0".to_string());
        }
        Ok(())
    }

    /// Returns the eviction watermark in bytes.
    ///
    /// Background eviction begins when total cache size exceeds this value.
    pub fn eviction_watermark(&self) -> u64 {
        (self.hot_cache_size as f64 * self.target_utilization) as u64
    }

    /// Estimated maximum number of accounts that can fit in the hot cache,
    /// assuming a given average account size in bytes.
    pub fn estimated_hot_capacity(&self, avg_account_size: u64) -> u64 {
        if avg_account_size == 0 {
            return 0;
        }
        self.hot_cache_size / avg_account_size
    }
}

// ── Tier Statistics ─────────────────────────────────────────────────────────

/// Runtime statistics for the tiered storage system.
///
/// These statistics are updated atomically and can be queried to monitor
/// cache performance, tier distribution, and read latencies.
#[derive(Debug, Clone, Default)]
pub struct TierStats {
    /// Number of accounts currently in the hot tier (RAM)
    pub hot_accounts: u64,
    /// Total size of accounts in the hot tier, in bytes
    pub hot_size_bytes: u64,

    /// Number of accounts currently in the warm tier (NVMe SSD)
    pub warm_accounts: u64,
    /// Total size of accounts in the warm tier, in bytes
    pub warm_size_bytes: u64,

    /// Number of accounts currently in the cold tier (archive)
    pub cold_accounts: u64,
    /// Total size of accounts in the cold tier, in bytes
    pub cold_size_bytes: u64,

    /// Cache hit rate (0.0 - 1.0) for the hot tier
    pub cache_hit_rate: f64,
    /// Cache miss rate (0.0 - 1.0) for the hot tier
    pub cache_miss_rate: f64,

    /// Average read latency in microseconds across all tiers
    pub avg_read_latency_us: f64,

    /// Total number of cache hits since startup
    pub total_hits: u64,
    /// Total number of cache misses since startup
    pub total_misses: u64,

    /// Number of accounts promoted from warm to hot
    pub warm_to_hot_promotions: u64,
    /// Number of accounts demoted from hot to warm
    pub hot_to_warm_demotions: u64,
    /// Number of accounts archived from warm to cold
    pub warm_to_cold_archives: u64,
    /// Number of accounts revived from cold storage
    pub cold_revivals: u64,
}

impl TierStats {
    /// Return the total number of accounts across all tiers.
    pub fn total_accounts(&self) -> u64 {
        self.hot_accounts
            .saturating_add(self.warm_accounts)
            .saturating_add(self.cold_accounts)
    }

    /// Return the total size in bytes across all tiers.
    pub fn total_size_bytes(&self) -> u64 {
        self.hot_size_bytes
            .saturating_add(self.warm_size_bytes)
            .saturating_add(self.cold_size_bytes)
    }

    /// Recalculate derived rates from running totals.
    pub fn recalculate_rates(&mut self) {
        let total = self.total_hits.saturating_add(self.total_misses);
        if total > 0 {
            self.cache_hit_rate = self.total_hits as f64 / total as f64;
            self.cache_miss_rate = self.total_misses as f64 / total as f64;
        }
    }

    /// Format a human-readable summary of tier distribution.
    pub fn summary(&self) -> String {
        format!(
            "Hot: {} accts ({} bytes) | Warm: {} accts ({} bytes) | Cold: {} accts ({} bytes) | Hit rate: {:.2}%",
            self.hot_accounts, self.hot_size_bytes,
            self.warm_accounts, self.warm_size_bytes,
            self.cold_accounts, self.cold_size_bytes,
            self.cache_hit_rate * 100.0,
        )
    }
}

/// Identifies which tier an account currently resides in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountTier {
    /// Account is in the hot cache (RAM)
    Hot,
    /// Account is on warm storage (NVMe SSD)
    Warm,
    /// Account is archived to cold storage (HDD)
    Cold,
}

impl std::fmt::Display for AccountTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountTier::Hot => write!(f, "Hot (RAM)"),
            AccountTier::Warm => write!(f, "Warm (SSD)"),
            AccountTier::Cold => write!(f, "Cold (Archive)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TieredStorageConfig::default();
        assert_eq!(config.hot_cache_size, DEFAULT_HOT_CACHE_SIZE);
        assert_eq!(config.eviction_policy, EvictionPolicy::LRU);
        assert_eq!(config.warm_threshold_slots, DEFAULT_WARM_THRESHOLD_SLOTS);
        assert_eq!(config.cold_threshold_days, DEFAULT_COLD_THRESHOLD_DAYS);
        assert!(!config.enable_state_rent_expiry);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_testing_config() {
        let config = TieredStorageConfig::for_testing();
        assert_eq!(config.hot_cache_size, 64 * 1024 * 1024);
        assert!(config.enable_state_rent_expiry);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validation_rejects_zero_cache() {
        let mut config = TieredStorageConfig::default();
        config.hot_cache_size = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_rejects_excessive_cache() {
        let mut config = TieredStorageConfig::default();
        config.hot_cache_size = MAX_HOT_CACHE_SIZE + 1;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_rejects_bad_utilization() {
        let mut config = TieredStorageConfig::default();
        config.target_utilization = 0.0;
        assert!(config.validate().is_err());

        config.target_utilization = 1.5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_eviction_watermark() {
        let config = TieredStorageConfig {
            hot_cache_size: 1000,
            target_utilization: 0.80,
            ..Default::default()
        };
        assert_eq!(config.eviction_watermark(), 800);
    }

    #[test]
    fn test_estimated_capacity() {
        let config = TieredStorageConfig {
            hot_cache_size: 1_000_000,
            ..Default::default()
        };
        assert_eq!(config.estimated_hot_capacity(1000), 1000);
        assert_eq!(config.estimated_hot_capacity(0), 0);
    }

    #[test]
    fn test_tier_stats_totals() {
        let stats = TierStats {
            hot_accounts: 100,
            hot_size_bytes: 1000,
            warm_accounts: 200,
            warm_size_bytes: 2000,
            cold_accounts: 300,
            cold_size_bytes: 3000,
            ..Default::default()
        };
        assert_eq!(stats.total_accounts(), 600);
        assert_eq!(stats.total_size_bytes(), 6000);
    }

    #[test]
    fn test_tier_stats_rates() {
        let mut stats = TierStats {
            total_hits: 80,
            total_misses: 20,
            ..Default::default()
        };
        stats.recalculate_rates();
        assert!((stats.cache_hit_rate - 0.80).abs() < f64::EPSILON);
        assert!((stats.cache_miss_rate - 0.20).abs() < f64::EPSILON);
    }

    #[test]
    fn test_eviction_policy_display() {
        assert_eq!(format!("{}", EvictionPolicy::LRU), "LRU");
        assert_eq!(format!("{}", EvictionPolicy::LFU), "LFU");
        assert_eq!(format!("{}", EvictionPolicy::ARC), "ARC");
    }

    #[test]
    fn test_account_tier_display() {
        assert_eq!(format!("{}", AccountTier::Hot), "Hot (RAM)");
        assert_eq!(format!("{}", AccountTier::Warm), "Warm (SSD)");
        assert_eq!(format!("{}", AccountTier::Cold), "Cold (Archive)");
    }

    #[test]
    fn test_preset_configs_are_valid() {
        assert!(TieredStorageConfig::for_32gb_validator().validate().is_ok());
        assert!(TieredStorageConfig::for_64gb_validator().validate().is_ok());
        assert!(TieredStorageConfig::for_testing().validate().is_ok());
    }
}

//! # TRv1 Monitoring
//!
//! Performance monitoring and metrics collection for the TRv1 blockchain.
//!
//! Provides three metric types matching Prometheus conventions:
//! - **Counter**: monotonically increasing value (e.g., blocks produced)
//! - **Gauge**: value that can go up or down (e.g., current base fee)
//! - **Histogram**: distribution of observations (e.g., finality times)
//!
//! ## Usage
//!
//! ```rust
//! use trv1_monitoring::{TRv1Metrics, MetricsSnapshot};
//!
//! let metrics = TRv1Metrics::new();
//!
//! // Record consensus events
//! metrics.blocks_produced.inc();
//! metrics.consensus_rounds.observe(2.0);
//! metrics.finality_time_ms.observe(1200.0);
//!
//! // Record fee events
//! metrics.current_base_fee.set(5_000);
//! metrics.total_fees_burned.add(100_000);
//!
//! // Export as Prometheus text format
//! let snapshot = metrics.snapshot();
//! let prom_text = trv1_monitoring::prometheus::encode(&snapshot);
//! ```

pub mod prometheus;

use parking_lot::Mutex;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Metric primitives
// ---------------------------------------------------------------------------

/// A monotonically increasing counter.
pub struct Counter {
    value: AtomicU64,
    #[allow(dead_code)]
    name: &'static str,
    #[allow(dead_code)]
    help: &'static str,
}

impl Counter {
    pub const fn new(name: &'static str, help: &'static str) -> Self {
        Self {
            value: AtomicU64::new(0),
            name,
            help,
        }
    }

    /// Increment the counter by 1.
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Add a value to the counter.
    pub fn add(&self, v: u64) {
        self.value.fetch_add(v, Ordering::Relaxed);
    }

    /// Get the current counter value.
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Reset the counter to zero.
    pub fn reset(&self) {
        self.value.store(0, Ordering::Relaxed);
    }
}

/// A gauge that can go up or down.
pub struct Gauge {
    value: AtomicI64,
    #[allow(dead_code)]
    name: &'static str,
    #[allow(dead_code)]
    help: &'static str,
}

impl Gauge {
    pub const fn new(name: &'static str, help: &'static str) -> Self {
        Self {
            value: AtomicI64::new(0),
            name,
            help,
        }
    }

    /// Set the gauge to an absolute value.
    pub fn set(&self, v: i64) {
        self.value.store(v, Ordering::Relaxed);
    }

    /// Increment the gauge by 1.
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement the gauge by 1.
    pub fn dec(&self) {
        self.value.fetch_sub(1, Ordering::Relaxed);
    }

    /// Add a value to the gauge.
    pub fn add(&self, v: i64) {
        self.value.fetch_add(v, Ordering::Relaxed);
    }

    /// Get the current gauge value.
    pub fn get(&self) -> i64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// A histogram that collects observations into configurable buckets.
pub struct Histogram {
    buckets: Vec<f64>,
    counts: Vec<AtomicU64>,
    sum: Mutex<f64>,
    count: AtomicU64,
    #[allow(dead_code)]
    name: &'static str,
    #[allow(dead_code)]
    help: &'static str,
}

impl Histogram {
    /// Create a histogram with the given bucket upper bounds.
    pub fn new(name: &'static str, help: &'static str, buckets: Vec<f64>) -> Self {
        let counts = buckets.iter().map(|_| AtomicU64::new(0)).collect();
        Self {
            buckets,
            counts,
            sum: Mutex::new(0.0),
            count: AtomicU64::new(0),
            name,
            help,
        }
    }

    /// Observe a value, adding it to the appropriate bucket(s).
    pub fn observe(&self, v: f64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        {
            let mut sum = self.sum.lock();
            *sum += v;
        }
        for (i, bound) in self.buckets.iter().enumerate() {
            if v <= *bound {
                self.counts[i].fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Get the total number of observations.
    pub fn get_count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Get the sum of all observations.
    pub fn get_sum(&self) -> f64 {
        *self.sum.lock()
    }

    /// Get bucket counts.
    pub fn get_buckets(&self) -> Vec<(f64, u64)> {
        self.buckets
            .iter()
            .zip(self.counts.iter())
            .map(|(bound, count)| (*bound, count.load(Ordering::Relaxed)))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Default histogram buckets
// ---------------------------------------------------------------------------

/// Default buckets for timing histograms (milliseconds).
pub fn default_time_buckets() -> Vec<f64> {
    vec![
        10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0,
    ]
}

/// Default buckets for round-count histograms.
pub fn default_round_buckets() -> Vec<f64> {
    vec![1.0, 2.0, 3.0, 4.0, 5.0, 10.0]
}

// ---------------------------------------------------------------------------
// TRv1 Metrics
// ---------------------------------------------------------------------------

/// Complete metrics collection for a TRv1 validator node.
pub struct TRv1Metrics {
    // -- Consensus --
    pub blocks_produced: Counter,
    pub consensus_rounds: Histogram,
    pub finality_time_ms: Histogram,
    pub missed_proposals: Counter,

    // -- Fee Market --
    pub current_base_fee: Gauge,
    pub block_utilization: Gauge,
    pub total_fees_burned: Counter,
    pub total_fees_treasury: Counter,
    pub total_fees_dev: Counter,
    pub total_fees_validator: Counter,

    // -- Storage --
    pub hot_cache_size: Gauge,
    pub warm_storage_size: Gauge,
    pub cold_storage_size: Gauge,
    pub cache_hit_rate: Gauge,
    pub cache_evictions: Counter,

    // -- Staking --
    pub total_staked: Gauge,
    pub staking_participation_rate: Gauge,
    pub active_validators: Gauge,
    pub standby_validators: Gauge,
    pub jailed_validators: Gauge,

    // -- Passive Staking --
    pub passive_stake_total: Gauge,
    pub passive_stake_tier_0: Gauge,
    pub passive_stake_tier_1: Gauge,
    pub passive_stake_tier_2: Gauge,
    pub passive_stake_tier_3: Gauge,
    pub passive_stake_tier_4: Gauge,
    pub passive_stake_tier_5: Gauge,
}

impl TRv1Metrics {
    /// Create a new metrics instance with all counters at zero.
    pub fn new() -> Self {
        Self {
            // Consensus
            blocks_produced: Counter::new(
                "trv1_blocks_produced_total",
                "Total number of blocks produced by this validator",
            ),
            consensus_rounds: Histogram::new(
                "trv1_consensus_rounds",
                "Number of consensus rounds needed to finalize a block",
                default_round_buckets(),
            ),
            finality_time_ms: Histogram::new(
                "trv1_finality_time_ms",
                "Time from proposal to commit in milliseconds",
                default_time_buckets(),
            ),
            missed_proposals: Counter::new(
                "trv1_missed_proposals_total",
                "Total number of missed block proposals",
            ),

            // Fee Market
            current_base_fee: Gauge::new(
                "trv1_current_base_fee",
                "Current base fee per compute unit in lamports",
            ),
            block_utilization: Gauge::new(
                "trv1_block_utilization_bps",
                "Current block utilization in basis points (10000 = 100%)",
            ),
            total_fees_burned: Counter::new(
                "trv1_fees_burned_total",
                "Total fees burned (lamports)",
            ),
            total_fees_treasury: Counter::new(
                "trv1_fees_treasury_total",
                "Total fees sent to treasury (lamports)",
            ),
            total_fees_dev: Counter::new(
                "trv1_fees_dev_total",
                "Total fees sent to developer fund (lamports)",
            ),
            total_fees_validator: Counter::new(
                "trv1_fees_validator_total",
                "Total fees distributed to validators (lamports)",
            ),

            // Storage
            hot_cache_size: Gauge::new(
                "trv1_hot_cache_size_bytes",
                "Size of the hot (in-memory) account cache in bytes",
            ),
            warm_storage_size: Gauge::new(
                "trv1_warm_storage_size_bytes",
                "Size of warm (SSD) storage in bytes",
            ),
            cold_storage_size: Gauge::new(
                "trv1_cold_storage_size_bytes",
                "Size of cold (archival) storage in bytes",
            ),
            cache_hit_rate: Gauge::new(
                "trv1_cache_hit_rate_bps",
                "Account cache hit rate in basis points (10000 = 100%)",
            ),
            cache_evictions: Counter::new(
                "trv1_cache_evictions_total",
                "Total number of cache evictions",
            ),

            // Staking
            total_staked: Gauge::new(
                "trv1_total_staked_lamports",
                "Total lamports staked across all validators",
            ),
            staking_participation_rate: Gauge::new(
                "trv1_staking_participation_rate_bps",
                "Staking participation rate in basis points",
            ),
            active_validators: Gauge::new(
                "trv1_active_validators",
                "Number of active validators in the current set",
            ),
            standby_validators: Gauge::new(
                "trv1_standby_validators",
                "Number of standby validators",
            ),
            jailed_validators: Gauge::new(
                "trv1_jailed_validators",
                "Number of jailed validators",
            ),

            // Passive Staking
            passive_stake_total: Gauge::new(
                "trv1_passive_stake_total_lamports",
                "Total lamports in passive staking",
            ),
            passive_stake_tier_0: Gauge::new(
                "trv1_passive_stake_tier_no_lock_lamports",
                "Passive stake in no-lock tier (lamports)",
            ),
            passive_stake_tier_1: Gauge::new(
                "trv1_passive_stake_tier_30d_lamports",
                "Passive stake in 30-day tier (lamports)",
            ),
            passive_stake_tier_2: Gauge::new(
                "trv1_passive_stake_tier_90d_lamports",
                "Passive stake in 90-day tier (lamports)",
            ),
            passive_stake_tier_3: Gauge::new(
                "trv1_passive_stake_tier_180d_lamports",
                "Passive stake in 180-day tier (lamports)",
            ),
            passive_stake_tier_4: Gauge::new(
                "trv1_passive_stake_tier_360d_lamports",
                "Passive stake in 360-day tier (lamports)",
            ),
            passive_stake_tier_5: Gauge::new(
                "trv1_passive_stake_tier_permanent_lamports",
                "Passive stake in permanent tier (lamports)",
            ),
        }
    }

    /// Get a reference to a passive stake tier gauge by index.
    pub fn passive_stake_tier(&self, index: usize) -> Option<&Gauge> {
        match index {
            0 => Some(&self.passive_stake_tier_0),
            1 => Some(&self.passive_stake_tier_1),
            2 => Some(&self.passive_stake_tier_2),
            3 => Some(&self.passive_stake_tier_3),
            4 => Some(&self.passive_stake_tier_4),
            5 => Some(&self.passive_stake_tier_5),
            _ => None,
        }
    }

    /// Take a full snapshot of all metrics for export.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            // Consensus
            blocks_produced: self.blocks_produced.get(),
            consensus_rounds_count: self.consensus_rounds.get_count(),
            consensus_rounds_sum: self.consensus_rounds.get_sum(),
            consensus_rounds_buckets: self.consensus_rounds.get_buckets(),
            finality_time_count: self.finality_time_ms.get_count(),
            finality_time_sum: self.finality_time_ms.get_sum(),
            finality_time_buckets: self.finality_time_ms.get_buckets(),
            missed_proposals: self.missed_proposals.get(),

            // Fee Market
            current_base_fee: self.current_base_fee.get(),
            block_utilization: self.block_utilization.get(),
            total_fees_burned: self.total_fees_burned.get(),
            total_fees_treasury: self.total_fees_treasury.get(),
            total_fees_dev: self.total_fees_dev.get(),
            total_fees_validator: self.total_fees_validator.get(),

            // Storage
            hot_cache_size: self.hot_cache_size.get(),
            warm_storage_size: self.warm_storage_size.get(),
            cold_storage_size: self.cold_storage_size.get(),
            cache_hit_rate: self.cache_hit_rate.get(),
            cache_evictions: self.cache_evictions.get(),

            // Staking
            total_staked: self.total_staked.get(),
            staking_participation_rate: self.staking_participation_rate.get(),
            active_validators: self.active_validators.get(),
            standby_validators: self.standby_validators.get(),
            jailed_validators: self.jailed_validators.get(),

            // Passive Staking
            passive_stake_total: self.passive_stake_total.get(),
            passive_stake_by_tier: [
                self.passive_stake_tier_0.get(),
                self.passive_stake_tier_1.get(),
                self.passive_stake_tier_2.get(),
                self.passive_stake_tier_3.get(),
                self.passive_stake_tier_4.get(),
                self.passive_stake_tier_5.get(),
            ],
        }
    }
}

impl Default for TRv1Metrics {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Snapshot (point-in-time export)
// ---------------------------------------------------------------------------

/// A serialisable point-in-time snapshot of all TRv1 metrics.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    // Consensus
    pub blocks_produced: u64,
    pub consensus_rounds_count: u64,
    pub consensus_rounds_sum: f64,
    pub consensus_rounds_buckets: Vec<(f64, u64)>,
    pub finality_time_count: u64,
    pub finality_time_sum: f64,
    pub finality_time_buckets: Vec<(f64, u64)>,
    pub missed_proposals: u64,

    // Fee Market
    pub current_base_fee: i64,
    pub block_utilization: i64,
    pub total_fees_burned: u64,
    pub total_fees_treasury: u64,
    pub total_fees_dev: u64,
    pub total_fees_validator: u64,

    // Storage
    pub hot_cache_size: i64,
    pub warm_storage_size: i64,
    pub cold_storage_size: i64,
    pub cache_hit_rate: i64,
    pub cache_evictions: u64,

    // Staking
    pub total_staked: i64,
    pub staking_participation_rate: i64,
    pub active_validators: i64,
    pub standby_validators: i64,
    pub jailed_validators: i64,

    // Passive Staking
    pub passive_stake_total: i64,
    pub passive_stake_by_tier: [i64; 6],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        let c = Counter::new("test", "test counter");
        assert_eq!(c.get(), 0);
        c.inc();
        assert_eq!(c.get(), 1);
        c.add(5);
        assert_eq!(c.get(), 6);
    }

    #[test]
    fn test_gauge() {
        let g = Gauge::new("test", "test gauge");
        assert_eq!(g.get(), 0);
        g.set(42);
        assert_eq!(g.get(), 42);
        g.inc();
        assert_eq!(g.get(), 43);
        g.dec();
        assert_eq!(g.get(), 42);
        g.add(-10);
        assert_eq!(g.get(), 32);
    }

    #[test]
    fn test_histogram() {
        let h = Histogram::new("test", "test histogram", vec![10.0, 50.0, 100.0]);
        h.observe(5.0);
        h.observe(25.0);
        h.observe(75.0);
        h.observe(150.0);

        assert_eq!(h.get_count(), 4);
        assert!((h.get_sum() - 255.0).abs() < 1e-6);

        let buckets = h.get_buckets();
        assert_eq!(buckets[0], (10.0, 1));  // 5.0 ≤ 10
        assert_eq!(buckets[1], (50.0, 2));  // 5.0, 25.0 ≤ 50
        assert_eq!(buckets[2], (100.0, 3)); // 5.0, 25.0, 75.0 ≤ 100
    }

    #[test]
    fn test_metrics_snapshot() {
        let m = TRv1Metrics::new();
        m.blocks_produced.inc();
        m.blocks_produced.inc();
        m.current_base_fee.set(5_000);
        m.active_validators.set(100);
        m.passive_stake_tier_0.set(1_000_000);

        let snap = m.snapshot();
        assert_eq!(snap.blocks_produced, 2);
        assert_eq!(snap.current_base_fee, 5_000);
        assert_eq!(snap.active_validators, 100);
        assert_eq!(snap.passive_stake_by_tier[0], 1_000_000);
    }

    #[test]
    fn test_passive_stake_tier_accessor() {
        let m = TRv1Metrics::new();
        for i in 0..6 {
            assert!(m.passive_stake_tier(i).is_some());
        }
        assert!(m.passive_stake_tier(6).is_none());
    }
}

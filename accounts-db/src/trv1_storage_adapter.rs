//! TRv1 Storage Adapter — Tiered caching layer for AccountsDb
//!
//! This module wraps AccountsDb with TRv1's three-tier storage system:
//! - **Hot cache** (RAM): LRU cache for frequently-accessed accounts
//! - **Warm storage** (NVMe SSD): Recently evicted accounts
//! - **Cold storage** (Archive): Long-inactive accounts
//!
//! The adapter intercepts reads and writes, checking the hot cache first
//! and populating it on misses. A background maintenance service handles
//! eviction and tier migration.
//!
//! # Feature Gate
//!
//! All functionality is gated behind the `trv1-tiered-storage` feature flag.

use {
    crate::{
        account_cache::AccountCache,
        state_rent_expiry::{
            archive_account, check_rent_expiry, ArchiveIndex, StateRentConfig,
        },
        tiered_storage_config::{TierStats, TieredStorageConfig},
    },
    log::*,
    solana_account::AccountSharedData,
    solana_pubkey::Pubkey,
    std::sync::{
        atomic::{AtomicU64, Ordering},
        RwLock,
    },
};

// ── Atomic Statistics ───────────────────────────────────────────────────────

/// Thread-safe counters for tiered storage operations.
///
/// These are updated atomically and periodically snapshotted into a `TierStats`
/// for reporting.
pub struct AtomicTierStats {
    pub hot_hits: AtomicU64,
    pub hot_misses: AtomicU64,
    pub hot_inserts: AtomicU64,
    pub hot_evictions: AtomicU64,
    pub warm_to_hot_promotions: AtomicU64,
    pub cold_archives: AtomicU64,
    pub cold_revivals: AtomicU64,
    pub maintenance_ticks: AtomicU64,
}

impl Default for AtomicTierStats {
    fn default() -> Self {
        Self {
            hot_hits: AtomicU64::new(0),
            hot_misses: AtomicU64::new(0),
            hot_inserts: AtomicU64::new(0),
            hot_evictions: AtomicU64::new(0),
            warm_to_hot_promotions: AtomicU64::new(0),
            cold_archives: AtomicU64::new(0),
            cold_revivals: AtomicU64::new(0),
            maintenance_ticks: AtomicU64::new(0),
        }
    }
}

impl AtomicTierStats {
    /// Snapshot the atomic counters into a `TierStats` struct.
    pub fn snapshot(&self) -> TierStats {
        let total_hits = self.hot_hits.load(Ordering::Relaxed);
        let total_misses = self.hot_misses.load(Ordering::Relaxed);
        let total = total_hits.saturating_add(total_misses);
        let (hit_rate, miss_rate) = if total > 0 {
            (
                total_hits as f64 / total as f64,
                total_misses as f64 / total as f64,
            )
        } else {
            (0.0, 0.0)
        };

        TierStats {
            total_hits,
            total_misses,
            cache_hit_rate: hit_rate,
            cache_miss_rate: miss_rate,
            hot_to_warm_demotions: self.hot_evictions.load(Ordering::Relaxed),
            warm_to_hot_promotions: self.warm_to_hot_promotions.load(Ordering::Relaxed),
            warm_to_cold_archives: self.cold_archives.load(Ordering::Relaxed),
            cold_revivals: self.cold_revivals.load(Ordering::Relaxed),
            ..TierStats::default()
        }
    }
}

// ── TRv1 Storage Adapter ────────────────────────────────────────────────────

/// The main adapter that layers TRv1 tiered storage on top of AccountsDb.
///
/// This struct owns the hot cache, configuration, and statistics. It does NOT
/// own the AccountsDb — that remains the caller's responsibility. Instead,
/// the adapter provides methods that the caller invokes around AccountsDb
/// operations.
///
/// # Usage Pattern
///
/// ```rust,ignore
/// // On load:
/// if let Some(account) = adapter.hot_cache_get(&pubkey) {
///     return Some((account, slot));
/// }
/// // ... normal AccountsDb load ...
/// adapter.hot_cache_insert(&pubkey, &account);
///
/// // On store:
/// adapter.hot_cache_insert(&pubkey, &account);
/// ```
pub struct TRv1StorageAdapter {
    /// Our custom LRU hot cache, protected by a read-write lock.
    /// Reads (get) take a write lock because LRU reordering is a mutation.
    hot_cache: RwLock<AccountCache>,

    /// Tiered storage configuration.
    config: TieredStorageConfig,

    /// Rent expiry configuration.
    rent_config: StateRentConfig,

    /// Index of archived (cold storage) accounts.
    archive_index: RwLock<ArchiveIndex>,

    /// Atomic statistics counters.
    stats: AtomicTierStats,
}

impl TRv1StorageAdapter {
    /// Create a new storage adapter with the given configuration.
    pub fn new(config: TieredStorageConfig, rent_config: StateRentConfig) -> Self {
        info!(
            "TRv1 Storage Adapter initialized: hot_cache={}GB, eviction_policy={}, rent_expiry={}",
            config.hot_cache_size / (1024 * 1024 * 1024),
            config.eviction_policy,
            config.enable_state_rent_expiry,
        );

        let cache = AccountCache::new(config.clone());

        Self {
            hot_cache: RwLock::new(cache),
            config,
            rent_config,
            archive_index: RwLock::new(ArchiveIndex::new()),
            stats: AtomicTierStats::default(),
        }
    }

    /// Try to load an account from the hot cache.
    ///
    /// Returns `Some(account)` on a cache hit, `None` on a miss.
    /// On hit, the account is promoted to the head of the LRU list.
    pub fn hot_cache_get(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        // LRU get requires mutation (reordering), so we need a write lock
        let mut cache = self.hot_cache.write().unwrap();
        if let Some(account) = cache.get(pubkey) {
            self.stats.hot_hits.fetch_add(1, Ordering::Relaxed);
            Some(account.clone())
        } else {
            self.stats.hot_misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    /// Insert an account into the hot cache.
    ///
    /// If the account already exists, its data is updated and it is promoted
    /// to the most-recently-used position. If it's new, it is added at the head.
    ///
    /// This does NOT trigger eviction. Call `maybe_evict()` separately
    /// (typically from the maintenance thread).
    pub fn hot_cache_insert(&self, pubkey: &Pubkey, account: &AccountSharedData) {
        let mut cache = self.hot_cache.write().unwrap();
        cache.insert(*pubkey, account.clone());
        self.stats.hot_inserts.fetch_add(1, Ordering::Relaxed);
    }

    /// Remove an account from the hot cache.
    ///
    /// Returns the account data if it was present.
    pub fn hot_cache_remove(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        let mut cache = self.hot_cache.write().unwrap();
        cache.remove(pubkey)
    }

    /// Check if the hot cache needs eviction and perform it if so.
    ///
    /// Returns the number of accounts evicted. Evicted accounts should be
    /// persisted to warm storage by the caller (or maintenance service).
    pub fn maybe_evict(&self) -> Vec<(Pubkey, AccountSharedData)> {
        let mut cache = self.hot_cache.write().unwrap();
        if cache.needs_eviction() {
            let evicted = cache.evict_to_warm();
            let count = evicted.len() as u64;
            self.stats.hot_evictions.fetch_add(count, Ordering::Relaxed);
            evicted
        } else {
            Vec::new()
        }
    }

    /// Perform a maintenance tick: evict cold accounts, check rent expiry.
    ///
    /// This is called periodically by the `MaintenanceService`.
    ///
    /// # Arguments
    ///
    /// * `current_slot` - The current slot number
    /// * `current_epoch` - The current epoch number
    pub fn maintenance_tick(&self, current_slot: u64, current_epoch: u64) {
        self.stats.maintenance_ticks.fetch_add(1, Ordering::Relaxed);

        // Step 1: Evict from hot cache if over watermark
        let evicted = self.maybe_evict();
        if !evicted.is_empty() {
            debug!(
                "TRv1 maintenance: evicted {} accounts from hot cache",
                evicted.len()
            );
            // In a full implementation, evicted accounts would be written to
            // warm storage (NVMe SSD) here. For now we just drop them — they
            // can be re-read from the underlying AccountsDb storage.
        }

        // Step 2: Check rent expiry if enabled
        if self.config.enable_state_rent_expiry {
            self.check_and_archive_expired_accounts(current_slot, current_epoch);
        }

        // Step 3: Report statistics
        let stats = self.stats();
        if stats.total_hits.saturating_add(stats.total_misses) > 0 {
            debug!(
                "TRv1 stats: hit_rate={:.2}%, hot_accounts={}, evictions={}, archives={}",
                stats.cache_hit_rate * 100.0,
                stats.hot_accounts,
                stats.hot_to_warm_demotions,
                stats.warm_to_cold_archives,
            );
        }
    }

    /// Check for accounts that have expired their rent and archive them.
    fn check_and_archive_expired_accounts(&self, current_slot: u64, current_epoch: u64) {
        // In the current implementation, we don't have direct access to
        // last_active_slot from here. The maintenance service will call
        // this with accounts it knows about. For now, this is a no-op
        // placeholder that will be filled in when integrated with the
        // AccountsIndex.
        //
        // The full flow would be:
        // 1. Scan accounts index for candidates
        // 2. For each candidate, call check_rent_expiry()
        // 3. If expired, call archive_account()
        // 4. Update archive_index
        let _ = (current_slot, current_epoch);
    }

    /// Archive a specific account to cold storage.
    ///
    /// This is called by the maintenance service when an account is
    /// determined to be eligible for archival.
    pub fn archive_account_to_cold(
        &self,
        pubkey: &Pubkey,
        account: &AccountSharedData,
        last_active_slot: u64,
        current_slot: u64,
        current_epoch: u64,
    ) -> bool {
        // Check if eligible
        if !check_rent_expiry(account, last_active_slot, current_slot, &self.rent_config) {
            return false;
        }

        // Perform archival
        match archive_account(pubkey, account, current_slot, current_epoch, &self.rent_config) {
            Ok(archived) => {
                // Remove from hot cache
                self.hot_cache_remove(pubkey);

                // Add to archive index
                let mut index = self.archive_index.write().unwrap();
                index.insert(archived);

                self.stats.cold_archives.fetch_add(1, Ordering::Relaxed);
                info!("TRv1: Archived account {} to cold storage", pubkey);
                true
            }
            Err(e) => {
                warn!(
                    "TRv1: Failed to archive account {} to cold storage: {}",
                    pubkey, e
                );
                false
            }
        }
    }

    /// Check if an account is archived in cold storage.
    pub fn is_archived(&self, pubkey: &Pubkey) -> bool {
        let index = self.archive_index.read().unwrap();
        index.is_archived(pubkey)
    }

    /// Get a snapshot of the current tier statistics.
    pub fn stats(&self) -> TierStats {
        let mut stats = self.stats.snapshot();

        // Merge in the cache's own stats for account counts/sizes
        let cache = self.hot_cache.read().unwrap();
        let cache_stats = cache.stats();
        stats.hot_accounts = cache_stats.hot_accounts;
        stats.hot_size_bytes = cache_stats.hot_size_bytes;

        // Add archive index stats
        let archive = self.archive_index.read().unwrap();
        stats.cold_accounts = archive.len() as u64;

        stats
    }

    /// Get the current hot cache utilization as a ratio (0.0 - 1.0).
    pub fn hot_cache_utilization(&self) -> f64 {
        let cache = self.hot_cache.read().unwrap();
        cache.utilization()
    }

    /// Get the number of accounts currently in the hot cache.
    pub fn hot_cache_len(&self) -> usize {
        let cache = self.hot_cache.read().unwrap();
        cache.len()
    }

    /// Get a reference to the tiered storage configuration.
    pub fn config(&self) -> &TieredStorageConfig {
        &self.config
    }

    /// Get a reference to the rent configuration.
    pub fn rent_config(&self) -> &StateRentConfig {
        &self.rent_config
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use solana_account::{AccountSharedData, WritableAccount};
    use solana_pubkey::Pubkey;

    fn make_account(data_len: usize, lamports: u64) -> AccountSharedData {
        let owner = Pubkey::new_unique();
        let mut account = AccountSharedData::new(lamports, data_len, &owner);
        account.set_data_from_slice(&vec![42u8; data_len]);
        account
    }

    fn test_adapter() -> TRv1StorageAdapter {
        TRv1StorageAdapter::new(
            TieredStorageConfig::for_testing(),
            StateRentConfig::for_testing(),
        )
    }

    #[test]
    fn test_new_adapter() {
        let adapter = test_adapter();
        assert_eq!(adapter.hot_cache_len(), 0);
        assert_eq!(adapter.hot_cache_utilization(), 0.0);
    }

    #[test]
    fn test_cache_hit_path() {
        let adapter = test_adapter();
        let pubkey = Pubkey::new_unique();
        let account = make_account(100, 1000);

        // Insert into hot cache
        adapter.hot_cache_insert(&pubkey, &account);
        assert_eq!(adapter.hot_cache_len(), 1);

        // Should be a cache hit
        let loaded = adapter.hot_cache_get(&pubkey);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().data(), account.data());

        // Stats should reflect the hit
        let stats = adapter.stats();
        assert_eq!(stats.total_hits, 1);
    }

    #[test]
    fn test_cache_miss_path() {
        let adapter = test_adapter();
        let pubkey = Pubkey::new_unique();

        // Should be a cache miss
        let loaded = adapter.hot_cache_get(&pubkey);
        assert!(loaded.is_none());

        // Stats should reflect the miss
        let stats = adapter.stats();
        assert_eq!(stats.total_misses, 1);
    }

    #[test]
    fn test_cache_eviction() {
        // Create adapter with tiny cache (700 bytes)
        let config = TieredStorageConfig {
            hot_cache_size: 700,
            eviction_batch_size: 10,
            target_utilization: 0.90,
            ..TieredStorageConfig::for_testing()
        };
        let adapter = TRv1StorageAdapter::new(config, StateRentConfig::for_testing());

        // Insert several accounts to exceed cache
        for _ in 0..5 {
            let pk = Pubkey::new_unique();
            let account = make_account(50, 1000);
            adapter.hot_cache_insert(&pk, &account);
        }

        // Force eviction
        let evicted = adapter.maybe_evict();
        assert!(!evicted.is_empty(), "Should have evicted some accounts");

        // Stats should track evictions
        let stats = adapter.stats();
        assert!(stats.hot_to_warm_demotions > 0);
    }

    #[test]
    fn test_cache_remove() {
        let adapter = test_adapter();
        let pubkey = Pubkey::new_unique();
        let account = make_account(100, 1000);

        adapter.hot_cache_insert(&pubkey, &account);
        assert_eq!(adapter.hot_cache_len(), 1);

        let removed = adapter.hot_cache_remove(&pubkey);
        assert!(removed.is_some());
        assert_eq!(adapter.hot_cache_len(), 0);

        // Should now be a miss
        assert!(adapter.hot_cache_get(&pubkey).is_none());
    }

    #[test]
    fn test_archive_account_to_cold() {
        let adapter = test_adapter();
        let pubkey = Pubkey::new_unique();
        // Account with sufficient size for archival (>= MIN_ARCHIVAL_DATA_SIZE = 128)
        let account = make_account(256, 10_000);

        // Insert into hot cache first
        adapter.hot_cache_insert(&pubkey, &account);
        assert_eq!(adapter.hot_cache_len(), 1);

        // Archive it — last active slot 0, current slot = 2 days worth
        // (archive_after_days for testing = 1)
        let current_slot = 2 * crate::state_rent_expiry::ESTIMATED_SLOTS_PER_DAY;
        let archived = adapter.archive_account_to_cold(
            &pubkey,
            &account,
            0, // last_active_slot
            current_slot,
            1, // current_epoch
        );
        assert!(archived, "Account should have been archived");

        // Should be removed from hot cache
        assert_eq!(adapter.hot_cache_len(), 0);
        assert!(adapter.hot_cache_get(&pubkey).is_none());

        // Should be in the archive index
        assert!(adapter.is_archived(&pubkey));

        // Stats should reflect the archival
        let stats = adapter.stats();
        assert_eq!(stats.cold_accounts, 1);
    }

    #[test]
    fn test_archive_refuses_active_account() {
        let adapter = test_adapter();
        let pubkey = Pubkey::new_unique();
        let account = make_account(256, 10_000);

        // Account was recently active — should NOT be archived
        let current_slot = 100; // only 100 slots ago
        let archived = adapter.archive_account_to_cold(
            &pubkey,
            &account,
            50,   // last_active_slot
            current_slot,
            1,
        );
        assert!(!archived, "Recently active account should not be archived");
    }

    #[test]
    fn test_stats_accuracy() {
        let adapter = test_adapter();
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let account = make_account(100, 1000);

        // 2 inserts
        adapter.hot_cache_insert(&pk1, &account);
        adapter.hot_cache_insert(&pk2, &account);

        // 3 hits
        adapter.hot_cache_get(&pk1);
        adapter.hot_cache_get(&pk2);
        adapter.hot_cache_get(&pk1);

        // 2 misses
        adapter.hot_cache_get(&Pubkey::new_unique());
        adapter.hot_cache_get(&Pubkey::new_unique());

        let stats = adapter.stats();
        assert_eq!(stats.total_hits, 3);
        assert_eq!(stats.total_misses, 2);
        assert_eq!(stats.hot_accounts, 2);
        // Hit rate should be 3/5 = 0.60
        assert!((stats.cache_hit_rate - 0.60).abs() < 0.01);
    }

    #[test]
    fn test_maintenance_tick() {
        let adapter = test_adapter();
        let pk = Pubkey::new_unique();
        let account = make_account(100, 1000);
        adapter.hot_cache_insert(&pk, &account);

        // Maintenance tick should not panic
        adapter.maintenance_tick(1000, 1);

        let stats = adapter.stats();
        assert_eq!(stats.total_hits, 0); // maintenance_ticks is tracked in AtomicTierStats
    }
}

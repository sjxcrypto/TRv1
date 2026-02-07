//! TRv1 Account Cache — Hot-Tier LRU Cache
//!
//! This module implements the hot-tier LRU cache that sits in front of
//! the accounts database. It provides O(1) lookups and insertions for
//! recently-accessed accounts, dramatically reducing RAM requirements
//! compared to Solana's full memory-mapped Cloudbreak approach.
//!
//! # Design
//!
//! The cache uses a doubly-linked list threaded through a `HashMap` for
//! O(1) LRU operations. Each access moves the entry to the head of the
//! list; eviction removes from the tail.
//!
//! ```text
//!   ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
//!   │  HEAD    │───▶│  entry   │───▶│  entry   │───▶│  TAIL    │
//!   │ (newest) │◀───│          │◀───│          │◀───│ (oldest) │
//!   └──────────┘    └──────────┘    └──────────┘    └──────────┘
//!                                                     ▲ evict from here
//! ```
//!
//! # Thread Safety
//!
//! The current implementation is single-threaded (`&mut self` on get/insert).
//! For production use, this will be wrapped in a `parking_lot::Mutex` or
//! sharded across multiple caches keyed by pubkey prefix, similar to how
//! Solana's `ReadOnlyAccountsCache` uses `DashMap`.
//!
//! # Integration Plan
//!
//! Phase 1 (current): Standalone cache with explicit get/insert/evict.
//! Phase 2: Wire into `AccountsDb::do_load` as a layer before `read_only_accounts_cache`.
//! Phase 3: Background eviction thread that moves evicted accounts to warm storage.

use {
    crate::tiered_storage_config::{EvictionPolicy, TierStats, TieredStorageConfig},
    solana_account::{AccountSharedData, ReadableAccount},
    solana_pubkey::Pubkey,
    std::{
        collections::HashMap,
        time::Instant,
    },
};

// ── Internal Node Types ─────────────────────────────────────────────────────

/// Index into the `nodes` Vec, serving as a pointer for the linked list.
type NodeIndex = usize;

/// Sentinel value indicating no link (equivalent to null pointer).
const NIL: NodeIndex = usize::MAX;

/// A node in the doubly-linked LRU list.
///
/// Nodes are stored in a Vec and linked via indices rather than raw pointers,
/// providing memory safety without reference-counting overhead.
#[derive(Debug)]
struct LruNode {
    /// The account's public key.
    pubkey: Pubkey,

    /// The cached account data.
    account: CachedAccount,

    /// Index of the next (newer) node, or NIL if this is the head.
    next: NodeIndex,

    /// Index of the previous (older) node, or NIL if this is the tail.
    prev: NodeIndex,
}

/// Metadata stored alongside each cached account.
#[derive(Debug, Clone)]
struct CachedAccount {
    /// The actual account data.
    data: AccountSharedData,

    /// When this account was last accessed (for LRU/TTL decisions).
    last_accessed: Instant,

    /// Total number of times this account has been accessed (for LFU policy).
    access_count: u64,

    /// Size of this account's data in bytes (cached to avoid recomputing).
    data_len: u64,
}

impl CachedAccount {
    fn new(data: AccountSharedData) -> Self {
        let data_len = data.data().len() as u64;
        Self {
            data,
            last_accessed: Instant::now(),
            access_count: 1,
            data_len,
        }
    }

    /// Approximate memory footprint of this entry in bytes.
    ///
    /// Includes the AccountSharedData overhead plus the data buffer.
    fn memory_size(&self) -> u64 {
        // AccountSharedData struct overhead (~128 bytes) + data
        // Plus CachedAccount fields (~32 bytes)
        160 + self.data_len
    }
}

// ── Free List for Recycling Node Slots ──────────────────────────────────────

/// Manages recycled node indices to avoid Vec fragmentation.
#[derive(Debug, Default)]
struct FreeList {
    free_indices: Vec<NodeIndex>,
}

impl FreeList {
    fn push(&mut self, index: NodeIndex) {
        self.free_indices.push(index);
    }

    fn pop(&mut self) -> Option<NodeIndex> {
        self.free_indices.pop()
    }
}

// ── Account Cache ───────────────────────────────────────────────────────────

/// Hot-tier LRU cache for TRv1 accounts.
///
/// Provides O(1) lookups and LRU eviction, designed to keep the most
/// frequently-accessed accounts in RAM while allowing less-active accounts
/// to spill to warm storage (NVMe SSD).
///
/// # Capacity
///
/// The cache is bounded by `config.hot_cache_size` bytes. When the total
/// memory usage exceeds `config.hot_cache_size * config.target_utilization`,
/// `evict_to_warm()` should be called to free space.
pub struct AccountCache {
    /// Configuration controlling cache behavior.
    config: TieredStorageConfig,

    /// Map from Pubkey → index into the `nodes` vec.
    map: HashMap<Pubkey, NodeIndex>,

    /// Arena-allocated linked list nodes.
    nodes: Vec<LruNode>,

    /// Recycled node indices.
    free_list: FreeList,

    /// Index of the most recently accessed node (head of LRU list).
    head: NodeIndex,

    /// Index of the least recently accessed node (tail of LRU list).
    tail: NodeIndex,

    /// Current total memory usage of cached accounts, in bytes.
    current_size_bytes: u64,

    /// Running statistics.
    stats: TierStats,
}

impl std::fmt::Debug for AccountCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountCache")
            .field("cached_accounts", &self.map.len())
            .field("current_size_bytes", &self.current_size_bytes)
            .field("config.hot_cache_size", &self.config.hot_cache_size)
            .field("hit_rate", &self.cache_hit_rate())
            .finish()
    }
}

impl AccountCache {
    /// Create a new cache with the given configuration.
    ///
    /// The cache starts empty. Accounts are inserted via `insert()` and
    /// retrieved via `get()`.
    pub fn new(config: TieredStorageConfig) -> Self {
        let estimated_capacity = config.estimated_hot_capacity(256) as usize; // assume avg 256 bytes
        Self {
            config,
            map: HashMap::with_capacity(estimated_capacity),
            nodes: Vec::with_capacity(estimated_capacity.min(1_000_000)),
            free_list: FreeList::default(),
            head: NIL,
            tail: NIL,
            current_size_bytes: 0,
            stats: TierStats::default(),
        }
    }

    /// Look up an account by pubkey.
    ///
    /// If found, the account is promoted to the head of the LRU list
    /// (most recently used). Returns `None` if the pubkey is not cached.
    ///
    /// This counts as a cache hit or miss for statistics tracking.
    pub fn get(&mut self, pubkey: &Pubkey) -> Option<&AccountSharedData> {
        if let Some(&node_idx) = self.map.get(pubkey) {
            // Cache hit
            self.stats.total_hits = self.stats.total_hits.saturating_add(1);
            self.stats.recalculate_rates();

            // Update access metadata
            self.nodes[node_idx].account.last_accessed = Instant::now();
            self.nodes[node_idx].account.access_count =
                self.nodes[node_idx].account.access_count.saturating_add(1);

            // Move to head (most recently used)
            self.move_to_head(node_idx);

            Some(&self.nodes[node_idx].account.data)
        } else {
            // Cache miss
            self.stats.total_misses = self.stats.total_misses.saturating_add(1);
            self.stats.recalculate_rates();
            None
        }
    }

    /// Check if a pubkey is in the cache without updating LRU order.
    pub fn contains(&self, pubkey: &Pubkey) -> bool {
        self.map.contains_key(pubkey)
    }

    /// Insert an account into the cache.
    ///
    /// If the pubkey already exists, its data is updated and it is
    /// promoted to the head of the LRU list. If it's a new entry,
    /// it is added at the head.
    ///
    /// **Note**: This does NOT automatically evict. Call `needs_eviction()`
    /// and `evict_to_warm()` to manage cache pressure.
    pub fn insert(&mut self, pubkey: Pubkey, account: AccountSharedData) {
        let cached = CachedAccount::new(account);
        let entry_size = cached.memory_size();

        if let Some(&existing_idx) = self.map.get(&pubkey) {
            // Update existing entry
            let old_size = self.nodes[existing_idx].account.memory_size();
            self.nodes[existing_idx].account = cached;
            self.current_size_bytes = self
                .current_size_bytes
                .saturating_sub(old_size)
                .saturating_add(entry_size);
            self.move_to_head(existing_idx);
        } else {
            // New entry
            let node = LruNode {
                pubkey,
                account: cached,
                next: NIL,
                prev: NIL,
            };

            let node_idx = self.alloc_node(node);
            self.map.insert(pubkey, node_idx);
            self.push_head(node_idx);
            self.current_size_bytes = self.current_size_bytes.saturating_add(entry_size);
        }

        // Update stats
        self.stats.hot_accounts = self.map.len() as u64;
        self.stats.hot_size_bytes = self.current_size_bytes;
    }

    /// Remove a specific account from the cache.
    ///
    /// Returns the account data if it was present.
    pub fn remove(&mut self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        if let Some(node_idx) = self.map.remove(pubkey) {
            let size = self.nodes[node_idx].account.memory_size();
            self.current_size_bytes = self.current_size_bytes.saturating_sub(size);
            self.unlink(node_idx);
            self.free_list.push(node_idx);

            self.stats.hot_accounts = self.map.len() as u64;
            self.stats.hot_size_bytes = self.current_size_bytes;

            Some(self.nodes[node_idx].account.data.clone())
        } else {
            None
        }
    }

    /// Returns `true` if the cache exceeds the eviction watermark.
    ///
    /// When this returns `true`, `evict_to_warm()` should be called
    /// (typically by a background thread) to free space.
    pub fn needs_eviction(&self) -> bool {
        self.current_size_bytes > self.config.eviction_watermark()
    }

    /// Evict the least-recently-used accounts to make room in the cache.
    ///
    /// Returns a vector of `(Pubkey, AccountSharedData)` pairs that were
    /// evicted. The caller is responsible for persisting these to warm
    /// storage (NVMe SSD).
    ///
    /// Evicts up to `config.eviction_batch_size` accounts, or until the
    /// cache is below the eviction watermark, whichever comes first.
    pub fn evict_to_warm(&mut self) -> Vec<(Pubkey, AccountSharedData)> {
        let watermark = self.config.eviction_watermark();
        let mut evicted = Vec::new();

        match self.config.eviction_policy {
            EvictionPolicy::LRU => {
                self.evict_lru(watermark, &mut evicted);
            }
            EvictionPolicy::LFU => {
                self.evict_lfu(watermark, &mut evicted);
            }
            EvictionPolicy::ARC => {
                // ARC falls back to LRU for now; full ARC with ghost lists
                // will be implemented in a subsequent phase.
                self.evict_lru(watermark, &mut evicted);
            }
        }

        // Update stats
        let evicted_count = evicted.len() as u64;
        self.stats.hot_to_warm_demotions = self
            .stats
            .hot_to_warm_demotions
            .saturating_add(evicted_count);
        self.stats.hot_accounts = self.map.len() as u64;
        self.stats.hot_size_bytes = self.current_size_bytes;

        evicted
    }

    /// LRU eviction: remove from tail (oldest) until below watermark.
    fn evict_lru(
        &mut self,
        watermark: u64,
        evicted: &mut Vec<(Pubkey, AccountSharedData)>,
    ) {
        let mut count = 0;
        while self.current_size_bytes > watermark
            && count < self.config.eviction_batch_size
            && self.tail != NIL
        {
            let tail_idx = self.tail;
            let pubkey = self.nodes[tail_idx].pubkey;
            let account_data = self.nodes[tail_idx].account.data.clone();
            let size = self.nodes[tail_idx].account.memory_size();

            self.map.remove(&pubkey);
            self.unlink(tail_idx);
            self.free_list.push(tail_idx);
            self.current_size_bytes = self.current_size_bytes.saturating_sub(size);

            evicted.push((pubkey, account_data));
            count += 1;
        }
    }

    /// LFU eviction: scan for least-frequently-used accounts.
    ///
    /// This is O(n) in the worst case. For production, a min-heap
    /// indexed by access_count would be more efficient.
    fn evict_lfu(
        &mut self,
        watermark: u64,
        evicted: &mut Vec<(Pubkey, AccountSharedData)>,
    ) {
        let mut count = 0;
        while self.current_size_bytes > watermark
            && count < self.config.eviction_batch_size
            && !self.map.is_empty()
        {
            // Find the entry with the lowest access_count.
            // Walk from the tail (LRU order) and pick the one with lowest access_count
            // among the bottom quarter of entries (amortize the scan).
            let scan_limit = (self.map.len() / 4).max(1);
            let mut min_count = u64::MAX;
            let mut min_idx = NIL;

            let mut cursor = self.tail;
            let mut scanned = 0;
            while cursor != NIL && scanned < scan_limit {
                let node = &self.nodes[cursor];
                if node.account.access_count < min_count {
                    min_count = node.account.access_count;
                    min_idx = cursor;
                }
                cursor = node.next; // move toward head (newer)
                scanned += 1;
            }

            if min_idx == NIL {
                break;
            }

            let pubkey = self.nodes[min_idx].pubkey;
            let account_data = self.nodes[min_idx].account.data.clone();
            let size = self.nodes[min_idx].account.memory_size();

            self.map.remove(&pubkey);
            self.unlink(min_idx);
            self.free_list.push(min_idx);
            self.current_size_bytes = self.current_size_bytes.saturating_sub(size);

            evicted.push((pubkey, account_data));
            count += 1;
        }
    }

    /// Return a snapshot of the current tier statistics.
    pub fn stats(&self) -> &TierStats {
        &self.stats
    }

    /// Return the current cache hit rate as a ratio (0.0 - 1.0).
    pub fn cache_hit_rate(&self) -> f64 {
        self.stats.cache_hit_rate
    }

    /// Return the number of accounts currently cached.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Return `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Return current memory usage in bytes.
    pub fn current_size_bytes(&self) -> u64 {
        self.current_size_bytes
    }

    /// Return the configured maximum cache size in bytes.
    pub fn max_size_bytes(&self) -> u64 {
        self.config.hot_cache_size
    }

    /// Return the cache utilization as a ratio (0.0 - 1.0).
    pub fn utilization(&self) -> f64 {
        if self.config.hot_cache_size == 0 {
            return 0.0;
        }
        self.current_size_bytes as f64 / self.config.hot_cache_size as f64
    }

    // ── Linked List Operations ──────────────────────────────────────────

    /// Allocate a node, recycling from the free list if possible.
    fn alloc_node(&mut self, node: LruNode) -> NodeIndex {
        if let Some(idx) = self.free_list.pop() {
            self.nodes[idx] = node;
            idx
        } else {
            let idx = self.nodes.len();
            self.nodes.push(node);
            idx
        }
    }

    /// Insert a node at the head of the list.
    fn push_head(&mut self, idx: NodeIndex) {
        self.nodes[idx].prev = NIL;
        self.nodes[idx].next = self.head;

        if self.head != NIL {
            self.nodes[self.head].prev = idx;
        }
        self.head = idx;

        if self.tail == NIL {
            self.tail = idx;
        }
    }

    /// Remove a node from its current position in the list.
    fn unlink(&mut self, idx: NodeIndex) {
        let prev = self.nodes[idx].prev;
        let next = self.nodes[idx].next;

        if prev != NIL {
            self.nodes[prev].next = next;
        } else {
            // idx was the head
            self.head = next;
        }

        if next != NIL {
            self.nodes[next].prev = prev;
        } else {
            // idx was the tail
            self.tail = prev;
        }

        self.nodes[idx].prev = NIL;
        self.nodes[idx].next = NIL;
    }

    /// Move an existing node to the head of the list.
    fn move_to_head(&mut self, idx: NodeIndex) {
        if idx == self.head {
            return; // already at head
        }
        self.unlink(idx);
        self.push_head(idx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_account::AccountSharedData;
    use solana_pubkey::Pubkey;

    fn make_account(data_len: usize) -> AccountSharedData {
        let mut account = AccountSharedData::default();
        account.set_data_from_slice(&vec![0u8; data_len]);
        account
    }

    fn test_config(cache_size: u64) -> TieredStorageConfig {
        TieredStorageConfig {
            hot_cache_size: cache_size,
            eviction_batch_size: 10,
            target_utilization: 0.90,
            ..TieredStorageConfig::for_testing()
        }
    }

    #[test]
    fn test_new_cache_is_empty() {
        let cache = AccountCache::new(TieredStorageConfig::for_testing());
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.current_size_bytes(), 0);
    }

    #[test]
    fn test_insert_and_get() {
        let mut cache = AccountCache::new(test_config(1_000_000));
        let pubkey = Pubkey::new_unique();
        let account = make_account(100);

        cache.insert(pubkey, account.clone());
        assert_eq!(cache.len(), 1);
        assert!(cache.contains(&pubkey));

        let retrieved = cache.get(&pubkey).unwrap();
        assert_eq!(retrieved.data(), account.data());
    }

    #[test]
    fn test_get_miss_returns_none() {
        let mut cache = AccountCache::new(test_config(1_000_000));
        let pubkey = Pubkey::new_unique();
        assert!(cache.get(&pubkey).is_none());
        assert_eq!(cache.stats().total_misses, 1);
    }

    #[test]
    fn test_insert_updates_existing() {
        let mut cache = AccountCache::new(test_config(1_000_000));
        let pubkey = Pubkey::new_unique();

        cache.insert(pubkey, make_account(100));
        let size_after_first = cache.current_size_bytes();

        cache.insert(pubkey, make_account(200));
        assert_eq!(cache.len(), 1); // still just one entry
        assert!(cache.current_size_bytes() > size_after_first);

        let retrieved = cache.get(&pubkey).unwrap();
        assert_eq!(retrieved.data().len(), 200);
    }

    #[test]
    fn test_remove() {
        let mut cache = AccountCache::new(test_config(1_000_000));
        let pubkey = Pubkey::new_unique();
        cache.insert(pubkey, make_account(100));

        let removed = cache.remove(&pubkey);
        assert!(removed.is_some());
        assert!(cache.is_empty());
        assert!(!cache.contains(&pubkey));
    }

    #[test]
    fn test_lru_eviction_order() {
        // Small cache that can hold ~3 entries
        let mut cache = AccountCache::new(test_config(700));

        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let pk3 = Pubkey::new_unique();
        let pk4 = Pubkey::new_unique();

        // Insert 4 accounts, each ~160+10 = 170 bytes overhead
        cache.insert(pk1, make_account(10));
        cache.insert(pk2, make_account(10));
        cache.insert(pk3, make_account(10));
        cache.insert(pk4, make_account(10));

        // Cache should be over the watermark (700 * 0.9 = 630)
        // 4 * 170 = 680, which is > 630
        assert!(cache.needs_eviction());

        let evicted = cache.evict_to_warm();
        // Should have evicted the oldest (pk1) to get below watermark
        assert!(!evicted.is_empty());

        // pk1 should have been evicted first (it's the oldest)
        let evicted_pubkeys: Vec<Pubkey> = evicted.iter().map(|(pk, _)| *pk).collect();
        assert!(evicted_pubkeys.contains(&pk1));
    }

    #[test]
    fn test_get_promotes_to_head() {
        let mut cache = AccountCache::new(test_config(100_000));

        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let pk3 = Pubkey::new_unique();

        cache.insert(pk1, make_account(10));
        cache.insert(pk2, make_account(10));
        cache.insert(pk3, make_account(10));

        // pk1 is currently the oldest (tail). Access it to promote.
        cache.get(&pk1);
        // Now pk2 should be the oldest (tail).

        // Verify by checking the tail
        let tail_pubkey = cache.nodes[cache.tail].pubkey;
        assert_eq!(tail_pubkey, pk2);
    }

    #[test]
    fn test_cache_hit_rate() {
        let mut cache = AccountCache::new(test_config(1_000_000));
        let pk1 = Pubkey::new_unique();
        cache.insert(pk1, make_account(100));

        // 3 hits
        cache.get(&pk1);
        cache.get(&pk1);
        cache.get(&pk1);

        // 1 miss
        cache.get(&Pubkey::new_unique());

        // Hit rate should be 3/4 = 0.75
        assert!((cache.cache_hit_rate() - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_utilization() {
        let mut cache = AccountCache::new(test_config(1000));
        assert_eq!(cache.utilization(), 0.0);

        cache.insert(Pubkey::new_unique(), make_account(10));
        assert!(cache.utilization() > 0.0);
    }

    #[test]
    fn test_stats_tracking() {
        let mut cache = AccountCache::new(test_config(1_000_000));
        let pk1 = Pubkey::new_unique();
        cache.insert(pk1, make_account(100));

        assert_eq!(cache.stats().hot_accounts, 1);
        assert!(cache.stats().hot_size_bytes > 0);

        cache.get(&pk1);
        assert_eq!(cache.stats().total_hits, 1);

        cache.get(&Pubkey::new_unique());
        assert_eq!(cache.stats().total_misses, 1);
    }

    #[test]
    fn test_lfu_eviction() {
        let mut cache = AccountCache::new(TieredStorageConfig {
            hot_cache_size: 700,
            eviction_policy: EvictionPolicy::LFU,
            eviction_batch_size: 10,
            target_utilization: 0.90,
            ..TieredStorageConfig::for_testing()
        });

        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let pk3 = Pubkey::new_unique();
        let pk4 = Pubkey::new_unique();

        cache.insert(pk1, make_account(10));
        cache.insert(pk2, make_account(10));
        cache.insert(pk3, make_account(10));

        // Access pk1 many times to increase its frequency
        for _ in 0..10 {
            cache.get(&pk1);
        }
        // Access pk3 a few times
        for _ in 0..3 {
            cache.get(&pk3);
        }
        // pk2 has access_count = 1 (just the insert)

        cache.insert(pk4, make_account(10));

        // Force eviction
        if cache.needs_eviction() {
            let evicted = cache.evict_to_warm();
            // pk2 should be evicted first (lowest frequency)
            if !evicted.is_empty() {
                let evicted_pubkeys: Vec<Pubkey> = evicted.iter().map(|(pk, _)| *pk).collect();
                // pk2 should be among the evicted since it has the lowest access count
                assert!(evicted_pubkeys.contains(&pk2));
            }
        }
    }

    #[test]
    fn test_many_insertions_and_evictions() {
        let mut cache = AccountCache::new(test_config(10_000));

        // Insert 100 accounts
        let pubkeys: Vec<Pubkey> = (0..100).map(|_| Pubkey::new_unique()).collect();
        for pk in &pubkeys {
            cache.insert(*pk, make_account(50));
        }

        // Evict
        while cache.needs_eviction() {
            let evicted = cache.evict_to_warm();
            if evicted.is_empty() {
                break;
            }
        }

        // Cache should be below watermark now
        assert!(!cache.needs_eviction() || cache.is_empty());
        assert!(cache.stats().hot_to_warm_demotions > 0);
    }
}

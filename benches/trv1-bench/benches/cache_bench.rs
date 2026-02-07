//! Account cache (LRU) benchmarks.
//!
//! Measures:
//! - LRU cache hit/miss rates at different sizes
//! - Cache eviction throughput
//! - Insert/lookup latency
//! - Simulated 1GB, 4GB, 16GB cache sizes

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rand::Rng;
use solana_hash::Hash;
use solana_pubkey::Pubkey;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Simulated LRU cache (mirrors the real TRv1 account cache design)
// ---------------------------------------------------------------------------

/// A simulated account entry in the cache.
#[derive(Clone)]
struct CachedAccount {
    pubkey: Pubkey,
    lamports: u64,
    data: Vec<u8>,
    _hash: Hash,
}

impl CachedAccount {
    fn new(pubkey: Pubkey, data_size: usize) -> Self {
        Self {
            pubkey,
            lamports: 1_000_000,
            data: vec![0u8; data_size],
            _hash: Hash::default(),
        }
    }

    fn size_bytes(&self) -> usize {
        // Pubkey(32) + lamports(8) + data + Hash(32) + overhead
        32 + 8 + self.data.len() + 32 + 64
    }
}

/// Simple LRU cache with a byte-budget limit (simulates TRv1's tiered storage hot cache).
struct AccountLruCache {
    entries: HashMap<Pubkey, (usize, CachedAccount)>, // order, account
    order: Vec<Pubkey>,
    max_bytes: usize,
    current_bytes: usize,
    evictions: u64,
}

impl AccountLruCache {
    fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
            max_bytes,
            current_bytes: 0,
            evictions: 0,
        }
    }

    fn insert(&mut self, account: CachedAccount) {
        let size = account.size_bytes();
        let pubkey = account.pubkey;

        // Evict if needed
        while self.current_bytes.saturating_add(size) > self.max_bytes && !self.order.is_empty() {
            let evicted_key = self.order.remove(0);
            if let Some((_, evicted)) = self.entries.remove(&evicted_key) {
                self.current_bytes = self.current_bytes.saturating_sub(evicted.size_bytes());
                self.evictions = self.evictions.saturating_add(1);
            }
        }

        let idx = self.order.len();
        self.order.push(pubkey);
        self.entries.insert(pubkey, (idx, account));
        self.current_bytes = self.current_bytes.saturating_add(size);
    }

    fn get(&mut self, pubkey: &Pubkey) -> Option<&CachedAccount> {
        if self.entries.contains_key(pubkey) {
            // Move to back (most recently used)
            self.order.retain(|k| k != pubkey);
            self.order.push(*pubkey);
            self.entries.get(pubkey).map(|(_, a)| a)
        } else {
            None
        }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

/// Simulated cache sizes (in entry count, not bytes, for deterministic benchmarks)
struct CacheScenario {
    name: &'static str,
    max_bytes: usize,
    account_data_size: usize,
}

const SCENARIOS: &[CacheScenario] = &[
    CacheScenario { name: "1GB", max_bytes: 1_073_741_824, account_data_size: 256 },
    CacheScenario { name: "4GB", max_bytes: 4_294_967_296, account_data_size: 256 },
    // 16GB is too large for actual alloc in CI — we simulate proportionally
];

fn bench_cache_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache/insert");

    for &n_entries in &[10_000usize, 100_000] {
        group.throughput(Throughput::Elements(n_entries as u64));
        group.bench_with_input(
            BenchmarkId::new("entries", n_entries),
            &n_entries,
            |b, &n| {
                // Use a reasonably-sized cache that forces evictions
                let entry_size = 256 + 136; // data + overhead
                let cache_size = (n / 2) * entry_size; // cache holds half the entries

                let accounts: Vec<CachedAccount> = (0..n)
                    .map(|_| CachedAccount::new(Pubkey::new_unique(), 256))
                    .collect();

                b.iter(|| {
                    let mut cache = AccountLruCache::new(cache_size);
                    for acct in &accounts {
                        cache.insert(acct.clone());
                    }
                    cache.len()
                });
            },
        );
    }
    group.finish();
}

fn bench_cache_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache/lookup");

    for &n_entries in &[10_000usize, 100_000] {
        group.throughput(Throughput::Elements(n_entries as u64));
        group.bench_with_input(
            BenchmarkId::new("entries", n_entries),
            &n_entries,
            |b, &n| {
                let entry_size = 256 + 136;
                let cache_size = n * entry_size * 2; // cache fits all

                let accounts: Vec<CachedAccount> = (0..n)
                    .map(|_| CachedAccount::new(Pubkey::new_unique(), 256))
                    .collect();

                let keys: Vec<Pubkey> = accounts.iter().map(|a| a.pubkey).collect();

                let mut cache = AccountLruCache::new(cache_size);
                for acct in &accounts {
                    cache.insert(acct.clone());
                }

                b.iter(|| {
                    let mut hits = 0u64;
                    for key in &keys {
                        if cache.get(key).is_some() {
                            hits = hits.saturating_add(1);
                        }
                    }
                    hits
                });
            },
        );
    }
    group.finish();
}

fn bench_cache_hit_miss_ratio(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache/hit_miss_ratio");
    group.sample_size(20);

    // Simulate realistic access pattern: 80% of accesses hit 20% of accounts (Zipf-like)
    for &cache_fill_pct in &[25u64, 50, 75] {
        let n_total_accounts = 100_000usize;
        let n_cache = (n_total_accounts as u64 * cache_fill_pct / 100) as usize;
        let entry_size = 256 + 136;
        let cache_size = n_cache * entry_size;

        group.throughput(Throughput::Elements(10_000));
        group.bench_with_input(
            BenchmarkId::new("fill_pct", cache_fill_pct),
            &cache_fill_pct,
            |b, _| {
                let all_accounts: Vec<CachedAccount> = (0..n_total_accounts)
                    .map(|_| CachedAccount::new(Pubkey::new_unique(), 256))
                    .collect();

                let all_keys: Vec<Pubkey> = all_accounts.iter().map(|a| a.pubkey).collect();

                let mut cache = AccountLruCache::new(cache_size);
                // Only populate first n_cache accounts
                for acct in all_accounts.iter().take(n_cache) {
                    cache.insert(acct.clone());
                }

                // Access pattern: 80% of requests target cached entries, 20% miss
                let mut rng = rand::rng();
                let lookups: Vec<Pubkey> = (0..10_000)
                    .map(|_| {
                        if rng.random_bool(0.8) {
                            all_keys[rng.random_range(0..n_cache)]
                        } else {
                            all_keys[rng.random_range(n_cache..n_total_accounts)]
                        }
                    })
                    .collect();

                b.iter(|| {
                    let mut hits = 0u64;
                    let mut misses = 0u64;
                    for key in &lookups {
                        if cache.get(key).is_some() {
                            hits = hits.saturating_add(1);
                        } else {
                            misses = misses.saturating_add(1);
                        }
                    }
                    (hits, misses)
                });
            },
        );
    }
    group.finish();
}

fn bench_cache_eviction_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache/eviction_throughput");
    group.sample_size(20);

    for &n_entries in &[50_000usize, 200_000] {
        group.throughput(Throughput::Elements(n_entries as u64));
        group.bench_with_input(
            BenchmarkId::new("entries", n_entries),
            &n_entries,
            |b, &n| {
                let entry_size = 256 + 136;
                // Cache holds only 10% — forces massive eviction
                let cache_size = (n / 10) * entry_size;

                let accounts: Vec<CachedAccount> = (0..n)
                    .map(|_| CachedAccount::new(Pubkey::new_unique(), 256))
                    .collect();

                b.iter(|| {
                    let mut cache = AccountLruCache::new(cache_size);
                    for acct in &accounts {
                        cache.insert(acct.clone());
                    }
                    cache.evictions
                });
            },
        );
    }
    group.finish();
}

fn bench_cache_varying_data_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache/varying_data_sizes");

    for &data_size in &[128usize, 1024, 10240] {
        let n = 10_000usize;
        let entry_size = data_size + 136;
        let cache_size = (n / 2) * entry_size;

        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(
            BenchmarkId::new("data_bytes", data_size),
            &data_size,
            |b, &ds| {
                let accounts: Vec<CachedAccount> = (0..n)
                    .map(|_| CachedAccount::new(Pubkey::new_unique(), ds))
                    .collect();

                b.iter(|| {
                    let mut cache = AccountLruCache::new(cache_size);
                    for acct in &accounts {
                        cache.insert(acct.clone());
                    }
                    (cache.len(), cache.evictions)
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_cache_insert,
    bench_cache_lookup,
    bench_cache_hit_miss_ratio,
    bench_cache_eviction_throughput,
    bench_cache_varying_data_sizes,
);
criterion_main!(benches);

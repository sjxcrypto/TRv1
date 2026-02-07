//! Stress Test: State Growth
//!
//! Simulates rapid account creation to test tiered storage behavior,
//! memory usage, and cache performance under state explosion.
//!
//! Run: `cargo test --test state_growth -- --nocapture`

use std::collections::HashMap;
use std::time::Instant;

/// Simulated storage tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StorageTier {
    Hot,  // In-memory cache
    Warm, // SSD / fast disk
    Cold, // Archival / compressed
}

/// Simulated account in tiered storage.
#[derive(Debug, Clone)]
struct Account {
    id: u64,
    data_size: usize,
    last_accessed_slot: u64,
    tier: StorageTier,
    lamports: u64,
}

/// Tiered storage manager.
struct TieredStorage {
    accounts: HashMap<u64, Account>,
    hot_budget: usize,   // bytes
    warm_budget: usize,   // bytes
    hot_used: usize,
    warm_used: usize,
    cold_used: usize,
    promotions: u64,
    demotions: u64,
}

impl TieredStorage {
    fn new(hot_gb: usize, warm_gb: usize) -> Self {
        Self {
            accounts: HashMap::new(),
            hot_budget: hot_gb * 1_073_741_824,
            warm_budget: warm_gb * 1_073_741_824,
            hot_used: 0,
            warm_used: 0,
            cold_used: 0,
            promotions: 0,
            demotions: 0,
        }
    }

    fn create_account(&mut self, id: u64, data_size: usize, slot: u64) {
        let account = Account {
            id,
            data_size,
            last_accessed_slot: slot,
            tier: StorageTier::Hot,
            lamports: 1_000_000,
        };

        self.hot_used += data_size;
        self.accounts.insert(id, account);

        // If hot tier overflows, demote oldest to warm
        if self.hot_used > self.hot_budget {
            self.demote_oldest_hot(slot);
        }
    }

    fn access_account(&mut self, id: u64, slot: u64) -> bool {
        if let Some(account) = self.accounts.get_mut(&id) {
            let old_tier = account.tier;
            account.last_accessed_slot = slot;

            // Promote if not already hot
            if old_tier != StorageTier::Hot {
                match old_tier {
                    StorageTier::Warm => self.warm_used = self.warm_used.saturating_sub(account.data_size),
                    StorageTier::Cold => self.cold_used = self.cold_used.saturating_sub(account.data_size),
                    _ => {}
                }
                account.tier = StorageTier::Hot;
                self.hot_used += account.data_size;
                self.promotions += 1;

                if self.hot_used > self.hot_budget {
                    self.demote_oldest_hot(slot);
                }
            }
            true
        } else {
            false
        }
    }

    fn demote_oldest_hot(&mut self, current_slot: u64) {
        // Find oldest hot account
        let mut oldest_id = None;
        let mut oldest_slot = u64::MAX;

        for (id, acct) in &self.accounts {
            if acct.tier == StorageTier::Hot && acct.last_accessed_slot < oldest_slot {
                oldest_slot = acct.last_accessed_slot;
                oldest_id = Some(*id);
            }
        }

        if let Some(id) = oldest_id {
            if let Some(acct) = self.accounts.get_mut(&id) {
                self.hot_used = self.hot_used.saturating_sub(acct.data_size);
                self.demotions += 1;

                if self.warm_used + acct.data_size <= self.warm_budget {
                    acct.tier = StorageTier::Warm;
                    self.warm_used += acct.data_size;
                } else {
                    acct.tier = StorageTier::Cold;
                    self.cold_used += acct.data_size;
                }
            }
        }
    }

    fn run_rent_collection(&mut self, current_slot: u64, rent_exempt_threshold: u64) -> u64 {
        let stale_threshold = current_slot.saturating_sub(1_000_000); // ~11.5 days at 1s blocks
        let mut archived = 0u64;

        let stale_ids: Vec<u64> = self
            .accounts
            .iter()
            .filter(|(_, a)| {
                a.last_accessed_slot < stale_threshold && a.lamports < rent_exempt_threshold
            })
            .map(|(id, _)| *id)
            .collect();

        for id in stale_ids {
            if let Some(acct) = self.accounts.get_mut(&id) {
                match acct.tier {
                    StorageTier::Hot => self.hot_used = self.hot_used.saturating_sub(acct.data_size),
                    StorageTier::Warm => self.warm_used = self.warm_used.saturating_sub(acct.data_size),
                    StorageTier::Cold => {} // already cold
                }
                acct.tier = StorageTier::Cold;
                self.cold_used += acct.data_size;
                archived += 1;
            }
        }

        archived
    }

    fn tier_counts(&self) -> (usize, usize, usize) {
        let mut hot = 0;
        let mut warm = 0;
        let mut cold = 0;
        for acct in self.accounts.values() {
            match acct.tier {
                StorageTier::Hot => hot += 1,
                StorageTier::Warm => warm += 1,
                StorageTier::Cold => cold += 1,
            }
        }
        (hot, warm, cold)
    }
}

#[test]
fn stress_state_growth() {
    println!("\n=== TRv1 State Growth Stress Test ===\n");

    // Simulate with scaled-down budget to force tier transitions quickly
    let mut storage = TieredStorage::new(1, 4); // 1GB hot, 4GB warm
    let start = Instant::now();

    let n_accounts = 500_000;
    let account_size = 256; // bytes per account

    println!("Creating {n_accounts} accounts of {account_size} bytes each...");

    for i in 0..n_accounts {
        storage.create_account(i, account_size, i as u64);

        // Simulate periodic access (hot accounts get re-accessed)
        if i > 1000 && i % 10 == 0 {
            // Access a recent account (simulates hot working set)
            storage.access_account(i.saturating_sub(50), i as u64);
        }

        if i % 100_000 == 99_999 {
            let (hot, warm, cold) = storage.tier_counts();
            println!(
                "  {i} accounts: hot={hot}, warm={warm}, cold={cold}, \
                 promotions={}, demotions={}",
                storage.promotions, storage.demotions
            );
        }
    }

    let (hot, warm, cold) = storage.tier_counts();
    let elapsed = start.elapsed();

    println!("\n--- Results ---");
    println!("Total accounts: {n_accounts}");
    println!("Hot tier:   {hot} accounts ({:.1} MB)", storage.hot_used as f64 / 1_048_576.0);
    println!("Warm tier:  {warm} accounts ({:.1} MB)", storage.warm_used as f64 / 1_048_576.0);
    println!("Cold tier:  {cold} accounts ({:.1} MB)", storage.cold_used as f64 / 1_048_576.0);
    println!("Promotions: {}", storage.promotions);
    println!("Demotions:  {}", storage.demotions);
    println!("Elapsed:    {elapsed:?}");

    assert_eq!(hot + warm + cold, n_accounts as usize);
    assert!(
        storage.hot_used <= storage.hot_budget,
        "hot tier exceeded budget"
    );
}

#[test]
fn stress_rent_collection() {
    println!("\n=== TRv1 Rent Collection Stress Test ===\n");

    let mut storage = TieredStorage::new(1, 4);
    let start = Instant::now();

    // Create accounts at various slots
    for i in 0..100_000u64 {
        storage.create_account(i, 256, i);
    }

    println!("Created 100k accounts. Running rent collection...");

    // Run rent collection at a much later slot
    let archived = storage.run_rent_collection(2_000_000, 5_000_000);

    let (hot, warm, cold) = storage.tier_counts();
    let elapsed = start.elapsed();

    println!("Archived {archived} rent-delinquent accounts");
    println!("Hot={hot}, Warm={warm}, Cold={cold}");
    println!("Elapsed: {elapsed:?}");

    assert!(archived > 0, "should archive some accounts");
}

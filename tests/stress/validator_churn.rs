//! Stress Test: Validator Churn
//!
//! Simulates validators rapidly joining, leaving, and being jailed
//! to test consensus stability and validator set management.
//!
//! Run: `cargo test --test validator_churn -- --nocapture`

use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// Validator states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValidatorStatus {
    Active,
    Standby,
    Jailed,
    Exited,
}

/// Simulated validator.
#[derive(Debug, Clone)]
struct Validator {
    id: u64,
    stake: u64,
    status: ValidatorStatus,
    missed_proposals: u32,
    jail_until_epoch: u64,
}

impl Validator {
    fn new(id: u64, stake: u64) -> Self {
        Self {
            id,
            stake,
            status: ValidatorStatus::Active,
            missed_proposals: 0,
            jail_until_epoch: 0,
        }
    }
}

/// Simulated validator set manager.
struct ValidatorSetManager {
    validators: HashMap<u64, Validator>,
    next_id: u64,
    max_active: usize,
    jail_threshold: u32,      // missed proposals before jailing
    jail_duration_epochs: u64, // how long jail lasts
}

impl ValidatorSetManager {
    fn new(max_active: usize) -> Self {
        Self {
            validators: HashMap::new(),
            next_id: 0,
            max_active,
            jail_threshold: 3,
            jail_duration_epochs: 5,
        }
    }

    fn add_validator(&mut self, stake: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let mut v = Validator::new(id, stake);

        let active_count = self.active_count();
        if active_count < self.max_active {
            v.status = ValidatorStatus::Active;
        } else {
            v.status = ValidatorStatus::Standby;
        }

        self.validators.insert(id, v);
        id
    }

    fn remove_validator(&mut self, id: u64) -> bool {
        if let Some(v) = self.validators.get_mut(&id) {
            v.status = ValidatorStatus::Exited;
            true
        } else {
            false
        }
    }

    fn jail_validator(&mut self, id: u64, current_epoch: u64) -> bool {
        if let Some(v) = self.validators.get_mut(&id) {
            v.status = ValidatorStatus::Jailed;
            v.jail_until_epoch = current_epoch + self.jail_duration_epochs;
            true
        } else {
            false
        }
    }

    fn record_missed_proposal(&mut self, id: u64, current_epoch: u64) {
        let jail_threshold = self.jail_threshold;
        let jail_duration = self.jail_duration_epochs;
        if let Some(v) = self.validators.get_mut(&id) {
            v.missed_proposals += 1;
            if v.missed_proposals >= jail_threshold {
                v.status = ValidatorStatus::Jailed;
                v.jail_until_epoch = current_epoch + jail_duration;
                v.missed_proposals = 0;
            }
        }
    }

    fn process_epoch(&mut self, current_epoch: u64) {
        // Unjail validators whose jail period expired
        let to_unjail: Vec<u64> = self
            .validators
            .iter()
            .filter(|(_, v)| v.status == ValidatorStatus::Jailed && v.jail_until_epoch <= current_epoch)
            .map(|(id, _)| *id)
            .collect();

        for id in to_unjail {
            if let Some(v) = self.validators.get_mut(&id) {
                v.status = ValidatorStatus::Standby;
            }
        }

        // Promote standby validators if active count is below max
        let active_count = self.active_count();
        if active_count < self.max_active {
            let deficit = self.max_active - active_count;
            let mut standby: Vec<_> = self
                .validators
                .iter()
                .filter(|(_, v)| v.status == ValidatorStatus::Standby)
                .map(|(id, v)| (*id, v.stake))
                .collect();
            standby.sort_by(|a, b| b.1.cmp(&a.1)); // highest stake first

            for (id, _) in standby.into_iter().take(deficit) {
                if let Some(v) = self.validators.get_mut(&id) {
                    v.status = ValidatorStatus::Active;
                }
            }
        }
    }

    fn active_count(&self) -> usize {
        self.validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Active)
            .count()
    }

    fn jailed_count(&self) -> usize {
        self.validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Jailed)
            .count()
    }

    fn standby_count(&self) -> usize {
        self.validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Standby)
            .count()
    }

    fn total_active_stake(&self) -> u64 {
        self.validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Active)
            .map(|v| v.stake)
            .sum()
    }
}

#[test]
fn stress_validator_churn() {
    println!("\n=== TRv1 Validator Churn Stress Test ===\n");

    let mut manager = ValidatorSetManager::new(100);
    let n_epochs = 200;
    let start = Instant::now();

    // Bootstrap: 100 active validators
    for i in 0..100 {
        manager.add_validator((100 - i) * 1_000_000);
    }

    println!(
        "Initial: active={}, standby={}, jailed={}",
        manager.active_count(),
        manager.standby_count(),
        manager.jailed_count()
    );

    let mut max_jailed = 0usize;
    let mut min_active = usize::MAX;

    for epoch in 1..=n_epochs {
        // Simulate: some validators miss proposals
        let active_ids: Vec<u64> = manager
            .validators
            .iter()
            .filter(|(_, v)| v.status == ValidatorStatus::Active)
            .map(|(id, _)| *id)
            .collect();

        // 10% of active validators miss proposals each epoch
        let n_miss = active_ids.len() / 10;
        for &id in active_ids.iter().take(n_miss) {
            manager.record_missed_proposal(id, epoch as u64);
        }

        // Every 10 epochs, some validators exit and new ones join
        if epoch % 10 == 0 {
            // Remove 5 validators
            let exit_ids: Vec<u64> = manager
                .validators
                .iter()
                .filter(|(_, v)| v.status == ValidatorStatus::Active)
                .take(5)
                .map(|(id, _)| *id)
                .collect();
            for id in exit_ids {
                manager.remove_validator(id);
            }

            // Add 7 new validators
            for _ in 0..7 {
                manager.add_validator(500_000 + (epoch as u64 * 1_000));
            }
        }

        // Directly jail a validator every 5 epochs (slash event)
        if epoch % 5 == 0 {
            if let Some(&id) = active_ids.first() {
                manager.jail_validator(id, epoch as u64);
            }
        }

        manager.process_epoch(epoch as u64);

        let jailed = manager.jailed_count();
        let active = manager.active_count();
        max_jailed = max_jailed.max(jailed);
        min_active = min_active.min(active);

        if epoch % 50 == 0 {
            println!(
                "Epoch {epoch}: active={active}, standby={}, jailed={jailed}, total_stake={}",
                manager.standby_count(),
                manager.total_active_stake()
            );
        }
    }

    let elapsed = start.elapsed();

    println!("\n--- Results ---");
    println!("Epochs simulated: {n_epochs}");
    println!(
        "Final: active={}, standby={}, jailed={}",
        manager.active_count(),
        manager.standby_count(),
        manager.jailed_count()
    );
    println!("Max jailed at once: {max_jailed}");
    println!("Min active at once: {min_active}");
    println!("Total elapsed: {elapsed:?}");

    // The network should maintain at least 2/3 of max_active validators
    let min_for_safety = manager.max_active * 2 / 3;
    assert!(
        min_active >= min_for_safety,
        "active validators dropped below safety threshold: min={min_active}, threshold={min_for_safety}"
    );
}

#[test]
fn stress_rapid_join_leave() {
    println!("\n=== TRv1 Rapid Join/Leave Stress Test ===\n");

    let mut manager = ValidatorSetManager::new(200);
    let start = Instant::now();

    // Add and remove 10,000 validators rapidly
    let mut ids = Vec::new();
    for i in 0..10_000 {
        let id = manager.add_validator(1_000_000);
        ids.push(id);

        // Remove every other one
        if i % 2 == 1 {
            manager.remove_validator(ids[i - 1]);
        }

        // Process epoch every 100 validators
        if i % 100 == 99 {
            manager.process_epoch((i / 100) as u64);
        }
    }

    let elapsed = start.elapsed();

    println!("Processed 10,000 join/leave operations in {elapsed:?}");
    println!(
        "Final: active={}, standby={}, total={}",
        manager.active_count(),
        manager.standby_count(),
        manager.validators.len()
    );

    assert!(
        elapsed.as_secs() < 10,
        "validator churn should complete within 10 seconds"
    );
}

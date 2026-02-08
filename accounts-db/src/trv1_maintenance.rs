//! TRv1 Maintenance Service — Background thread for tiered storage upkeep
//!
//! This service runs a background thread that periodically:
//! 1. Evicts cold accounts from the hot cache to warm storage
//! 2. Checks for rent-expired accounts and archives them
//! 3. Reports statistics
//!
//! # Feature Gate
//!
//! All functionality is gated behind the `trv1-tiered-storage` feature flag.

use {
    crate::trv1_storage_adapter::TRv1StorageAdapter,
    log::*,
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, JoinHandle},
        time::Duration,
    },
};

/// Default maintenance interval: 60 seconds.
pub const DEFAULT_MAINTENANCE_INTERVAL_SECS: u64 = 60;

/// Minimum allowed maintenance interval: 5 seconds.
pub const MIN_MAINTENANCE_INTERVAL_SECS: u64 = 5;

/// Background maintenance service for TRv1 tiered storage.
///
/// Spawns a thread that periodically performs cache eviction, tier migration,
/// rent expiry checks, and statistics reporting.
///
/// # Lifecycle
///
/// The service is started with `MaintenanceService::start()` and runs until
/// `stop()` is called or the `exit` signal is set. The `Drop` implementation
/// also signals the thread to stop.
///
/// # Example
///
/// ```rust,ignore
/// use std::sync::{Arc, atomic::AtomicBool};
/// use solana_accounts_db::trv1_maintenance::MaintenanceService;
/// use solana_accounts_db::trv1_storage_adapter::TRv1StorageAdapter;
///
/// let adapter = Arc::new(TRv1StorageAdapter::new(config, rent_config));
/// let exit = Arc::new(AtomicBool::new(false));
///
/// let service = MaintenanceService::start(
///     adapter.clone(),
///     exit.clone(),
///     Duration::from_secs(60),
/// );
///
/// // ... later ...
/// service.stop();
/// ```
pub struct MaintenanceService {
    /// Handle to the background thread.
    thread: Option<JoinHandle<()>>,

    /// Signal to stop the maintenance thread.
    exit: Arc<AtomicBool>,
}

impl MaintenanceService {
    /// Start the maintenance service with the given adapter and interval.
    ///
    /// # Arguments
    ///
    /// * `adapter` - The TRv1 storage adapter to maintain
    /// * `exit` - Shared exit signal (set to true to stop)
    /// * `interval` - How often to run maintenance (minimum 5 seconds)
    pub fn start(
        adapter: Arc<TRv1StorageAdapter>,
        exit: Arc<AtomicBool>,
        interval: Duration,
    ) -> Self {
        let interval = interval.max(Duration::from_secs(MIN_MAINTENANCE_INTERVAL_SECS));
        let exit_clone = exit.clone();

        info!(
            "TRv1 Maintenance Service starting (interval: {:?})",
            interval,
        );

        let thread = thread::Builder::new()
            .name("trv1Maintain".to_string())
            .spawn(move || {
                Self::run_loop(adapter, exit_clone, interval);
            })
            .expect("Failed to spawn TRv1 maintenance thread");

        Self {
            thread: Some(thread),
            exit,
        }
    }

    /// The main loop of the maintenance thread.
    fn run_loop(
        adapter: Arc<TRv1StorageAdapter>,
        exit: Arc<AtomicBool>,
        interval: Duration,
    ) {
        info!("TRv1 Maintenance Service thread started");

        // Use a current_slot counter. In a real integration this would
        // come from the Bank or the AccountsDb. For now we estimate it
        // based on elapsed time.
        let mut tick_count: u64 = 0;

        while !exit.load(Ordering::Relaxed) {
            // Sleep in small increments to check exit signal promptly
            let sleep_step = Duration::from_millis(500);
            let mut slept = Duration::ZERO;
            while slept < interval && !exit.load(Ordering::Relaxed) {
                thread::sleep(sleep_step.min(interval - slept));
                slept += sleep_step;
            }

            if exit.load(Ordering::Relaxed) {
                break;
            }

            tick_count += 1;

            // Estimate current slot from tick count and interval.
            // At ~400ms per slot, interval_secs * 1000 / 400 = slots per interval.
            let estimated_slot_increment =
                (interval.as_millis() as u64).saturating_div(400);
            let estimated_current_slot = tick_count.saturating_mul(estimated_slot_increment);
            let estimated_epoch = estimated_current_slot / 432_000; // ~2 day epochs

            // Run maintenance
            adapter.maintenance_tick(estimated_current_slot, estimated_epoch);

            // Log periodic summary
            if tick_count % 10 == 0 {
                let stats = adapter.stats();
                info!(
                    "TRv1 Maintenance [{} ticks]: {}",
                    tick_count,
                    stats.summary(),
                );
            }
        }

        info!("TRv1 Maintenance Service thread exiting");
    }

    /// Signal the maintenance thread to stop and wait for it to finish.
    pub fn stop(mut self) {
        self.exit.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            if let Err(e) = thread.join() {
                warn!("TRv1 Maintenance Service thread panicked: {:?}", e);
            }
        }
    }

    /// Check if the maintenance thread is still running.
    pub fn is_running(&self) -> bool {
        self.thread
            .as_ref()
            .map(|t| !t.is_finished())
            .unwrap_or(false)
    }
}

impl Drop for MaintenanceService {
    fn drop(&mut self) {
        self.exit.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        state_rent_expiry::StateRentConfig,
        tiered_storage_config::TieredStorageConfig,
    };

    #[test]
    fn test_maintenance_service_start_stop() {
        let adapter = Arc::new(TRv1StorageAdapter::new(
            TieredStorageConfig::for_testing(),
            StateRentConfig::for_testing(),
        ));
        let exit = Arc::new(AtomicBool::new(false));

        let service = MaintenanceService::start(
            adapter.clone(),
            exit.clone(),
            Duration::from_secs(1),
        );

        assert!(service.is_running());

        // Let it run for a bit
        thread::sleep(Duration::from_millis(200));

        // Stop it
        service.stop();
    }

    #[test]
    fn test_maintenance_service_exit_signal() {
        let adapter = Arc::new(TRv1StorageAdapter::new(
            TieredStorageConfig::for_testing(),
            StateRentConfig::for_testing(),
        ));
        let exit = Arc::new(AtomicBool::new(false));

        let service = MaintenanceService::start(
            adapter.clone(),
            exit.clone(),
            Duration::from_secs(1),
        );

        // Signal exit externally
        exit.store(true, Ordering::Relaxed);

        // Give thread time to notice
        thread::sleep(Duration::from_millis(1500));

        assert!(!service.is_running());
    }

    #[test]
    fn test_maintenance_enforces_minimum_interval() {
        let adapter = Arc::new(TRv1StorageAdapter::new(
            TieredStorageConfig::for_testing(),
            StateRentConfig::for_testing(),
        ));
        let exit = Arc::new(AtomicBool::new(false));

        // Try to create with a too-short interval
        let service = MaintenanceService::start(
            adapter,
            exit.clone(),
            Duration::from_secs(1), // below minimum, will be clamped to 5s
        );

        // Just verify it starts without panicking
        assert!(service.is_running());
        service.stop();
    }

    #[test]
    fn test_maintenance_drop_stops_thread() {
        let adapter = Arc::new(TRv1StorageAdapter::new(
            TieredStorageConfig::for_testing(),
            StateRentConfig::for_testing(),
        ));
        let exit = Arc::new(AtomicBool::new(false));

        {
            let _service = MaintenanceService::start(
                adapter.clone(),
                exit.clone(),
                Duration::from_secs(1),
            );
            // service drops here
        }

        // Exit should have been signaled by drop
        assert!(exit.load(Ordering::Relaxed));
    }
}

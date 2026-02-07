//! Timeout management for the BFT consensus protocol.
//!
//! Each step (Propose, Prevote, Precommit) has a configurable timeout.
//! The Propose timeout increases linearly with round number to accommodate
//! slower proposers and network delays.

use {
    crate::config::BftConfig,
    crate::types::ConsensusStep,
    std::time::{Duration, Instant},
};

/// Tracks active timeouts for each consensus step.
#[derive(Debug)]
pub struct TimeoutScheduler {
    config: BftConfig,
    /// When the current timeout started (if any).
    started_at: Option<Instant>,
    /// Which step the timeout is for.
    active_step: Option<ConsensusStep>,
    /// The current round (affects propose timeout).
    current_round: u32,
}

impl TimeoutScheduler {
    /// Create a new timeout scheduler with the given configuration.
    pub fn new(config: BftConfig) -> Self {
        Self {
            config,
            started_at: None,
            active_step: None,
            current_round: 0,
        }
    }

    /// Start a timeout for the given step and round.
    pub fn start(&mut self, step: ConsensusStep, round: u32) {
        self.started_at = Some(Instant::now());
        self.active_step = Some(step);
        self.current_round = round;
    }

    /// Cancel the current timeout.
    pub fn cancel(&mut self) {
        self.started_at = None;
        self.active_step = None;
    }

    /// Returns the duration for the timeout of the given step at the given round.
    pub fn timeout_duration(&self, step: ConsensusStep, round: u32) -> Duration {
        let ms = match step {
            ConsensusStep::Propose | ConsensusStep::NewRound => {
                self.config.propose_timeout_ms(round)
            }
            ConsensusStep::Prevote => self.config.prevote_timeout_ms,
            ConsensusStep::Precommit => self.config.precommit_timeout_ms,
            ConsensusStep::Commit => 0, // Commit doesn't timeout
        };
        Duration::from_millis(ms)
    }

    /// Check if the current timeout has expired.
    /// Returns Some(step) if expired, None if still active or no timeout set.
    pub fn check_expired(&self) -> Option<ConsensusStep> {
        let started_at = self.started_at?;
        let step = self.active_step?;
        let duration = self.timeout_duration(step, self.current_round);
        if started_at.elapsed() >= duration {
            Some(step)
        } else {
            None
        }
    }

    /// Returns how much time remains before the current timeout expires.
    /// Returns None if no timeout is active, or Duration::ZERO if already expired.
    pub fn remaining(&self) -> Option<Duration> {
        let started_at = self.started_at?;
        let step = self.active_step?;
        let duration = self.timeout_duration(step, self.current_round);
        let elapsed = started_at.elapsed();
        Some(duration.saturating_sub(elapsed))
    }

    /// Returns the currently active step, if any.
    pub fn active_step(&self) -> Option<ConsensusStep> {
        self.active_step
    }

    /// Returns the current round.
    pub fn current_round(&self) -> u32 {
        self.current_round
    }

    /// Update the configuration (e.g., for dynamic parameter tuning).
    pub fn update_config(&mut self, config: BftConfig) {
        self.config = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_timeout_duration_propose_increases() {
        let config = BftConfig::default();
        let scheduler = TimeoutScheduler::new(config);
        let d0 = scheduler.timeout_duration(ConsensusStep::Propose, 0);
        let d1 = scheduler.timeout_duration(ConsensusStep::Propose, 1);
        let d2 = scheduler.timeout_duration(ConsensusStep::Propose, 2);
        assert!(d1 > d0);
        assert!(d2 > d1);
    }

    #[test]
    fn test_timeout_duration_prevote_constant() {
        let config = BftConfig::default();
        let scheduler = TimeoutScheduler::new(config);
        let d0 = scheduler.timeout_duration(ConsensusStep::Prevote, 0);
        let d1 = scheduler.timeout_duration(ConsensusStep::Prevote, 5);
        assert_eq!(d0, d1);
        assert_eq!(d0, Duration::from_millis(1000));
    }

    #[test]
    fn test_no_active_timeout() {
        let config = BftConfig::default();
        let scheduler = TimeoutScheduler::new(config);
        assert!(scheduler.check_expired().is_none());
        assert!(scheduler.remaining().is_none());
        assert!(scheduler.active_step().is_none());
    }

    #[test]
    fn test_start_and_cancel() {
        let config = BftConfig::default();
        let mut scheduler = TimeoutScheduler::new(config);
        scheduler.start(ConsensusStep::Prevote, 0);
        assert_eq!(scheduler.active_step(), Some(ConsensusStep::Prevote));
        scheduler.cancel();
        assert!(scheduler.active_step().is_none());
        assert!(scheduler.check_expired().is_none());
    }

    #[test]
    fn test_timeout_expires() {
        let mut config = BftConfig::default();
        config.prevote_timeout_ms = 10; // 10ms for testing
        let mut scheduler = TimeoutScheduler::new(config);
        scheduler.start(ConsensusStep::Prevote, 0);

        // Should not be expired immediately
        // (May be flaky on very slow systems, but 10ms is generous)
        assert!(scheduler.check_expired().is_none());

        // Wait for it to expire
        sleep(Duration::from_millis(20));
        assert_eq!(scheduler.check_expired(), Some(ConsensusStep::Prevote));
    }

    #[test]
    fn test_remaining_decreases() {
        let config = BftConfig::default();
        let mut scheduler = TimeoutScheduler::new(config);
        scheduler.start(ConsensusStep::Precommit, 0);
        let r1 = scheduler.remaining().unwrap();
        sleep(Duration::from_millis(10));
        let r2 = scheduler.remaining().unwrap();
        assert!(r2 < r1);
    }

    #[test]
    fn test_commit_has_zero_timeout() {
        let config = BftConfig::default();
        let scheduler = TimeoutScheduler::new(config);
        assert_eq!(
            scheduler.timeout_duration(ConsensusStep::Commit, 0),
            Duration::ZERO
        );
    }
}

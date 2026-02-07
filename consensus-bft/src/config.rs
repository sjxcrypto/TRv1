//! BFT consensus configuration.
//!
//! Defines timing parameters, quorum thresholds, and round limits
//! for the Tendermint-style BFT consensus protocol.

/// Configuration for the BFT consensus engine.
///
/// All timeouts are in milliseconds. The protocol targets 1-second block time
/// with ~6-second deterministic finality under normal operation.
#[derive(Debug, Clone)]
pub struct BftConfig {
    /// Target block production interval in milliseconds.
    /// Default: 1000 (1 second).
    pub block_time_ms: u64,

    /// Timeout for the prevote phase in milliseconds.
    /// If a validator doesn't receive enough prevotes within this window,
    /// it transitions to precommit with a nil vote.
    /// Default: 1000.
    pub prevote_timeout_ms: u64,

    /// Timeout for the precommit phase in milliseconds.
    /// If a validator doesn't receive enough precommits within this window,
    /// a new round begins.
    /// Default: 1000.
    pub precommit_timeout_ms: u64,

    /// Fraction of total stake required for quorum (2/3 + 1).
    /// Default: 0.667.
    pub finality_threshold: f64,

    /// Maximum number of rounds before the engine gives up on a height.
    /// Default: 5.
    pub max_rounds_per_height: u32,

    /// Base timeout for the propose phase in milliseconds.
    /// The actual timeout is: base + delta * round.
    /// Default: 3000.
    pub propose_timeout_base_ms: u64,

    /// Additional timeout per round for the propose phase.
    /// Default: 500.
    pub propose_timeout_delta_ms: u64,
}

impl Default for BftConfig {
    fn default() -> Self {
        Self {
            block_time_ms: 1000,
            prevote_timeout_ms: 1000,
            precommit_timeout_ms: 1000,
            finality_threshold: 0.667,
            max_rounds_per_height: 5,
            propose_timeout_base_ms: 3000,
            propose_timeout_delta_ms: 500,
        }
    }
}

impl BftConfig {
    /// Compute the propose timeout for a given round.
    /// Increases linearly with the round number to give slower proposers more time.
    pub fn propose_timeout_ms(&self, round: u32) -> u64 {
        self.propose_timeout_base_ms + self.propose_timeout_delta_ms * round as u64
    }

    /// Validate configuration parameters.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.block_time_ms == 0 {
            return Err(ConfigError::InvalidBlockTime);
        }
        if self.finality_threshold < 0.5 || self.finality_threshold > 1.0 {
            return Err(ConfigError::InvalidFinalityThreshold(
                self.finality_threshold,
            ));
        }
        if self.max_rounds_per_height == 0 {
            return Err(ConfigError::InvalidMaxRounds);
        }
        Ok(())
    }
}

/// Errors in BFT configuration.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    #[error("block_time_ms must be > 0")]
    InvalidBlockTime,
    #[error("finality_threshold must be in [0.5, 1.0], got {0}")]
    InvalidFinalityThreshold(f64),
    #[error("max_rounds_per_height must be > 0")]
    InvalidMaxRounds,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BftConfig::default();
        assert_eq!(config.block_time_ms, 1000);
        assert_eq!(config.prevote_timeout_ms, 1000);
        assert_eq!(config.precommit_timeout_ms, 1000);
        assert!((config.finality_threshold - 0.667).abs() < 1e-6);
        assert_eq!(config.max_rounds_per_height, 5);
        assert_eq!(config.propose_timeout_base_ms, 3000);
        assert_eq!(config.propose_timeout_delta_ms, 500);
    }

    #[test]
    fn test_propose_timeout_increases_with_round() {
        let config = BftConfig::default();
        assert_eq!(config.propose_timeout_ms(0), 3000);
        assert_eq!(config.propose_timeout_ms(1), 3500);
        assert_eq!(config.propose_timeout_ms(2), 4000);
        assert_eq!(config.propose_timeout_ms(4), 5000);
    }

    #[test]
    fn test_valid_config() {
        let config = BftConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_block_time() {
        let mut config = BftConfig::default();
        config.block_time_ms = 0;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidBlockTime)
        ));
    }

    #[test]
    fn test_invalid_finality_threshold_too_low() {
        let mut config = BftConfig::default();
        config.finality_threshold = 0.3;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidFinalityThreshold(_))
        ));
    }

    #[test]
    fn test_invalid_finality_threshold_too_high() {
        let mut config = BftConfig::default();
        config.finality_threshold = 1.1;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidFinalityThreshold(_))
        ));
    }

    #[test]
    fn test_invalid_max_rounds() {
        let mut config = BftConfig::default();
        config.max_rounds_per_height = 0;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidMaxRounds)
        ));
    }
}

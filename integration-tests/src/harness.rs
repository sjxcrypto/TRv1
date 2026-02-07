//! TRv1 Test Harness
//!
//! Provides a lightweight test environment for integration-testing TRv1's
//! custom subsystems:
//!
//! - Passive staking (tiered locks, rewards, governance weights)
//! - Developer rewards (revenue sharing, anti-gaming)
//! - Treasury (multisig → governance transition)
//! - Slashing & jailing
//! - Active validator set management (200 cap)
//! - Fee distribution (4-way split with epoch transition)
//! - Inflation model (flat 5% on staked supply)
//!
//! The harness does NOT spin up a full `Bank`; instead it provides
//! deterministic helpers that test the program logic directly using the
//! crate APIs of each subsystem.

use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;

// ─── Constants ───────────────────────────────────────────────────────────────

/// Default staker balance used in tests (100 SOL in lamports).
pub const DEFAULT_STAKE_LAMPORTS: u64 = 100_000_000_000;

/// One SOL in lamports.
pub const SOL: u64 = 1_000_000_000;

/// Default number of validators to create in a test cluster.
pub const DEFAULT_VALIDATOR_COUNT: usize = 10;

/// Maximum active validators in the TRv1 network.
pub const MAX_ACTIVE_VALIDATORS: usize = 200;

/// Epochs per year (one epoch ≈ one day).
pub const EPOCHS_PER_YEAR: u64 = 365;

/// Flat annual staking rate (5%).
pub const STAKING_RATE: f64 = 0.05;

// ─── Test validator ──────────────────────────────────────────────────────────

/// A test validator with associated keypairs and state.
#[derive(Debug)]
pub struct TestValidator {
    /// Node identity keypair.
    pub keypair: Keypair,
    /// Vote account keypair.
    pub vote_keypair: Keypair,
    /// Amount of own stake (lamports).
    pub stake_amount: u64,
    /// Whether this validator is in the active set.
    pub is_active: bool,
}

impl TestValidator {
    /// Create a new test validator with the given stake.
    pub fn new(stake_amount: u64) -> Self {
        Self {
            keypair: Keypair::new(),
            vote_keypair: Keypair::new(),
            stake_amount,
            is_active: true,
        }
    }

    /// Create a new validator marked as standby.
    pub fn new_standby(stake_amount: u64) -> Self {
        Self {
            keypair: Keypair::new(),
            vote_keypair: Keypair::new(),
            stake_amount,
            is_active: false,
        }
    }

    /// Return the node identity pubkey.
    pub fn pubkey(&self) -> Pubkey {
        self.keypair.pubkey()
    }

    /// Return the vote account pubkey.
    pub fn vote_pubkey(&self) -> Pubkey {
        self.vote_keypair.pubkey()
    }
}

// ─── Test harness ────────────────────────────────────────────────────────────

/// Top-level test harness that sets up a minimal TRv1 environment.
///
/// This harness provides:
/// - A set of test validators with configurable stake amounts
/// - A treasury pubkey
/// - A mint keypair for funding
///
/// It does NOT construct a full `Bank` — the subsystem tests exercise
/// the program logic directly against the individual crate APIs.
pub struct TRv1TestHarness {
    /// Test validators participating in the network.
    pub validators: Vec<TestValidator>,
    /// Treasury pubkey.
    pub treasury_pubkey: Pubkey,
    /// Mint / faucet keypair used to fund accounts in tests.
    pub mint_keypair: Keypair,
    /// Authority keypair for the treasury.
    pub treasury_authority: Keypair,
    /// Developer rewards program authority (for test registration).
    pub developer_authority: Keypair,
    /// Current simulated epoch.
    pub current_epoch: u64,
    /// Current simulated slot.
    pub current_slot: u64,
    /// Current simulated unix timestamp.
    pub current_unix_timestamp: i64,
}

impl Default for TRv1TestHarness {
    fn default() -> Self {
        Self::new(DEFAULT_VALIDATOR_COUNT)
    }
}

impl TRv1TestHarness {
    /// Create a new harness with `n` validators, each with `DEFAULT_STAKE_LAMPORTS`.
    pub fn new(num_validators: usize) -> Self {
        let validators: Vec<TestValidator> = (0..num_validators)
            .map(|_| TestValidator::new(DEFAULT_STAKE_LAMPORTS))
            .collect();

        Self {
            validators,
            treasury_pubkey: Pubkey::new_unique(),
            mint_keypair: Keypair::new(),
            treasury_authority: Keypair::new(),
            developer_authority: Keypair::new(),
            current_epoch: 0,
            current_slot: 0,
            current_unix_timestamp: 1_700_000_000, // ~Nov 2023
        }
    }

    /// Create a harness with custom validator stakes.
    pub fn with_stakes(stakes: &[u64]) -> Self {
        let validators: Vec<TestValidator> = stakes
            .iter()
            .map(|&s| TestValidator::new(s))
            .collect();

        Self {
            validators,
            treasury_pubkey: Pubkey::new_unique(),
            mint_keypair: Keypair::new(),
            treasury_authority: Keypair::new(),
            developer_authority: Keypair::new(),
            current_epoch: 0,
            current_slot: 0,
            current_unix_timestamp: 1_700_000_000,
        }
    }

    /// Create a harness with `n` active + `m` standby validators.
    pub fn with_active_and_standby(active_count: usize, standby_count: usize) -> Self {
        let mut validators: Vec<TestValidator> = (0..active_count)
            .map(|i| {
                // Active validators get decreasing stake so they're clearly ranked.
                TestValidator::new(DEFAULT_STAKE_LAMPORTS * (active_count - i) as u64)
            })
            .collect();

        for i in 0..standby_count {
            validators.push(TestValidator::new_standby(SOL * (standby_count - i) as u64));
        }

        Self {
            validators,
            treasury_pubkey: Pubkey::new_unique(),
            mint_keypair: Keypair::new(),
            treasury_authority: Keypair::new(),
            developer_authority: Keypair::new(),
            current_epoch: 0,
            current_slot: 0,
            current_unix_timestamp: 1_700_000_000,
        }
    }

    /// Advance the simulated epoch by `n` epochs.
    pub fn advance_epochs(&mut self, n: u64) {
        self.current_epoch += n;
        // ~216_000 slots per epoch at ~400ms/slot (~24h)
        self.current_slot += n * 216_000;
        // ~86_400 seconds per epoch (1 day)
        self.current_unix_timestamp += (n as i64) * 86_400;
    }

    /// Advance the simulated time by `days` days.
    pub fn advance_days(&mut self, days: u64) {
        self.advance_epochs(days);
    }

    /// Advance the simulated time by `seconds` seconds, without advancing epochs.
    pub fn advance_seconds(&mut self, seconds: i64) {
        self.current_unix_timestamp += seconds;
        // Rough slot advancement: 1 slot = ~400ms = 0.4s
        self.current_slot += (seconds as u64) * 5 / 2;
    }

    /// Total staked supply across all validators.
    pub fn total_staked_supply(&self) -> u64 {
        self.validators.iter().map(|v| v.stake_amount).sum()
    }

    /// Total staked supply for active validators only.
    pub fn active_staked_supply(&self) -> u64 {
        self.validators
            .iter()
            .filter(|v| v.is_active)
            .map(|v| v.stake_amount)
            .sum()
    }

    /// Returns the count of active validators.
    pub fn active_count(&self) -> usize {
        self.validators.iter().filter(|v| v.is_active).count()
    }

    /// Returns the count of standby validators.
    pub fn standby_count(&self) -> usize {
        self.validators.iter().filter(|v| !v.is_active).count()
    }
}

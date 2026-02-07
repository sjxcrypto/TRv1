//! TRv1 Genesis Configuration
//!
//! Provides helpers for creating a TRv1-flavoured genesis configuration
//! that can be used by the test validator or the genesis binary.
//!
//! # Key differences from Solana defaults
//!
//! - **Inflation**: flat 5 % annual rate on staked supply (no declining curve).
//! - **Epoch schedule**: 86 400 slots per epoch (~1 day at 1 s slot times).
//! - **Fee market**: EIP-1559 dynamic base fee (starting at 5 000 lamports/CU).
//! - **Fee split**: 4-way (burn / validator / treasury / developer) with a
//!   5-year linear transition from launch to maturity ratios.
//! - **TRv1 programs**: passive-stake, treasury, governance, developer-rewards
//!   registered at well-known addresses.
//! - **Treasury**: funded at genesis with an initial allocation.
//! - **Validator cap**: 200 active validators.

use {
    solana_account::{AccountSharedData, WritableAccount},
    solana_clock::Slot,
    solana_epoch_schedule::EpochSchedule,
    solana_inflation::Inflation,
    solana_native_token::LAMPORTS_PER_SOL,
    solana_pubkey::Pubkey,
    solana_rent::Rent,
    std::str::FromStr,
};

// ── TRv1 Program IDs ──────────────────────────────────────────────────────────

/// Passive Stake program ID.
pub const PASSIVE_STAKE_PROGRAM_ID: &str = "Pass1veStake1111111111111111111111111111111";

/// Treasury program ID.
pub const TREASURY_PROGRAM_ID: &str = "Treasury11111111111111111111111111111111111";

/// Governance program ID.
pub const GOVERNANCE_PROGRAM_ID: &str = "Governance1111111111111111111111111111111111";

/// Developer Rewards program ID.
pub const DEVELOPER_REWARDS_PROGRAM_ID: &str = "DevRew11111111111111111111111111111111111111";

// ── Economic Constants ─────────────────────────────────────────────────────────

/// Flat 5 % annual inflation applied only to staked supply.
pub const STAKING_RATE: f64 = 0.05;

/// Slots per epoch: 86 400 (24 h at 1 s slots).
pub const SLOTS_PER_EPOCH: Slot = 86_400;

/// Initial treasury balance: 10 M TRV1 tokens.
pub const INITIAL_TREASURY_LAMPORTS: u64 = 10_000_000 * LAMPORTS_PER_SOL;

/// Stake per test validator: 500 k TRV1.
pub const TEST_VALIDATOR_STAKE_LAMPORTS: u64 = 500_000 * LAMPORTS_PER_SOL;

/// Initial faucet / mint balance for the test network: 500 M TRV1.
pub const TEST_MINT_LAMPORTS: u64 = 500_000_000 * LAMPORTS_PER_SOL;

// ── Fee Market Defaults ────────────────────────────────────────────────────────

/// Starting base fee per compute unit (lamports) — genesis block.
pub const INITIAL_BASE_FEE_PER_CU: u64 = 5_000;

/// Minimum base fee floor (lamports / CU).
pub const MIN_BASE_FEE: u64 = 5_000;

/// Maximum base fee ceiling (lamports / CU).
pub const MAX_BASE_FEE: u64 = 50_000_000;

/// Target block utilization (percentage).
pub const TARGET_UTILIZATION_PCT: u8 = 50;

/// Maximum compute units per block.
pub const MAX_BLOCK_COMPUTE_UNITS: u64 = 48_000_000;

/// Base fee change denominator (±12.5 % per block).
pub const BASE_FEE_CHANGE_DENOMINATOR: u64 = 8;

// ── Fee Distribution (Launch) ──────────────────────────────────────────────────

/// Fee distribution at epoch 0: (burn, validator, treasury, developer).
pub const LAUNCH_FEE_SPLIT: (f64, f64, f64, f64) = (0.10, 0.00, 0.45, 0.45);

/// Fee distribution at maturity (epoch 1825+).
pub const MATURE_FEE_SPLIT: (f64, f64, f64, f64) = (0.25, 0.25, 0.25, 0.25);

/// Number of epochs for the fee transition (≈ 5 years of daily epochs).
pub const FEE_TRANSITION_EPOCHS: u64 = 1825;

// ── Genesis Builder ────────────────────────────────────────────────────────────

/// A description of an account to be injected at genesis.
#[derive(Debug, Clone)]
pub struct GenesisAccount {
    pub pubkey: Pubkey,
    pub lamports: u64,
    pub owner: Pubkey,
    pub executable: bool,
    pub data: Vec<u8>,
}

/// Collects all TRv1-specific genesis parameters.
#[derive(Debug, Clone)]
pub struct Trv1GenesisConfig {
    /// Slots per epoch.
    pub slots_per_epoch: Slot,
    /// Inflation model — we use `Inflation::new_fixed(STAKING_RATE)`.
    pub inflation: Inflation,
    /// Epoch schedule.
    pub epoch_schedule: EpochSchedule,
    /// Rent configuration.
    pub rent: Rent,
    /// Extra accounts to inject (treasury, program stubs, etc.).
    pub extra_accounts: Vec<GenesisAccount>,
}

impl Default for Trv1GenesisConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl Trv1GenesisConfig {
    /// Create a new TRv1 genesis config with all defaults.
    pub fn new() -> Self {
        let slots_per_epoch = SLOTS_PER_EPOCH;
        let epoch_schedule = EpochSchedule::custom(
            slots_per_epoch,
            slots_per_epoch,
            /* enable_warmup_epochs = */ false,
        );

        // Flat 5 % inflation — the Inflation struct's `new_fixed` sets a
        // constant rate with no taper and no foundation allocation.
        let inflation = Inflation::new_fixed(STAKING_RATE);

        let rent = Rent::with_slots_per_epoch(slots_per_epoch);

        let extra_accounts = Self::build_initial_accounts();

        Self {
            slots_per_epoch,
            inflation,
            epoch_schedule,
            rent,
            extra_accounts,
        }
    }

    /// Build the list of accounts that should be injected at genesis.
    fn build_initial_accounts() -> Vec<GenesisAccount> {
        let mut accounts = Vec::new();

        // ── Treasury Account ──────────────────────────────────────────────
        //
        // A simple system-owned account holding the initial treasury funds.
        // The Treasury *program* (a builtin) will manage this account, but
        // at genesis we just seed it with lamports.
        let treasury_pubkey = Pubkey::from_str(TREASURY_PROGRAM_ID)
            .expect("invalid treasury program ID");
        accounts.push(GenesisAccount {
            pubkey: treasury_pubkey,
            lamports: INITIAL_TREASURY_LAMPORTS,
            owner: solana_pubkey::Pubkey::default(), // system program
            executable: false,
            data: Vec::new(),
        });

        // ── Program Marker Accounts ───────────────────────────────────────
        //
        // For the test validator we register TRv1 programs as builtins in the
        // runtime.  Here we create zero-balance marker accounts so that
        // on-chain program ID lookups work.
        for program_id_str in &[
            PASSIVE_STAKE_PROGRAM_ID,
            GOVERNANCE_PROGRAM_ID,
            DEVELOPER_REWARDS_PROGRAM_ID,
        ] {
            let pubkey = Pubkey::from_str(program_id_str)
                .unwrap_or_else(|_| panic!("invalid program ID: {program_id_str}"));
            accounts.push(GenesisAccount {
                pubkey,
                lamports: 1, // minimum non-zero balance for existence
                owner: Pubkey::from_str("BPFLoaderUpgradeab1e11111111111111111111111")
                    .unwrap_or_default(),
                executable: true,
                data: Vec::new(),
            });
        }

        accounts
    }

    /// Returns `(pubkey, AccountSharedData)` pairs suitable for adding to a
    /// `TestValidatorGenesis` or `GenesisConfig`.
    pub fn account_pairs(&self) -> Vec<(Pubkey, AccountSharedData)> {
        self.extra_accounts
            .iter()
            .map(|a| {
                let mut account = AccountSharedData::new(a.lamports, a.data.len(), &a.owner);
                account.set_executable(a.executable);
                if !a.data.is_empty() {
                    account.set_data_from_slice(&a.data);
                }
                (a.pubkey, account)
            })
            .collect()
    }

    /// Convenience: list of all TRv1 program IDs.
    pub fn program_ids() -> Vec<Pubkey> {
        [
            PASSIVE_STAKE_PROGRAM_ID,
            TREASURY_PROGRAM_ID,
            GOVERNANCE_PROGRAM_ID,
            DEVELOPER_REWARDS_PROGRAM_ID,
        ]
        .iter()
        .map(|s| Pubkey::from_str(s).expect("invalid program ID"))
        .collect()
    }

    /// Human-readable summary for logging at startup.
    pub fn summary(&self) -> String {
        format!(
            "TRv1 Genesis Configuration:\n\
             ├─ Slots per epoch:      {}\n\
             ├─ Inflation:            {:.1}% annual (flat)\n\
             ├─ Initial base fee:     {} lamports/CU\n\
             ├─ Fee split (launch):   burn={:.0}% val={:.0}% treasury={:.0}% dev={:.0}%\n\
             ├─ Fee transition:       {} epochs (~5 years)\n\
             ├─ Treasury initial:     {} TRV1\n\
             ├─ Validator stake cap:  200\n\
             └─ Programs:             passive-stake, treasury, governance, dev-rewards",
            self.slots_per_epoch,
            STAKING_RATE * 100.0,
            INITIAL_BASE_FEE_PER_CU,
            LAUNCH_FEE_SPLIT.0 * 100.0,
            LAUNCH_FEE_SPLIT.1 * 100.0,
            LAUNCH_FEE_SPLIT.2 * 100.0,
            LAUNCH_FEE_SPLIT.3 * 100.0,
            FEE_TRANSITION_EPOCHS,
            INITIAL_TREASURY_LAMPORTS / LAMPORTS_PER_SOL,
        )
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_creates_valid_epoch_schedule() {
        let cfg = Trv1GenesisConfig::new();
        assert_eq!(cfg.slots_per_epoch, 86_400);
        assert_eq!(cfg.epoch_schedule.slots_per_epoch, 86_400);
    }

    #[test]
    fn test_inflation_is_flat_five_percent() {
        let cfg = Trv1GenesisConfig::new();
        // At any year the inflation should be 5 %
        let rate_year_0 = cfg.inflation.total(0.0);
        let rate_year_5 = cfg.inflation.total(5.0);
        let rate_year_50 = cfg.inflation.total(50.0);
        // new_fixed sets initial = rate, terminal = rate, taper = 1.0
        assert!((rate_year_0 - STAKING_RATE).abs() < 1e-10);
        assert!((rate_year_5 - STAKING_RATE).abs() < 1e-10);
        assert!((rate_year_50 - STAKING_RATE).abs() < 1e-10);
    }

    #[test]
    fn test_extra_accounts_include_treasury() {
        let cfg = Trv1GenesisConfig::new();
        let treasury_pk = Pubkey::from_str(TREASURY_PROGRAM_ID).unwrap();
        let found = cfg.extra_accounts.iter().any(|a| a.pubkey == treasury_pk);
        assert!(found, "treasury account should be in genesis");
    }

    #[test]
    fn test_account_pairs_are_non_empty() {
        let cfg = Trv1GenesisConfig::new();
        let pairs = cfg.account_pairs();
        // At least treasury + 3 program markers
        assert!(pairs.len() >= 4);
    }

    #[test]
    fn test_program_ids() {
        let ids = Trv1GenesisConfig::program_ids();
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn test_summary_is_non_empty() {
        let cfg = Trv1GenesisConfig::new();
        let s = cfg.summary();
        assert!(s.contains("TRv1 Genesis Configuration"));
        assert!(s.contains("86400"));
        assert!(s.contains("5.0%"));
    }

    #[test]
    fn test_fee_split_sums_to_one() {
        let (b, v, t, d) = LAUNCH_FEE_SPLIT;
        assert!((b + v + t + d - 1.0).abs() < 1e-10, "launch split must sum to 1.0");

        let (b, v, t, d) = MATURE_FEE_SPLIT;
        assert!((b + v + t + d - 1.0).abs() < 1e-10, "mature split must sum to 1.0");
    }
}

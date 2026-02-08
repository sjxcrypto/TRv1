//! Shared test utilities for TRv1 end-to-end tests.
//!
//! Provides a simulated network environment that orchestrates:
//! - Multiple validators with stake-weighted consensus
//! - Block production via the BFT engine
//! - Fee market dynamics (EIP-1559)
//! - Epoch management and transitions
//! - Account state tracking
//! - Passive staking, governance, treasury, and developer rewards bookkeeping

use {
    rand::Rng,
    solana_hash::Hash,
    solana_pubkey::Pubkey,
    std::collections::HashMap,
    trv1_consensus_bft::{
        BftConfig, ConsensusEngine, ConsensusMessage, EngineOutput, ValidatorInfo, ValidatorSet,
    },
    trv1_fee_market::{
        calculator::{calculate_next_base_fee, calculate_transaction_fee},
        BlockFeeState, FeeMarketConfig,
    },
};

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Slots per epoch in the simulated network.
pub const SLOTS_PER_EPOCH: u64 = 32;

/// Maximum active validators in the network.
pub const MAX_ACTIVE_VALIDATORS: usize = 200;

/// Jailing threshold: consecutive missed slots before jailing.
pub const JAIL_THRESHOLD_MISSED_SLOTS: u64 = 100;

/// Slash rate for double-signing (basis points): 5% = 500 bps.
pub const DOUBLE_SIGN_SLASH_BPS: u64 = 500;

/// Basis-point denominator.
pub const BPS_DENOM: u64 = 10_000;

// ─────────────────────────────────────────────────────────────────────────────
// Fee distribution schedule (mirrors developer-rewards/src/constants.rs)
// ─────────────────────────────────────────────────────────────────────────────

/// Fee split at launch (basis points).
pub struct FeeSplit {
    pub burn_bps: u64,
    pub validator_bps: u64,
    pub treasury_bps: u64,
    pub developer_bps: u64,
}

/// Launch-era fee split.
pub const LAUNCH_FEE_SPLIT: FeeSplit = FeeSplit {
    burn_bps: 1_000,     // 10%
    validator_bps: 0,    //  0%
    treasury_bps: 4_500, // 45%
    developer_bps: 4_500, // 45%
};

/// Maturity-era fee split.
pub const MATURITY_FEE_SPLIT: FeeSplit = FeeSplit {
    burn_bps: 2_500,     // 25%
    validator_bps: 2_500, // 25%
    treasury_bps: 2_500,  // 25%
    developer_bps: 2_500, // 25%
};

/// Transition epochs from launch → maturity.
pub const TRANSITION_EPOCHS: u64 = 912;

/// Interpolate fee split for a given epoch.
pub fn fee_split_at_epoch(epoch: u64) -> FeeSplit {
    if epoch >= TRANSITION_EPOCHS {
        return FeeSplit {
            burn_bps: MATURITY_FEE_SPLIT.burn_bps,
            validator_bps: MATURITY_FEE_SPLIT.validator_bps,
            treasury_bps: MATURITY_FEE_SPLIT.treasury_bps,
            developer_bps: MATURITY_FEE_SPLIT.developer_bps,
        };
    }
    let lerp = |a: u64, b: u64| -> u64 {
        let diff = if b > a { b - a } else { 0 };
        let neg_diff = if a > b { a - b } else { 0 };
        if b >= a {
            a + diff * epoch / TRANSITION_EPOCHS
        } else {
            a - neg_diff * epoch / TRANSITION_EPOCHS
        }
    };
    FeeSplit {
        burn_bps: lerp(LAUNCH_FEE_SPLIT.burn_bps, MATURITY_FEE_SPLIT.burn_bps),
        validator_bps: lerp(LAUNCH_FEE_SPLIT.validator_bps, MATURITY_FEE_SPLIT.validator_bps),
        treasury_bps: lerp(LAUNCH_FEE_SPLIT.treasury_bps, MATURITY_FEE_SPLIT.treasury_bps),
        developer_bps: lerp(LAUNCH_FEE_SPLIT.developer_bps, MATURITY_FEE_SPLIT.developer_bps),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Simulated Validator
// ─────────────────────────────────────────────────────────────────────────────

/// Status of a validator in the simulated network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidatorStatus {
    Active,
    Jailed,
    Inactive,
}

/// A simulated validator node.
#[derive(Debug, Clone)]
pub struct SimValidator {
    pub pubkey: Pubkey,
    pub stake: u64,
    pub status: ValidatorStatus,
    /// Rewards earned (lamports).
    pub rewards_earned: u64,
    /// Consecutive slots missed.
    pub consecutive_missed: u64,
    /// Whether this validator is online.
    pub online: bool,
    /// Delegators: (delegator_pubkey → staked_amount).
    pub delegators: HashMap<Pubkey, u64>,
    /// Total delegation (sum of delegator stakes).
    pub total_delegation: u64,
    /// Has double-signed?
    pub double_signed: bool,
    /// Total slashed from own stake.
    pub total_slashed: u64,
}

impl SimValidator {
    pub fn new(pubkey: Pubkey, stake: u64) -> Self {
        Self {
            pubkey,
            stake,
            status: ValidatorStatus::Active,
            rewards_earned: 0,
            consecutive_missed: 0,
            online: true,
            delegators: HashMap::new(),
            total_delegation: 0,
            double_signed: false,
            total_slashed: 0,
        }
    }

    /// Total stake including delegations.
    pub fn total_stake(&self) -> u64 {
        self.stake.saturating_add(self.total_delegation)
    }

    /// Add a delegator.
    pub fn add_delegator(&mut self, delegator: Pubkey, amount: u64) {
        let entry = self.delegators.entry(delegator).or_insert(0);
        *entry = entry.saturating_add(amount);
        self.total_delegation = self.delegators.values().sum();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Simulated Passive Stake
// ─────────────────────────────────────────────────────────────────────────────

/// A passive stake position in the simulated network.
#[derive(Debug, Clone)]
pub struct SimPassiveStake {
    pub authority: Pubkey,
    pub amount: u64,
    pub lock_days: u64,
    pub lock_start_epoch: u64,
    pub lock_end_epoch: u64,
    pub unclaimed_rewards: u64,
    pub last_reward_epoch: u64,
    pub is_permanent: bool,
    pub vote_weight_bps: u16,
    pub active: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Simulated Transaction
// ─────────────────────────────────────────────────────────────────────────────

/// A simulated transaction for the test network.
#[derive(Debug, Clone)]
pub struct SimTransaction {
    pub sender: Pubkey,
    pub compute_units: u64,
    pub priority_fee_per_cu: u64,
    /// Optional: the program this transaction invokes (for developer fee attribution).
    pub invoked_program: Option<Pubkey>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Simulated Governance
// ─────────────────────────────────────────────────────────────────────────────

/// Status of a governance proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimProposalStatus {
    Active,
    Passed,
    Rejected,
    Vetoed,
    Timelocked,
    Executed,
    Cancelled,
    Expired,
}

/// A governance proposal in the simulated network.
#[derive(Debug, Clone)]
pub struct SimProposal {
    pub id: u64,
    pub proposer: Pubkey,
    pub title: String,
    pub status: SimProposalStatus,
    pub created_epoch: u64,
    pub voting_ends_epoch: u64,
    pub execution_epoch: u64,
    pub votes_for: u64,
    pub votes_against: u64,
    pub votes_abstain: u64,
    pub veto_votes: u64,
    pub is_emergency_unlock: bool,
    pub executed: bool,
}

/// Governance configuration.
#[derive(Debug, Clone)]
pub struct SimGovernanceConfig {
    pub is_active: bool,
    pub authority: Pubkey,
    pub proposal_threshold: u64,
    pub voting_period_epochs: u64,
    pub quorum_bps: u16,
    pub pass_threshold_bps: u16,
    pub veto_threshold_bps: u16,
    pub timelock_epochs: u64,
    pub emergency_multisig: Pubkey,
    pub next_proposal_id: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Simulated Treasury
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SimTreasury {
    pub authority: Pubkey,
    pub balance: u64,
    pub total_received: u64,
    pub total_disbursed: u64,
    pub governance_active: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// SimNetwork — the full simulated network
// ─────────────────────────────────────────────────────────────────────────────

/// A complete simulated TRv1 network.
pub struct SimNetwork {
    // ── Validators ───────────────────────────────────────────────────────
    pub validators: Vec<SimValidator>,

    // ── Epoch / Slot tracking ────────────────────────────────────────────
    pub current_slot: u64,
    pub current_epoch: u64,
    pub slots_per_epoch: u64,

    // ── Fee market ───────────────────────────────────────────────────────
    pub fee_config: FeeMarketConfig,
    pub fee_state: BlockFeeState,

    // ── Account balances (lamports) ──────────────────────────────────────
    pub balances: HashMap<Pubkey, u64>,

    // ── Fee distribution tracking ────────────────────────────────────────
    pub total_fees_collected: u64,
    pub total_burned: u64,
    pub treasury_fees: u64,
    pub validator_fees: u64,
    pub developer_fees: u64,

    // ── Passive staking ──────────────────────────────────────────────────
    pub passive_stakes: Vec<SimPassiveStake>,

    // ── Governance ───────────────────────────────────────────────────────
    pub governance: Option<SimGovernanceConfig>,
    pub proposals: Vec<SimProposal>,

    // ── Treasury ─────────────────────────────────────────────────────────
    pub treasury: Option<SimTreasury>,

    // ── Developer rewards tracking ───────────────────────────────────────
    /// program_id → accumulated developer fees.
    pub developer_reward_accounts: HashMap<Pubkey, u64>,

    // ── Consensus tracking ───────────────────────────────────────────────
    pub blocks_produced: u64,
    pub epoch_history: Vec<EpochSummary>,

    // ── Unix-time simulation ─────────────────────────────────────────────
    pub simulated_unix_time: i64,
}

/// Summary of a completed epoch.
#[derive(Debug, Clone)]
pub struct EpochSummary {
    pub epoch: u64,
    pub slots_in_epoch: u64,
    pub blocks_produced: u64,
    pub total_fees: u64,
    pub active_validators: usize,
    pub total_stake: u64,
}

impl SimNetwork {
    /// Create a new simulated network with the given validators.
    pub fn new(validator_stakes: &[(Pubkey, u64)]) -> Self {
        let validators: Vec<SimValidator> = validator_stakes
            .iter()
            .map(|(pk, stake)| SimValidator::new(*pk, *stake))
            .collect();

        let mut balances = HashMap::new();
        for v in &validators {
            // Each validator starts with their stake + some operating balance.
            balances.insert(v.pubkey, 100_000_000_000); // 100 SOL operating
        }

        Self {
            validators,
            current_slot: 0,
            current_epoch: 0,
            slots_per_epoch: SLOTS_PER_EPOCH,
            fee_config: FeeMarketConfig::default(),
            fee_state: BlockFeeState::genesis(FeeMarketConfig::default().min_base_fee),
            balances,
            total_fees_collected: 0,
            total_burned: 0,
            treasury_fees: 0,
            validator_fees: 0,
            developer_fees: 0,
            passive_stakes: Vec::new(),
            governance: None,
            proposals: Vec::new(),
            treasury: None,
            developer_reward_accounts: HashMap::new(),
            blocks_produced: 0,
            epoch_history: Vec::new(),
            simulated_unix_time: 1_700_000_000, // ~Nov 2023
        }
    }

    // ── Validator set helpers ────────────────────────────────────────────

    /// Build a `ValidatorSet` from current active validators (up to MAX_ACTIVE).
    pub fn active_validator_set(&self) -> ValidatorSet {
        let mut active: Vec<(Pubkey, u64)> = self
            .validators
            .iter()
            .filter(|v| v.status == ValidatorStatus::Active && v.online)
            .map(|v| (v.pubkey, v.total_stake()))
            .collect();
        // Sort by stake descending for the top-N cut.
        active.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        active.truncate(MAX_ACTIVE_VALIDATORS);
        ValidatorSet::new(active)
    }

    /// Returns the list of currently active validator pubkeys.
    pub fn active_validator_pubkeys(&self) -> Vec<Pubkey> {
        self.active_validator_set().pubkeys()
    }

    /// Number of currently active validators.
    pub fn active_validator_count(&self) -> usize {
        self.validators
            .iter()
            .filter(|v| v.status == ValidatorStatus::Active && v.online)
            .count()
            .min(MAX_ACTIVE_VALIDATORS)
    }

    /// Find a validator by pubkey (mutable).
    pub fn validator_mut(&mut self, pubkey: &Pubkey) -> Option<&mut SimValidator> {
        self.validators.iter_mut().find(|v| v.pubkey == *pubkey)
    }

    /// Find a validator by pubkey.
    pub fn validator(&self, pubkey: &Pubkey) -> Option<&SimValidator> {
        self.validators.iter().find(|v| v.pubkey == *pubkey)
    }

    /// Add a new validator to the network.
    pub fn add_validator(&mut self, pubkey: Pubkey, stake: u64) {
        self.validators.push(SimValidator::new(pubkey, stake));
        self.balances.entry(pubkey).or_insert(100_000_000_000);
    }

    /// Set a validator offline.
    pub fn set_validator_offline(&mut self, pubkey: &Pubkey) {
        if let Some(v) = self.validator_mut(pubkey) {
            v.online = false;
        }
    }

    /// Set a validator online.
    pub fn set_validator_online(&mut self, pubkey: &Pubkey) {
        if let Some(v) = self.validator_mut(pubkey) {
            v.online = true;
        }
    }

    /// Jail a validator.
    pub fn jail_validator(&mut self, pubkey: &Pubkey) {
        if let Some(v) = self.validator_mut(pubkey) {
            v.status = ValidatorStatus::Jailed;
            println!("  [JAIL] Validator {} jailed", pubkey);
        }
    }

    /// Unjail a validator (must be online).
    pub fn unjail_validator(&mut self, pubkey: &Pubkey) -> bool {
        if let Some(v) = self.validator_mut(pubkey) {
            if v.status == ValidatorStatus::Jailed && v.online {
                v.status = ValidatorStatus::Active;
                v.consecutive_missed = 0;
                println!("  [UNJAIL] Validator {} unjailed", pubkey);
                return true;
            }
        }
        false
    }

    /// Simulate a double-sign by a validator. Slash 5% of own stake only.
    pub fn slash_double_sign(&mut self, pubkey: &Pubkey) -> u64 {
        if let Some(v) = self.validator_mut(pubkey) {
            v.double_signed = true;
            let slash_amount = v.stake * DOUBLE_SIGN_SLASH_BPS / BPS_DENOM;
            v.stake = v.stake.saturating_sub(slash_amount);
            v.total_slashed += slash_amount;
            v.status = ValidatorStatus::Jailed;
            println!(
                "  [SLASH] Validator {} slashed {} lamports (5% of own stake), now jailed",
                pubkey, slash_amount
            );
            slash_amount
        } else {
            0
        }
    }

    // ── Block production ─────────────────────────────────────────────────

    /// Produce a single block (slot). Selects proposer round-robin by stake weight.
    pub fn produce_block(&mut self, transactions: &[SimTransaction]) -> u64 {
        let prev_epoch = self.current_epoch;
        self.current_slot += 1;
        self.current_epoch = self.current_slot / self.slots_per_epoch;
        self.simulated_unix_time += 1; // 1-second block time

        // Check for epoch boundary.
        if self.current_epoch > prev_epoch {
            self.on_epoch_transition(prev_epoch);
        }

        // Select proposer (simple stake-weighted round-robin).
        let active_set = self.active_validator_set();
        if active_set.is_empty() {
            println!("  [WARN] No active validators, skipping block");
            return 0;
        }
        let proposer_idx = (self.current_slot as usize) % active_set.len();
        let proposer_pk = active_set.get(proposer_idx).unwrap().pubkey;

        // Process missed-slot tracking for offline validators.
        for v in self.validators.iter_mut() {
            if v.status == ValidatorStatus::Active && !v.online {
                v.consecutive_missed += 1;
                if v.consecutive_missed >= JAIL_THRESHOLD_MISSED_SLOTS {
                    v.status = ValidatorStatus::Jailed;
                    println!(
                        "  [AUTO-JAIL] Validator {} jailed after {} missed slots",
                        v.pubkey, v.consecutive_missed
                    );
                }
            } else if v.online && v.status == ValidatorStatus::Active {
                v.consecutive_missed = 0;
            }
        }

        // Process transactions and collect fees.
        let mut block_cu = 0u64;
        let mut block_fees = 0u64;
        for tx in transactions {
            let fee = calculate_transaction_fee(
                self.fee_state.base_fee_per_cu,
                tx.priority_fee_per_cu,
                tx.compute_units,
            );
            block_fees += fee.total_fee;
            block_cu += tx.compute_units;

            // Deduct from sender balance.
            let sender_balance = self.balances.entry(tx.sender).or_insert(0);
            *sender_balance = sender_balance.saturating_sub(fee.total_fee);
        }

        // Record gas usage.
        self.fee_state.record_gas(block_cu);

        // Distribute fees according to schedule.
        self.distribute_fees(block_fees, &proposer_pk, transactions);

        // Advance fee state for next block.
        let next_base_fee = calculate_next_base_fee(&self.fee_config, &self.fee_state);
        self.fee_state = self.fee_state.next_block(next_base_fee, self.current_slot);

        self.blocks_produced += 1;

        // Reward the proposer for producing a block.
        if let Some(v) = self.validator_mut(&proposer_pk) {
            // Base block reward: a small fixed amount per block.
            let block_reward = 1_000_000; // 0.001 SOL
            v.rewards_earned += block_reward;
        }

        block_fees
    }

    /// Produce N empty blocks (fast-forward).
    pub fn produce_empty_blocks(&mut self, n: u64) {
        for _ in 0..n {
            self.produce_block(&[]);
        }
    }

    /// Produce blocks until reaching the target epoch.
    pub fn advance_to_epoch(&mut self, target_epoch: u64) {
        while self.current_epoch < target_epoch {
            self.produce_block(&[]);
        }
    }

    /// Produce one full epoch of empty blocks.
    pub fn produce_epoch(&mut self) {
        let target = self.current_epoch + 1;
        self.advance_to_epoch(target);
    }

    // ── Fee distribution ─────────────────────────────────────────────────

    fn distribute_fees(
        &mut self,
        total_fees: u64,
        proposer: &Pubkey,
        transactions: &[SimTransaction],
    ) {
        if total_fees == 0 {
            return;
        }
        self.total_fees_collected += total_fees;

        let split = fee_split_at_epoch(self.current_epoch);

        let burn = total_fees * split.burn_bps / BPS_DENOM;
        let to_validator = total_fees * split.validator_bps / BPS_DENOM;
        let to_treasury = total_fees * split.treasury_bps / BPS_DENOM;
        let to_developer = total_fees * split.developer_bps / BPS_DENOM;

        self.total_burned += burn;
        self.validator_fees += to_validator;
        self.treasury_fees += to_treasury;
        self.developer_fees += to_developer;

        // Credit the block proposer their validator share.
        if let Some(v) = self.validator_mut(proposer) {
            v.rewards_earned += to_validator;
        }

        // Credit the treasury.
        if let Some(ref mut t) = self.treasury {
            t.balance += to_treasury;
            t.total_received += to_treasury;
        }

        // Attribute developer fees to invoked programs.
        let programs_in_block: Vec<Pubkey> = transactions
            .iter()
            .filter_map(|tx| tx.invoked_program)
            .collect();
        if !programs_in_block.is_empty() {
            let per_program = to_developer / programs_in_block.len() as u64;
            for prog in &programs_in_block {
                *self.developer_reward_accounts.entry(*prog).or_insert(0) += per_program;
            }
        }
    }

    // ── Epoch transitions ────────────────────────────────────────────────

    fn on_epoch_transition(&mut self, completed_epoch: u64) {
        println!(
            "--- Epoch {} complete (slot {}) ---",
            completed_epoch, self.current_slot
        );

        // Calculate staking rewards for active validators.
        let total_active_stake: u64 = self
            .validators
            .iter()
            .filter(|v| v.status == ValidatorStatus::Active)
            .map(|v| v.total_stake())
            .sum();

        // Base validator APY: 5% → per-epoch ≈ 5% / 365 ≈ 0.0137% per epoch.
        // We use a simplified fixed reward per epoch per stake unit.
        let validator_reward_rate_bps: u64 = 500; // 5% APY expressed as bps.

        for v in self.validators.iter_mut() {
            if v.status == ValidatorStatus::Active {
                // Per-epoch reward: stake * rate / BPS / 365
                let epoch_reward = (v.total_stake() as u128)
                    .saturating_mul(validator_reward_rate_bps as u128)
                    / (BPS_DENOM as u128)
                    / 365;
                v.rewards_earned += epoch_reward as u64;
            }
        }

        // Calculate passive staking rewards.
        self.calculate_passive_staking_rewards(completed_epoch + 1, validator_reward_rate_bps);

        let summary = EpochSummary {
            epoch: completed_epoch,
            slots_in_epoch: self.slots_per_epoch,
            blocks_produced: self.blocks_produced,
            total_fees: self.total_fees_collected,
            active_validators: self.active_validator_count(),
            total_stake: total_active_stake,
        };
        self.epoch_history.push(summary);
    }

    // ── Passive staking ──────────────────────────────────────────────────

    /// Create a new passive stake position.
    pub fn create_passive_stake(
        &mut self,
        authority: Pubkey,
        amount: u64,
        lock_days: u64,
    ) -> usize {
        // Deduct from authority balance.
        let bal = self.balances.entry(authority).or_insert(0);
        *bal = bal.saturating_sub(amount);

        let (lock_end_epoch, is_permanent) = if lock_days == u64::MAX {
            (0, true)
        } else if lock_days == 0 {
            (0, false)
        } else {
            // Convert days to epochs (1 epoch ≈ 1 day simplification).
            (self.current_epoch + lock_days, false)
        };

        let vote_weight_bps = solana_passive_stake_program::constants::vote_weight_bps_for_tier(lock_days)
            .unwrap_or(0);

        let stake = SimPassiveStake {
            authority,
            amount,
            lock_days,
            lock_start_epoch: self.current_epoch,
            lock_end_epoch,
            unclaimed_rewards: 0,
            last_reward_epoch: self.current_epoch,
            is_permanent,
            vote_weight_bps,
            active: true,
        };

        self.passive_stakes.push(stake);
        let idx = self.passive_stakes.len() - 1;
        println!(
            "  [PASSIVE-STAKE] Created stake #{}: {} lamports, {} day lock for {}",
            idx, amount, lock_days, authority
        );
        idx
    }

    /// Calculate passive staking rewards for all active positions.
    fn calculate_passive_staking_rewards(&mut self, current_epoch: u64, validator_rate_bps: u64) {
        for stake in self.passive_stakes.iter_mut() {
            if !stake.active || current_epoch <= stake.last_reward_epoch {
                continue;
            }
            let epochs_elapsed = current_epoch - stake.last_reward_epoch;

            let tier_rate_bps =
                solana_passive_stake_program::constants::reward_rate_bps_for_tier(stake.lock_days)
                    .unwrap_or(0);

            // reward_per_epoch = amount × validator_rate × tier_rate / (BPS² × 365)
            let amount = stake.amount as u128;
            let v_rate = validator_rate_bps as u128;
            let t_rate = tier_rate_bps as u128;
            let denom = (BPS_DENOM as u128) * (BPS_DENOM as u128) * 365;

            let reward_per_epoch = amount * v_rate * t_rate / denom;
            let total_new = reward_per_epoch * epochs_elapsed as u128;

            stake.unclaimed_rewards += total_new as u64;
            stake.last_reward_epoch = current_epoch;
        }
    }

    /// Claim rewards from a passive stake position.
    pub fn claim_passive_rewards(&mut self, stake_idx: usize) -> u64 {
        let rewards = self.passive_stakes[stake_idx].unclaimed_rewards;
        let authority = self.passive_stakes[stake_idx].authority;
        self.passive_stakes[stake_idx].unclaimed_rewards = 0;

        *self.balances.entry(authority).or_insert(0) += rewards;
        println!(
            "  [CLAIM] Stake #{}: {} lamports claimed by {}",
            stake_idx, rewards, authority
        );
        rewards
    }

    /// Unlock a passive stake after lock expiry (returns principal).
    pub fn unlock_passive_stake(&mut self, stake_idx: usize) -> Result<u64, &'static str> {
        let stake = &self.passive_stakes[stake_idx];
        if !stake.active {
            return Err("Stake already unlocked");
        }
        if stake.is_permanent {
            return Err("Permanent locks cannot be unlocked");
        }
        if stake.lock_days != 0 && self.current_epoch < stake.lock_end_epoch {
            return Err("Lock period has not expired");
        }

        let principal = stake.amount;
        let authority = stake.authority;
        self.passive_stakes[stake_idx].active = false;

        *self.balances.entry(authority).or_insert(0) += principal;
        println!(
            "  [UNLOCK] Stake #{}: {} lamports returned to {}",
            stake_idx, principal, authority
        );
        Ok(principal)
    }

    /// Early-unlock a passive stake before lock expiry (penalty applied).
    pub fn early_unlock_passive_stake(&mut self, stake_idx: usize) -> Result<(u64, u64), &'static str> {
        let stake = &self.passive_stakes[stake_idx];
        if !stake.active {
            return Err("Stake already unlocked");
        }
        if stake.is_permanent {
            return Err("Permanent locks cannot be early-unlocked");
        }

        let penalty_bps =
            solana_passive_stake_program::constants::early_unlock_penalty_bps_for_tier(
                stake.lock_days,
            )
            .unwrap_or(0);

        let penalty = stake.amount * penalty_bps / BPS_DENOM;
        let returned = stake.amount.saturating_sub(penalty);
        let authority = stake.authority;

        self.passive_stakes[stake_idx].active = false;
        self.total_burned += penalty; // Penalty is burned.

        *self.balances.entry(authority).or_insert(0) += returned;
        println!(
            "  [EARLY-UNLOCK] Stake #{}: {} lamports returned, {} burned for {}",
            stake_idx, returned, penalty, authority
        );
        Ok((returned, penalty))
    }

    // ── Governance ───────────────────────────────────────────────────────

    /// Initialize governance in pre-activation (multisig) mode.
    pub fn init_governance(&mut self, authority: Pubkey, emergency_multisig: Pubkey) {
        self.governance = Some(SimGovernanceConfig {
            is_active: false,
            authority,
            proposal_threshold: 50_000_000_000_000, // 50k SOL
            voting_period_epochs: 7,
            quorum_bps: 3_000,
            pass_threshold_bps: 5_000,
            veto_threshold_bps: 3_333,
            timelock_epochs: 2,
            emergency_multisig,
            next_proposal_id: 0,
        });
        println!("  [GOV] Governance initialized (inactive), authority={}", authority);
    }

    /// Activate governance (one-way transition).
    pub fn activate_governance(&mut self) -> Result<(), &'static str> {
        let gov = self.governance.as_mut().ok_or("Governance not initialized")?;
        if gov.is_active {
            return Err("Governance already active");
        }
        gov.is_active = true;
        println!("  [GOV] Governance ACTIVATED");
        Ok(())
    }

    /// Create a proposal (multisig mode: proposer must be authority).
    pub fn create_proposal(
        &mut self,
        proposer: &Pubkey,
        title: &str,
        is_emergency_unlock: bool,
    ) -> Result<u64, &'static str> {
        let gov = self.governance.as_mut().ok_or("Governance not initialized")?;

        if !gov.is_active && *proposer != gov.authority {
            return Err("Only authority can create proposals when governance is inactive");
        }

        let id = gov.next_proposal_id;
        gov.next_proposal_id += 1;

        let voting_ends = self.current_epoch + gov.voting_period_epochs;
        let execution_epoch = voting_ends + gov.timelock_epochs;

        let initial_status = if gov.is_active {
            SimProposalStatus::Active
        } else {
            SimProposalStatus::Timelocked
        };

        self.proposals.push(SimProposal {
            id,
            proposer: *proposer,
            title: title.to_string(),
            status: initial_status,
            created_epoch: self.current_epoch,
            voting_ends_epoch: voting_ends,
            execution_epoch,
            votes_for: 0,
            votes_against: 0,
            votes_abstain: 0,
            veto_votes: 0,
            is_emergency_unlock,
            executed: false,
        });

        println!(
            "  [GOV] Proposal #{} created: '{}' status={:?}",
            id, title, initial_status
        );
        Ok(id)
    }

    /// Cast a vote on a proposal (governance must be active).
    pub fn cast_vote(
        &mut self,
        proposal_id: u64,
        voter_weight: u64,
        vote: &str, // "for", "against", "abstain", "veto"
    ) -> Result<(), &'static str> {
        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or("Proposal not found")?;

        if proposal.status != SimProposalStatus::Active {
            return Err("Proposal is not in Active status");
        }
        if self.current_epoch >= proposal.voting_ends_epoch {
            return Err("Voting period has ended");
        }

        match vote {
            "for" => proposal.votes_for += voter_weight,
            "against" => proposal.votes_against += voter_weight,
            "abstain" => proposal.votes_abstain += voter_weight,
            "veto" => proposal.veto_votes += voter_weight,
            _ => return Err("Invalid vote type"),
        }

        println!(
            "  [GOV] Vote '{}' on proposal #{} with weight {}",
            vote, proposal_id, voter_weight
        );
        Ok(())
    }

    /// Finalize a proposal after voting ends.
    pub fn finalize_proposal(&mut self, proposal_id: u64) -> Result<SimProposalStatus, &'static str> {
        let gov = self.governance.as_ref().ok_or("Governance not initialized")?;
        let pass_bps = gov.pass_threshold_bps;
        let veto_bps = gov.veto_threshold_bps;
        let emergency_pass_bps: u16 = 8_000; // 80% for emergency unlock.

        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or("Proposal not found")?;

        if proposal.status != SimProposalStatus::Active {
            return Err("Proposal is not Active");
        }
        if self.current_epoch < proposal.voting_ends_epoch {
            return Err("Voting period not yet ended");
        }

        let total_votes = proposal.votes_for + proposal.votes_against + proposal.votes_abstain + proposal.veto_votes;
        if total_votes == 0 {
            proposal.status = SimProposalStatus::Expired;
            return Ok(SimProposalStatus::Expired);
        }

        // Check veto.
        let veto_pct = proposal.veto_votes * BPS_DENOM / total_votes;
        if veto_pct >= veto_bps as u64 {
            proposal.status = SimProposalStatus::Vetoed;
            return Ok(SimProposalStatus::Vetoed);
        }

        // Check pass threshold.
        let decisive = proposal.votes_for + proposal.votes_against;
        if decisive == 0 {
            proposal.status = SimProposalStatus::Rejected;
            return Ok(SimProposalStatus::Rejected);
        }

        let for_pct = proposal.votes_for * BPS_DENOM / decisive;
        let required = if proposal.is_emergency_unlock {
            emergency_pass_bps as u64
        } else {
            pass_bps as u64
        };

        if for_pct >= required {
            proposal.status = SimProposalStatus::Timelocked;
            println!(
                "  [GOV] Proposal #{} PASSED → Timelocked until epoch {}",
                proposal_id, proposal.execution_epoch
            );
            Ok(SimProposalStatus::Timelocked)
        } else {
            proposal.status = SimProposalStatus::Rejected;
            Ok(SimProposalStatus::Rejected)
        }
    }

    /// Execute a timelocked proposal.
    pub fn execute_proposal(&mut self, proposal_id: u64) -> Result<(), &'static str> {
        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or("Proposal not found")?;

        if proposal.status != SimProposalStatus::Timelocked {
            return Err("Proposal is not Timelocked");
        }
        if self.current_epoch < proposal.execution_epoch {
            return Err("Timelock has not expired");
        }

        proposal.status = SimProposalStatus::Executed;
        proposal.executed = true;
        println!("  [GOV] Proposal #{} EXECUTED", proposal_id);
        Ok(())
    }

    /// Cancel a proposal (emergency multisig only).
    pub fn cancel_proposal(&mut self, proposal_id: u64, signer: &Pubkey) -> Result<(), &'static str> {
        let gov = self.governance.as_ref().ok_or("Governance not initialized")?;
        if *signer != gov.emergency_multisig {
            return Err("Only emergency multisig can cancel");
        }

        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or("Proposal not found")?;

        match proposal.status {
            SimProposalStatus::Active | SimProposalStatus::Timelocked => {
                proposal.status = SimProposalStatus::Cancelled;
                println!("  [GOV] Proposal #{} CANCELLED by emergency multisig", proposal_id);
                Ok(())
            }
            _ => Err("Cannot cancel proposal in this status"),
        }
    }

    // ── Treasury ─────────────────────────────────────────────────────────

    /// Initialize the treasury.
    pub fn init_treasury(&mut self, authority: Pubkey) {
        self.treasury = Some(SimTreasury {
            authority,
            balance: 0,
            total_received: 0,
            total_disbursed: 0,
            governance_active: false,
        });
        println!("  [TREASURY] Initialized with authority={}", authority);
    }

    /// Disburse from treasury.
    pub fn disburse_treasury(
        &mut self,
        signer: &Pubkey,
        recipient: &Pubkey,
        amount: u64,
    ) -> Result<(), &'static str> {
        let treasury = self.treasury.as_mut().ok_or("Treasury not initialized")?;
        if *signer != treasury.authority {
            return Err("Signer is not the treasury authority");
        }
        if treasury.balance < amount {
            return Err("Insufficient treasury balance");
        }
        treasury.balance -= amount;
        treasury.total_disbursed += amount;

        *self.balances.entry(*recipient).or_insert(0) += amount;
        println!(
            "  [TREASURY] Disbursed {} lamports to {}",
            amount, recipient
        );
        Ok(())
    }

    /// Transfer treasury authority.
    pub fn transfer_treasury_authority(
        &mut self,
        current_signer: &Pubkey,
        new_authority: &Pubkey,
    ) -> Result<(), &'static str> {
        let treasury = self.treasury.as_mut().ok_or("Treasury not initialized")?;
        if *current_signer != treasury.authority {
            return Err("Signer is not the treasury authority");
        }
        treasury.authority = *new_authority;
        println!(
            "  [TREASURY] Authority transferred to {}",
            new_authority
        );
        Ok(())
    }

    // ── Utility ──────────────────────────────────────────────────────────

    /// Get or create a balance entry.
    pub fn balance(&self, pubkey: &Pubkey) -> u64 {
        *self.balances.get(pubkey).unwrap_or(&0)
    }

    /// Credit lamports to an account (for test setup).
    pub fn credit(&mut self, pubkey: &Pubkey, amount: u64) {
        *self.balances.entry(*pubkey).or_insert(0) += amount;
    }

    /// Print a summary of the current network state.
    pub fn print_summary(&self) {
        println!("\n=== Network Summary ===");
        println!("Slot: {} | Epoch: {}", self.current_slot, self.current_epoch);
        println!("Blocks produced: {}", self.blocks_produced);
        println!(
            "Fee state: base_fee={} lamports/CU",
            self.fee_state.base_fee_per_cu
        );
        println!(
            "Fees: collected={} burned={} treasury={} validator={} developer={}",
            self.total_fees_collected,
            self.total_burned,
            self.treasury_fees,
            self.validator_fees,
            self.developer_fees
        );
        println!("Validators:");
        for v in &self.validators {
            println!(
                "  {} — stake={} status={:?} rewards={} online={} delegations={}",
                v.pubkey,
                v.stake,
                v.status,
                v.rewards_earned,
                v.online,
                v.total_delegation
            );
        }
        if let Some(ref t) = self.treasury {
            println!(
                "Treasury: balance={} received={} disbursed={}",
                t.balance, t.total_received, t.total_disbursed
            );
        }
        println!("Passive stakes: {} active", self.passive_stakes.iter().filter(|s| s.active).count());
        println!("========================\n");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test helper functions
// ─────────────────────────────────────────────────────────────────────────────

/// Create N unique pubkeys.
pub fn make_pubkeys(n: usize) -> Vec<Pubkey> {
    (0..n).map(|_| Pubkey::new_unique()).collect()
}

/// Create a standard 3-validator network.
pub fn standard_3_validator_network() -> (SimNetwork, Vec<Pubkey>) {
    let pks = make_pubkeys(3);
    let stakes = vec![
        (pks[0], 1_000_000_000_000),  // 1000 SOL
        (pks[1], 2_000_000_000_000),  // 2000 SOL
        (pks[2], 3_000_000_000_000),  // 3000 SOL
    ];
    let mut net = SimNetwork::new(&stakes);
    let authority = pks[0]; // Use first validator as authority for governance/treasury.
    let emergency = Pubkey::new_unique();
    net.init_governance(authority, emergency);
    net.init_treasury(authority);
    (net, pks)
}

/// Generate a batch of random transactions.
pub fn random_transactions(n: usize, senders: &[Pubkey]) -> Vec<SimTransaction> {
    let mut rng = rand::rng();
    (0..n)
        .map(|_| SimTransaction {
            sender: senders[rng.random_range(0..senders.len())],
            compute_units: rng.random_range(10_000..500_000),
            priority_fee_per_cu: rng.random_range(0..1_000),
            invoked_program: None,
        })
        .collect()
}

/// Generate transactions that invoke a specific program.
pub fn program_transactions(
    n: usize,
    sender: Pubkey,
    program: Pubkey,
) -> Vec<SimTransaction> {
    let mut rng = rand::rng();
    (0..n)
        .map(|_| SimTransaction {
            sender,
            compute_units: rng.random_range(50_000..200_000),
            priority_fee_per_cu: rng.random_range(100..500),
            invoked_program: Some(program),
        })
        .collect()
}

/// Initialize env_logger once for test output.
pub fn init_logging() {
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Info)
        .try_init();
}

//! Response types for TRv1-specific RPC endpoints.
//!
//! These structs are shared between the RPC server and clients.
//! All types derive Serialize + Deserialize for JSON-RPC transport.

use serde::{Deserialize, Serialize};

// ─── Passive Staking ────────────────────────────────────────────────────────

/// Information about a single passive stake account.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PassiveStakeAccountInfo {
    /// The passive-stake account pubkey (base-58).
    pub pubkey: String,
    /// Owner wallet that created this stake (base-58).
    pub owner: String,
    /// Lamports currently staked.
    pub staked_lamports: u64,
    /// Staking tier (0 = flexible, 1 = 30-day, 2 = 90-day, 3 = 180-day).
    pub tier: u8,
    /// Human-readable tier name.
    pub tier_name: String,
    /// Unix timestamp when the stake was created.
    pub activated_at: i64,
    /// Unix timestamp when the lock-up expires (0 for flexible tier).
    pub lockup_expires_at: i64,
    /// Whether the stake is currently withdrawable.
    pub is_withdrawable: bool,
    /// Total rewards earned to date (lamports).
    pub total_rewards_earned: u64,
    /// Last epoch in which rewards were credited.
    pub last_reward_epoch: u64,
    /// Current annual percentage yield for this tier (basis points, e.g. 450 = 4.50%).
    pub current_apy_bps: u16,
}

/// Current passive staking rates for every tier.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PassiveStakingRates {
    /// Current epoch.
    pub epoch: u64,
    /// Per-tier rate information.
    pub tiers: Vec<PassiveStakingTierRate>,
    /// Total lamports staked across all tiers.
    pub total_staked_lamports: u64,
    /// Total number of passive stake accounts.
    pub total_accounts: u64,
}

/// Rate info for one staking tier.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PassiveStakingTierRate {
    pub tier: u8,
    pub tier_name: String,
    /// Lock-up duration in days (0 = flexible).
    pub lockup_days: u16,
    /// Annual percentage yield (basis points).
    pub apy_bps: u16,
    /// Total lamports staked in this tier.
    pub total_staked_lamports: u64,
    /// Number of accounts in this tier.
    pub account_count: u64,
}

// ─── Fee Market ─────────────────────────────────────────────────────────────

/// Current base fee information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BaseFeeInfo {
    /// Current base fee in lamports per signature.
    pub base_fee_lamports: u64,
    /// Slot at which this fee was sampled.
    pub slot: u64,
    /// The EIP-1559-style utilization ratio (0.0 – 1.0).
    pub utilization_ratio: f64,
    /// Minimum possible base fee (floor).
    pub min_base_fee_lamports: u64,
    /// Maximum possible base fee (ceiling).
    pub max_base_fee_lamports: u64,
    /// Base fee change direction: "up", "down", or "stable".
    pub trend: String,
}

/// Fee statistics for a single block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BlockFeeInfo {
    /// Slot number.
    pub slot: u64,
    /// Base fee in effect for this block (lamports).
    pub base_fee_lamports: u64,
    /// Average priority fee paid (lamports).
    pub avg_priority_fee_lamports: u64,
    /// Median priority fee paid (lamports).
    pub median_priority_fee_lamports: u64,
    /// Max priority fee paid (lamports).
    pub max_priority_fee_lamports: u64,
    /// Number of transactions in the block.
    pub transaction_count: u64,
    /// Block utilization as fraction (0.0 – 1.0).
    pub utilization_ratio: f64,
}

/// Fee estimate for a proposed transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FeeEstimate {
    /// Estimated base fee (lamports).
    pub base_fee_lamports: u64,
    /// Recommended priority fee for fast inclusion (lamports).
    pub recommended_priority_fee_lamports: u64,
    /// Total estimated fee (base + priority, lamports).
    pub total_estimated_fee_lamports: u64,
    /// Estimated landing slot range.
    pub estimated_slot: u64,
    /// Confidence level: "low", "medium", "high".
    pub confidence: String,
}

// ─── Validators ─────────────────────────────────────────────────────────────

/// Information about a validator in the active or standby set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Trv1ValidatorInfo {
    /// Validator identity pubkey (base-58).
    pub identity: String,
    /// Vote account pubkey (base-58).
    pub vote_account: String,
    /// Human-readable node name (from gossip).
    pub name: Option<String>,
    /// Commission percentage (0–100).
    pub commission: u8,
    /// Total active stake delegated to this validator (lamports).
    pub active_stake_lamports: u64,
    /// Whether the validator is in the active set.
    pub is_active: bool,
    /// Rank by stake (1 = highest stake in active set).
    pub rank: u32,
    /// Last vote slot.
    pub last_vote_slot: u64,
    /// Root slot.
    pub root_slot: u64,
    /// Whether the validator is currently delinquent.
    pub is_delinquent: bool,
    /// Uptime percentage for the current epoch.
    pub epoch_uptime_pct: f64,
    /// Number of blocks produced this epoch.
    pub epoch_blocks_produced: u64,
    /// Number of leader slots this epoch.
    pub epoch_leader_slots: u64,
    /// Whether the validator is currently jailed.
    pub is_jailed: bool,
    /// Software version string.
    pub version: Option<String>,
}

/// Slashing history for a validator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SlashingInfo {
    /// Validator identity (base-58).
    pub validator: String,
    /// Total number of slashing events.
    pub total_slashing_events: u32,
    /// Total lamports slashed.
    pub total_lamports_slashed: u64,
    /// Individual slashing events.
    pub events: Vec<SlashingEvent>,
}

/// A single slashing event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SlashingEvent {
    /// Epoch in which slashing occurred.
    pub epoch: u64,
    /// Slot at which the offense was detected.
    pub slot: u64,
    /// Reason for slashing.
    pub reason: String,
    /// Lamports slashed.
    pub lamports_slashed: u64,
    /// Percentage of stake slashed (basis points).
    pub slash_pct_bps: u16,
}

/// Jail status for a validator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JailStatus {
    /// Validator identity (base-58).
    pub validator: String,
    /// Whether the validator is currently jailed.
    pub is_jailed: bool,
    /// Epoch when the jail sentence started (None if not jailed).
    pub jailed_since_epoch: Option<u64>,
    /// Epoch when the jail sentence ends (None if not jailed).
    pub release_epoch: Option<u64>,
    /// Reason for jailing.
    pub reason: Option<String>,
    /// Number of times this validator has been jailed.
    pub total_jail_count: u32,
}

// ─── Treasury ───────────────────────────────────────────────────────────────

/// Treasury account information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TreasuryInfo {
    /// Treasury account pubkey (base-58).
    pub treasury_pubkey: String,
    /// Current balance (lamports).
    pub balance_lamports: u64,
    /// Total lamports ever received by the treasury.
    pub total_received_lamports: u64,
    /// Total lamports disbursed from the treasury.
    pub total_disbursed_lamports: u64,
    /// Current epoch.
    pub epoch: u64,
    /// Lamports received this epoch.
    pub epoch_received_lamports: u64,
    /// Lamports disbursed this epoch.
    pub epoch_disbursed_lamports: u64,
    /// Percentage of fees routed to treasury (basis points).
    pub fee_share_bps: u16,
}

// ─── Governance ─────────────────────────────────────────────────────────────

/// Governance module configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GovernanceConfig {
    /// Governance program ID (base-58).
    pub program_id: String,
    /// Minimum stake required to create a proposal (lamports).
    pub min_proposal_stake_lamports: u64,
    /// Voting period in slots.
    pub voting_period_slots: u64,
    /// Voting period in approximate hours.
    pub voting_period_hours: f64,
    /// Quorum required for a proposal to pass (basis points of total stake).
    pub quorum_bps: u16,
    /// Super-majority threshold for passage (basis points of voting stake).
    pub pass_threshold_bps: u16,
    /// Cooldown period after voting ends before execution (slots).
    pub execution_cooldown_slots: u64,
    /// Maximum number of concurrent active proposals.
    pub max_active_proposals: u32,
}

/// A governance proposal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProposalInfo {
    /// Unique proposal ID.
    pub proposal_id: u64,
    /// Proposer pubkey (base-58).
    pub proposer: String,
    /// Short title.
    pub title: String,
    /// Description / rationale (may be truncated in list view).
    pub description: String,
    /// Status: "pending", "active", "passed", "rejected", "executed", "cancelled".
    pub status: String,
    /// Epoch when the proposal was created.
    pub created_epoch: u64,
    /// Slot when voting starts.
    pub voting_start_slot: u64,
    /// Slot when voting ends.
    pub voting_end_slot: u64,
    /// Total "yes" stake (lamports).
    pub yes_stake_lamports: u64,
    /// Total "no" stake (lamports).
    pub no_stake_lamports: u64,
    /// Total "abstain" stake (lamports).
    pub abstain_stake_lamports: u64,
    /// Current quorum reached (basis points of total stake).
    pub quorum_reached_bps: u16,
    /// Whether quorum has been met.
    pub quorum_met: bool,
    /// Proposal type: "parameter_change", "program_upgrade", "treasury_spend", "text".
    pub proposal_type: String,
    /// Encoded proposal payload (type-dependent).
    pub payload: Option<String>,
}

// ─── Developer Rewards ──────────────────────────────────────────────────────

/// Developer reward configuration for a program.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DevRewardsConfig {
    /// Program ID (base-58).
    pub program_id: String,
    /// Reward recipient pubkey (base-58). Defaults to program upgrade authority.
    pub reward_recipient: String,
    /// Whether developer rewards are enabled for this program.
    pub is_enrolled: bool,
    /// Share of transaction fees routed to developer (basis points).
    pub developer_share_bps: u16,
    /// Total rewards earned to date (lamports).
    pub total_rewards_earned_lamports: u64,
    /// Rewards earned this epoch (lamports).
    pub epoch_rewards_lamports: u64,
    /// Number of transactions invoking this program this epoch.
    pub epoch_transaction_count: u64,
}

/// Earnings summary for a program in the current epoch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProgramEarnings {
    /// Program ID (base-58).
    pub program_id: String,
    /// Reward recipient (base-58).
    pub reward_recipient: String,
    /// Lamports earned this epoch.
    pub epoch_earnings_lamports: u64,
    /// Number of transactions invoking this program this epoch.
    pub epoch_transaction_count: u64,
    /// Rank by earnings (1 = top earner).
    pub rank: u32,
}

// ─── Network Info ───────────────────────────────────────────────────────────

/// High-level TRv1 network summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSummary {
    /// Current epoch.
    pub epoch: u64,
    /// Current slot.
    pub slot: u64,
    /// Block height.
    pub block_height: u64,
    /// Total supply (lamports).
    pub total_supply_lamports: u64,
    /// Circulating supply (lamports).
    pub circulating_supply_lamports: u64,
    /// Active stake as percentage of total supply (basis points).
    pub staking_participation_bps: u16,
    /// Total lamports in active (delegated) stake.
    pub active_stake_lamports: u64,
    /// Total lamports in passive stake.
    pub passive_stake_lamports: u64,
    /// Number of active validators.
    pub active_validator_count: u32,
    /// Number of standby validators.
    pub standby_validator_count: u32,
    /// Current base fee (lamports).
    pub current_base_fee_lamports: u64,
    /// Current inflation rate (basis points per epoch).
    pub inflation_rate_bps: u16,
    /// TPS averaged over the last 60 seconds.
    pub recent_tps: f64,
    /// Network version string.
    pub version: String,
}

/// Fee distribution breakdown for the current epoch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FeeDistributionInfo {
    /// Current epoch.
    pub epoch: u64,
    /// Total fees collected this epoch (lamports).
    pub total_fees_collected_lamports: u64,
    /// Amount burned (lamports).
    pub burned_lamports: u64,
    /// Amount sent to treasury (lamports).
    pub treasury_lamports: u64,
    /// Amount distributed to validators (lamports).
    pub validator_lamports: u64,
    /// Amount distributed to developers (lamports).
    pub developer_lamports: u64,
    /// Amount routed to passive staking rewards pool (lamports).
    pub passive_staking_lamports: u64,
    /// Burn percentage (basis points).
    pub burn_share_bps: u16,
    /// Treasury percentage (basis points).
    pub treasury_share_bps: u16,
    /// Validator percentage (basis points).
    pub validator_share_bps: u16,
    /// Developer percentage (basis points).
    pub developer_share_bps: u16,
    /// Passive staking percentage (basis points).
    pub passive_staking_share_bps: u16,
}

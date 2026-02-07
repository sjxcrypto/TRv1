//! TRv1-specific JSON-RPC endpoints.
//!
//! This module adds RPC methods for features unique to the TRv1 network:
//! passive staking, dynamic fee market, validator management (active/standby/
//! slashing/jail), treasury, governance, developer rewards, and network
//! summary endpoints.
//!
//! All method names are prefixed with `trv1_` to avoid collisions with the
//! upstream Solana/Agave RPC namespace.

use {
    crate::rpc::JsonRpcRequestProcessor,
    jsonrpc_core::{Error, Result},
    jsonrpc_derive::rpc,
    solana_rpc_client_api::trv1_response::*,
};

// ────────────────────────────────────────────────────────────────────────────
// Trait definition (generates the JSON-RPC dispatch table via `jsonrpc-derive`)
// ────────────────────────────────────────────────────────────────────────────

pub mod rpc_trv1 {
    use super::*;

    #[rpc]
    pub trait Trv1 {
        type Metadata;

        // ── Passive Staking ─────────────────────────────────────────────

        /// Returns details of a single passive-stake account.
        #[rpc(meta, name = "trv1_getPassiveStakeAccount")]
        fn get_passive_stake_account(
            &self,
            meta: Self::Metadata,
            pubkey: String,
        ) -> Result<PassiveStakeAccountInfo>;

        /// Returns all passive-stake accounts owned by the given wallet.
        #[rpc(meta, name = "trv1_getPassiveStakesByOwner")]
        fn get_passive_stakes_by_owner(
            &self,
            meta: Self::Metadata,
            owner: String,
        ) -> Result<Vec<PassiveStakeAccountInfo>>;

        /// Returns the current passive-staking APY for every tier.
        #[rpc(meta, name = "trv1_getPassiveStakingRates")]
        fn get_passive_staking_rates(
            &self,
            meta: Self::Metadata,
        ) -> Result<PassiveStakingRates>;

        // ── Fee Market ──────────────────────────────────────────────────

        /// Returns the current EIP-1559-style base fee.
        #[rpc(meta, name = "trv1_getCurrentBaseFee")]
        fn get_current_base_fee(
            &self,
            meta: Self::Metadata,
        ) -> Result<BaseFeeInfo>;

        /// Returns fee statistics for the last `blocks` blocks.
        #[rpc(meta, name = "trv1_getFeeHistory")]
        fn get_fee_history(
            &self,
            meta: Self::Metadata,
            blocks: u64,
        ) -> Result<Vec<BlockFeeInfo>>;

        /// Estimates the total fee for a base64-encoded transaction.
        #[rpc(meta, name = "trv1_estimateFee")]
        fn estimate_fee(
            &self,
            meta: Self::Metadata,
            transaction: String,
        ) -> Result<FeeEstimate>;

        // ── Validators ──────────────────────────────────────────────────

        /// Returns the active validator set (top 200 by stake).
        #[rpc(meta, name = "trv1_getActiveValidators")]
        fn get_active_validators(
            &self,
            meta: Self::Metadata,
        ) -> Result<Vec<Trv1ValidatorInfo>>;

        /// Returns validators in the standby set.
        #[rpc(meta, name = "trv1_getStandbyValidators")]
        fn get_standby_validators(
            &self,
            meta: Self::Metadata,
        ) -> Result<Vec<Trv1ValidatorInfo>>;

        /// Returns the slashing history for a given validator.
        #[rpc(meta, name = "trv1_getSlashingInfo")]
        fn get_slashing_info(
            &self,
            meta: Self::Metadata,
            validator: String,
        ) -> Result<SlashingInfo>;

        /// Returns the jail status for a given validator.
        #[rpc(meta, name = "trv1_getJailStatus")]
        fn get_jail_status(
            &self,
            meta: Self::Metadata,
            validator: String,
        ) -> Result<JailStatus>;

        // ── Treasury ────────────────────────────────────────────────────

        /// Returns treasury balance and flow statistics.
        #[rpc(meta, name = "trv1_getTreasuryInfo")]
        fn get_treasury_info(
            &self,
            meta: Self::Metadata,
        ) -> Result<TreasuryInfo>;

        // ── Governance ──────────────────────────────────────────────────

        /// Returns the governance module configuration.
        #[rpc(meta, name = "trv1_getGovernanceConfig")]
        fn get_governance_config(
            &self,
            meta: Self::Metadata,
        ) -> Result<GovernanceConfig>;

        /// Returns governance proposals, optionally filtered by status.
        #[rpc(meta, name = "trv1_getProposals")]
        fn get_proposals(
            &self,
            meta: Self::Metadata,
            status: Option<String>,
        ) -> Result<Vec<ProposalInfo>>;

        /// Returns the details of a single proposal by ID.
        #[rpc(meta, name = "trv1_getProposal")]
        fn get_proposal(
            &self,
            meta: Self::Metadata,
            proposal_id: u64,
        ) -> Result<ProposalInfo>;

        // ── Developer Rewards ───────────────────────────────────────────

        /// Returns the developer-reward configuration for a program.
        #[rpc(meta, name = "trv1_getDevRewardsConfig")]
        fn get_dev_rewards_config(
            &self,
            meta: Self::Metadata,
            program_id: String,
        ) -> Result<DevRewardsConfig>;

        /// Returns the top-earning programs for the current epoch.
        #[rpc(meta, name = "trv1_getTopEarningPrograms")]
        fn get_top_earning_programs(
            &self,
            meta: Self::Metadata,
            limit: Option<u64>,
        ) -> Result<Vec<ProgramEarnings>>;

        // ── Network Info ────────────────────────────────────────────────

        /// Returns a high-level network summary.
        #[rpc(meta, name = "trv1_getNetworkSummary")]
        fn get_network_summary(
            &self,
            meta: Self::Metadata,
        ) -> Result<NetworkSummary>;

        /// Returns the fee-distribution breakdown for the current epoch.
        #[rpc(meta, name = "trv1_getFeeDistribution")]
        fn get_fee_distribution(
            &self,
            meta: Self::Metadata,
        ) -> Result<FeeDistributionInfo>;
    }

    // ────────────────────────────────────────────────────────────────────
    // Implementation
    // ────────────────────────────────────────────────────────────────────
    //
    // Each method delegates to `JsonRpcRequestProcessor` (the `meta` object).
    // During early development the implementations return placeholder / stub
    // data so that the RPC interface can be exercised end-to-end before the
    // on-chain programs that back these queries are fully deployed.
    // ────────────────────────────────────────────────────────────────────

    pub struct Trv1Impl;

    impl Trv1 for Trv1Impl {
        type Metadata = JsonRpcRequestProcessor;

        // ── Passive Staking ─────────────────────────────────────────────

        fn get_passive_stake_account(
            &self,
            _meta: Self::Metadata,
            pubkey: String,
        ) -> Result<PassiveStakeAccountInfo> {
            // TODO: Read account from bank via meta, decode PassiveStake state
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: format!(
                    "trv1_getPassiveStakeAccount not yet implemented (queried: {pubkey})"
                ),
                data: None,
            })
        }

        fn get_passive_stakes_by_owner(
            &self,
            _meta: Self::Metadata,
            owner: String,
        ) -> Result<Vec<PassiveStakeAccountInfo>> {
            // TODO: Scan accounts owned by passive-staking program, filter by owner field
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: format!(
                    "trv1_getPassiveStakesByOwner not yet implemented (owner: {owner})"
                ),
                data: None,
            })
        }

        fn get_passive_staking_rates(
            &self,
            _meta: Self::Metadata,
        ) -> Result<PassiveStakingRates> {
            // TODO: Read global passive-staking config account
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: "trv1_getPassiveStakingRates not yet implemented".to_string(),
                data: None,
            })
        }

        // ── Fee Market ──────────────────────────────────────────────────

        fn get_current_base_fee(
            &self,
            _meta: Self::Metadata,
        ) -> Result<BaseFeeInfo> {
            // TODO: Read dynamic fee state from bank
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: "trv1_getCurrentBaseFee not yet implemented".to_string(),
                data: None,
            })
        }

        fn get_fee_history(
            &self,
            _meta: Self::Metadata,
            blocks: u64,
        ) -> Result<Vec<BlockFeeInfo>> {
            if blocks == 0 || blocks > 1024 {
                return Err(Error::invalid_params(
                    "blocks must be between 1 and 1024",
                ));
            }
            // TODO: Query blockstore for recent fee data
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: format!(
                    "trv1_getFeeHistory not yet implemented (blocks: {blocks})"
                ),
                data: None,
            })
        }

        fn estimate_fee(
            &self,
            _meta: Self::Metadata,
            transaction: String,
        ) -> Result<FeeEstimate> {
            if transaction.is_empty() {
                return Err(Error::invalid_params(
                    "transaction must be a non-empty base64-encoded string",
                ));
            }
            // TODO: Decode tx, simulate fee computation using current fee state
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: "trv1_estimateFee not yet implemented".to_string(),
                data: None,
            })
        }

        // ── Validators ──────────────────────────────────────────────────

        fn get_active_validators(
            &self,
            _meta: Self::Metadata,
        ) -> Result<Vec<Trv1ValidatorInfo>> {
            // TODO: Read vote accounts, sort by stake, return top 200
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: "trv1_getActiveValidators not yet implemented".to_string(),
                data: None,
            })
        }

        fn get_standby_validators(
            &self,
            _meta: Self::Metadata,
        ) -> Result<Vec<Trv1ValidatorInfo>> {
            // TODO: Read vote accounts beyond rank 200
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: "trv1_getStandbyValidators not yet implemented".to_string(),
                data: None,
            })
        }

        fn get_slashing_info(
            &self,
            _meta: Self::Metadata,
            validator: String,
        ) -> Result<SlashingInfo> {
            // TODO: Read slashing records from on-chain log
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: format!(
                    "trv1_getSlashingInfo not yet implemented (validator: {validator})"
                ),
                data: None,
            })
        }

        fn get_jail_status(
            &self,
            _meta: Self::Metadata,
            validator: String,
        ) -> Result<JailStatus> {
            // TODO: Read jail account state
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: format!(
                    "trv1_getJailStatus not yet implemented (validator: {validator})"
                ),
                data: None,
            })
        }

        // ── Treasury ────────────────────────────────────────────────────

        fn get_treasury_info(
            &self,
            _meta: Self::Metadata,
        ) -> Result<TreasuryInfo> {
            // TODO: Read treasury account + config account
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: "trv1_getTreasuryInfo not yet implemented".to_string(),
                data: None,
            })
        }

        // ── Governance ──────────────────────────────────────────────────

        fn get_governance_config(
            &self,
            _meta: Self::Metadata,
        ) -> Result<GovernanceConfig> {
            // TODO: Read governance config account
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: "trv1_getGovernanceConfig not yet implemented".to_string(),
                data: None,
            })
        }

        fn get_proposals(
            &self,
            _meta: Self::Metadata,
            status: Option<String>,
        ) -> Result<Vec<ProposalInfo>> {
            // Validate status filter if provided
            if let Some(ref s) = status {
                let valid = ["pending", "active", "passed", "rejected", "executed", "cancelled"];
                if !valid.contains(&s.as_str()) {
                    return Err(Error::invalid_params(format!(
                        "invalid status filter '{s}'; valid values: {valid:?}"
                    )));
                }
            }
            // TODO: Scan proposal accounts, optionally filter
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: format!(
                    "trv1_getProposals not yet implemented (status: {status:?})"
                ),
                data: None,
            })
        }

        fn get_proposal(
            &self,
            _meta: Self::Metadata,
            proposal_id: u64,
        ) -> Result<ProposalInfo> {
            // TODO: Derive PDA from proposal_id, read account
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: format!(
                    "trv1_getProposal not yet implemented (id: {proposal_id})"
                ),
                data: None,
            })
        }

        // ── Developer Rewards ───────────────────────────────────────────

        fn get_dev_rewards_config(
            &self,
            _meta: Self::Metadata,
            program_id: String,
        ) -> Result<DevRewardsConfig> {
            // TODO: Derive PDA for dev-rewards config, read account
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: format!(
                    "trv1_getDevRewardsConfig not yet implemented (program: {program_id})"
                ),
                data: None,
            })
        }

        fn get_top_earning_programs(
            &self,
            _meta: Self::Metadata,
            limit: Option<u64>,
        ) -> Result<Vec<ProgramEarnings>> {
            let limit = limit.unwrap_or(10);
            if limit == 0 || limit > 100 {
                return Err(Error::invalid_params(
                    "limit must be between 1 and 100",
                ));
            }
            // TODO: Scan dev-rewards accounts, sort by epoch earnings
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: format!(
                    "trv1_getTopEarningPrograms not yet implemented (limit: {limit})"
                ),
                data: None,
            })
        }

        // ── Network Info ────────────────────────────────────────────────

        fn get_network_summary(
            &self,
            _meta: Self::Metadata,
        ) -> Result<NetworkSummary> {
            // TODO: Aggregate data from bank, fee state, validator set, etc.
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: "trv1_getNetworkSummary not yet implemented".to_string(),
                data: None,
            })
        }

        fn get_fee_distribution(
            &self,
            _meta: Self::Metadata,
        ) -> Result<FeeDistributionInfo> {
            // TODO: Read fee-distribution state for current epoch
            Err(Error {
                code: jsonrpc_core::ErrorCode::ServerError(-32000),
                message: "trv1_getFeeDistribution not yet implemented".to_string(),
                data: None,
            })
        }
    }
}

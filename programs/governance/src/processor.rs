//! Instruction processing logic for the TRv1 Governance program.

use {
    crate::{
        constants::{BPS_DENOMINATOR, EMERGENCY_UNLOCK_PASS_THRESHOLD_BPS},
        error::GovernanceError,
        instruction::GovernanceInstruction,
        state::{
            GovernanceConfig, Proposal, ProposalStatus, ProposalType, Vote, VoteRecord,
            GOVERNANCE_CONFIG_DISCRIMINATOR, PROPOSAL_DISCRIMINATOR, VOTE_RECORD_DISCRIMINATOR,
        },
        vote_weight::voting_power_from_passive_stake_data,
    },
    log::*,
    solana_bincode::limited_deserialize,
    solana_hash::Hash,
    solana_instruction::error::InstructionError,
    solana_program_runtime::{declare_process_instruction, invoke_context::InvokeContext},
    solana_pubkey::Pubkey,
    solana_svm_log_collector::ic_msg,
};

/// Default compute-unit budget for governance instructions.
pub const DEFAULT_COMPUTE_UNITS: u64 = 2_000;

// ---------------------------------------------------------------------------
// Program ID
// ---------------------------------------------------------------------------

solana_pubkey::declare_id!("Governance1111111111111111111111111111111111");

// ---------------------------------------------------------------------------
// Entrypoint
// ---------------------------------------------------------------------------

declare_process_instruction!(Entrypoint, DEFAULT_COMPUTE_UNITS, |invoke_context| {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();

    let instruction: GovernanceInstruction =
        limited_deserialize(instruction_data, solana_packet::PACKET_DATA_SIZE as u64)?;

    trace!("governance process_instruction: {instruction:?}");

    match instruction {
        GovernanceInstruction::InitializeGovernance {
            authority,
            proposal_threshold,
            voting_period_epochs,
            quorum_bps,
            pass_threshold_bps,
            veto_threshold_bps,
            timelock_epochs,
            emergency_multisig,
        } => process_initialize_governance(
            invoke_context,
            authority,
            proposal_threshold,
            voting_period_epochs,
            quorum_bps,
            pass_threshold_bps,
            veto_threshold_bps,
            timelock_epochs,
            emergency_multisig,
        ),
        GovernanceInstruction::CreateProposal {
            title,
            description_hash,
            proposal_type,
        } => process_create_proposal(invoke_context, title, description_hash, proposal_type),
        GovernanceInstruction::CastVote { proposal_id, vote } => {
            process_cast_vote(invoke_context, proposal_id, vote)
        }
        GovernanceInstruction::ExecuteProposal { proposal_id } => {
            process_execute_proposal(invoke_context, proposal_id)
        }
        GovernanceInstruction::CancelProposal { proposal_id } => {
            process_cancel_proposal(invoke_context, proposal_id)
        }
        GovernanceInstruction::VetoProposal { proposal_id } => {
            process_veto_proposal(invoke_context, proposal_id)
        }
        GovernanceInstruction::ActivateGovernance => {
            process_activate_governance(invoke_context)
        }
        GovernanceInstruction::UpdateConfig {
            proposal_threshold,
            voting_period_epochs,
            quorum_bps,
            pass_threshold_bps,
            veto_threshold_bps,
            timelock_epochs,
            emergency_multisig,
        } => process_update_config(
            invoke_context,
            proposal_threshold,
            voting_period_epochs,
            quorum_bps,
            pass_threshold_bps,
            veto_threshold_bps,
            timelock_epochs,
            emergency_multisig,
        ),
    }
});

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Load and deserialise the `GovernanceConfig` from instruction account at `index`.
fn load_governance_config(
    invoke_context: &InvokeContext,
    account_index: u16,
) -> Result<GovernanceConfig, InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let account = instruction_context.try_borrow_instruction_account(account_index)?;

    if account.get_owner() != &id() {
        return Err(GovernanceError::InvalidAccountOwner.into());
    }
    let data = account.get_data().to_vec();
    GovernanceConfig::deserialize(&data).map_err(|_| GovernanceError::NotInitialized.into())
}

/// Load and deserialise a `Proposal` from instruction account at `index`.
fn load_proposal(
    invoke_context: &InvokeContext,
    account_index: u16,
) -> Result<Proposal, InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let account = instruction_context.try_borrow_instruction_account(account_index)?;

    if account.get_owner() != &id() {
        return Err(GovernanceError::InvalidAccountOwner.into());
    }
    let data = account.get_data().to_vec();
    Proposal::deserialize(&data).map_err(|_| GovernanceError::InvalidAccountData.into())
}

/// Save a `GovernanceConfig` back to instruction account at `index`.
fn save_governance_config(
    invoke_context: &InvokeContext,
    account_index: u16,
    config: &GovernanceConfig,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let mut account = instruction_context.try_borrow_instruction_account(account_index)?;

    let mut data = account.get_data().to_vec();
    if data.len() < GovernanceConfig::SERIALIZED_SIZE {
        data.resize(GovernanceConfig::SERIALIZED_SIZE, 0);
    }
    config
        .serialize_into(&mut data)
        .map_err(|_| GovernanceError::InvalidAccountData)?;
    account.set_data_from_slice(&data)
}

/// Save a `Proposal` back to instruction account at `index`.
fn save_proposal(
    invoke_context: &InvokeContext,
    account_index: u16,
    proposal: &Proposal,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let mut account = instruction_context.try_borrow_instruction_account(account_index)?;

    let mut data = account.get_data().to_vec();
    if data.len() < Proposal::SERIALIZED_SIZE {
        data.resize(Proposal::SERIALIZED_SIZE, 0);
    }
    proposal
        .serialize_into(&mut data)
        .map_err(|_| GovernanceError::InvalidAccountData)?;
    account.set_data_from_slice(&data)
}

/// Determine the effective pass threshold for a proposal type.
/// EmergencyUnlock uses 80% supermajority; everything else uses the config default.
fn effective_pass_threshold(proposal: &Proposal, config: &GovernanceConfig) -> u16 {
    if proposal.is_emergency_unlock() {
        EMERGENCY_UNLOCK_PASS_THRESHOLD_BPS
    } else {
        config.pass_threshold_bps
    }
}

// ---------------------------------------------------------------------------
// Instruction handlers
// ---------------------------------------------------------------------------

/// `InitializeGovernance`
///
/// Accounts:
///   0. `[signer, writable]` — Initialiser.
///   1. `[writable]`         — Governance config account.
#[allow(clippy::too_many_arguments)]
fn process_initialize_governance(
    invoke_context: &InvokeContext,
    authority: Pubkey,
    proposal_threshold: u64,
    voting_period_epochs: u64,
    quorum_bps: u16,
    pass_threshold_bps: u16,
    veto_threshold_bps: u16,
    timelock_epochs: u64,
    emergency_multisig: Pubkey,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(2)?;

    // Initialiser must sign.
    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(GovernanceError::MissingAuthoritySignature.into());
    }

    // Validate config values.
    if quorum_bps == 0 || quorum_bps > 10_000 {
        return Err(GovernanceError::InvalidConfigValue.into());
    }
    if pass_threshold_bps == 0 || pass_threshold_bps > 10_000 {
        return Err(GovernanceError::InvalidConfigValue.into());
    }
    if veto_threshold_bps == 0 || veto_threshold_bps > 10_000 {
        return Err(GovernanceError::InvalidConfigValue.into());
    }
    if voting_period_epochs == 0 {
        return Err(GovernanceError::InvalidConfigValue.into());
    }

    // Verify config account is owned by this program and uninitialised.
    {
        let config_account = instruction_context.try_borrow_instruction_account(1)?;
        if config_account.get_owner() != &id() {
            ic_msg!(
                invoke_context,
                "InitializeGovernance: config account not owned by governance program"
            );
            return Err(GovernanceError::InvalidAccountOwner.into());
        }
        let data = config_account.get_data();
        if !data.is_empty() && data[0] == GOVERNANCE_CONFIG_DISCRIMINATOR {
            ic_msg!(
                invoke_context,
                "InitializeGovernance: config account already initialised"
            );
            return Err(GovernanceError::AlreadyInitialized.into());
        }
    }

    let config = GovernanceConfig {
        is_active: false, // governance is DISABLED at launch
        authority,
        proposal_threshold,
        voting_period_epochs,
        quorum_bps,
        pass_threshold_bps,
        veto_threshold_bps,
        timelock_epochs,
        emergency_multisig,
        next_proposal_id: 0,
    };

    save_governance_config(invoke_context, 1, &config)?;

    ic_msg!(
        invoke_context,
        "InitializeGovernance: authority={}, governance disabled at launch",
        authority
    );
    Ok(())
}

/// `CreateProposal`
///
/// Accounts:
///   0. `[signer]`           — Proposer (or authority if inactive).
///   1. `[writable]`         — Governance config account.
///   2. `[writable]`         — Proposal account (pre-allocated, uninitialised).
///   3. `[]`                 — Proposer's passive stake account (weight proof).
fn process_create_proposal(
    invoke_context: &InvokeContext,
    title_vec: Vec<u8>,
    description_hash: Hash,
    proposal_type: ProposalType,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(3)?;

    // Proposer must sign.
    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(GovernanceError::MissingAuthoritySignature.into());
    }
    let proposer = *instruction_context.get_key_of_instruction_account(0)?;

    // Convert title Vec<u8> to fixed [u8; 64], zero-padded.
    if title_vec.len() > crate::constants::MAX_TITLE_LEN {
        return Err(GovernanceError::TitleTooLong.into());
    }
    let mut title = [0u8; 64];
    title[..title_vec.len()].copy_from_slice(&title_vec);

    // Load config.
    let mut config = load_governance_config(invoke_context, 1)?;

    if config.is_active {
        // Governance is active — verify proposer has enough staked tokens.
        instruction_context.check_number_of_instruction_accounts(4)?;

        let stake_account = instruction_context.try_borrow_instruction_account(3)?;
        let stake_data = stake_account.get_data().to_vec();
        drop(stake_account);

        let (_amount, voting_power) = voting_power_from_passive_stake_data(&stake_data)
            .ok_or(GovernanceError::InsufficientStakeForProposal)?;

        if voting_power < config.proposal_threshold {
            ic_msg!(
                invoke_context,
                "CreateProposal: voting power {} < threshold {}",
                voting_power,
                config.proposal_threshold
            );
            return Err(GovernanceError::InsufficientStakeForProposal.into());
        }
    } else {
        // Governance is inactive — only the authority (multisig) can create proposals.
        if proposer != config.authority {
            ic_msg!(
                invoke_context,
                "CreateProposal: governance inactive, only authority can create proposals"
            );
            return Err(GovernanceError::MultisigOnly.into());
        }
    }

    // Verify proposal account is owned by this program and uninitialised.
    {
        let proposal_account = instruction_context.try_borrow_instruction_account(2)?;
        if proposal_account.get_owner() != &id() {
            return Err(GovernanceError::InvalidAccountOwner.into());
        }
        let data = proposal_account.get_data();
        if !data.is_empty() && data[0] == PROPOSAL_DISCRIMINATOR {
            ic_msg!(invoke_context, "CreateProposal: proposal account already initialised");
            return Err(GovernanceError::AlreadyInitialized.into());
        }
    }

    let clock = invoke_context.get_sysvar_cache().get_clock()?;
    let current_epoch = clock.epoch;

    // Assign proposal ID and increment counter.
    let proposal_id = config.next_proposal_id;
    config.next_proposal_id = config
        .next_proposal_id
        .checked_add(1)
        .ok_or(GovernanceError::ArithmeticOverflow)?;

    let voting_ends_epoch = current_epoch
        .checked_add(config.voting_period_epochs)
        .ok_or(GovernanceError::ArithmeticOverflow)?;

    let execution_epoch = voting_ends_epoch
        .checked_add(config.timelock_epochs)
        .ok_or(GovernanceError::ArithmeticOverflow)?;

    // When governance is inactive, proposals go straight to Timelocked
    // (the authority has implicitly "passed" it).
    let initial_status = if config.is_active {
        ProposalStatus::Active
    } else {
        ProposalStatus::Timelocked
    };

    let proposal = Proposal {
        id: proposal_id,
        proposer,
        title,
        description_hash,
        proposal_type,
        status: initial_status,
        created_epoch: current_epoch,
        voting_ends_epoch,
        execution_epoch,
        votes_for: 0,
        votes_against: 0,
        votes_abstain: 0,
        veto_votes: 0,
        executed: false,
    };

    // Save both.
    save_governance_config(invoke_context, 1, &config)?;
    save_proposal(invoke_context, 2, &proposal)?;

    ic_msg!(
        invoke_context,
        "CreateProposal: id={}, proposer={}, status={:?}",
        proposal_id,
        proposer,
        initial_status
    );
    Ok(())
}

/// `CastVote`
///
/// Accounts:
///   0. `[signer]`           — Voter.
///   1. `[writable]`         — Proposal account.
///   2. `[]`                 — Governance config account.
///   3. `[]`                 — Voter's passive stake account (weight proof).
///   4. `[writable]`         — Vote record account (created on first vote).
fn process_cast_vote(
    invoke_context: &InvokeContext,
    proposal_id: u64,
    vote: Vote,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(5)?;

    // Voter must sign.
    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(GovernanceError::MissingAuthoritySignature.into());
    }
    let voter = *instruction_context.get_key_of_instruction_account(0)?;

    // Load config — governance must be active.
    let config = load_governance_config(invoke_context, 2)?;
    if !config.is_active {
        ic_msg!(invoke_context, "CastVote: governance is not active");
        return Err(GovernanceError::GovernanceNotActive.into());
    }

    // Load proposal.
    let mut proposal = load_proposal(invoke_context, 1)?;

    // Verify proposal ID matches.
    if proposal.id != proposal_id {
        ic_msg!(
            invoke_context,
            "CastVote: proposal id mismatch (expected {}, got {})",
            proposal_id,
            proposal.id
        );
        return Err(GovernanceError::InvalidAccountData.into());
    }

    // Proposal must be Active.
    if proposal.status != ProposalStatus::Active {
        return Err(GovernanceError::InvalidProposalStatus.into());
    }

    // Must be within voting period.
    let clock = invoke_context.get_sysvar_cache().get_clock()?;
    if clock.epoch >= proposal.voting_ends_epoch {
        return Err(GovernanceError::VotingPeriodEnded.into());
    }

    // Check vote record account — must not already exist (no double voting).
    {
        let vote_record_account = instruction_context.try_borrow_instruction_account(4)?;
        if vote_record_account.get_owner() != &id() {
            return Err(GovernanceError::InvalidAccountOwner.into());
        }
        let data = vote_record_account.get_data();
        if !data.is_empty() && data[0] == VOTE_RECORD_DISCRIMINATOR {
            ic_msg!(invoke_context, "CastVote: voter has already voted on this proposal");
            return Err(GovernanceError::AlreadyVoted.into());
        }
    }

    // Read voter's passive stake account to determine voting power.
    let voting_power = {
        let stake_account = instruction_context.try_borrow_instruction_account(3)?;
        let stake_data = stake_account.get_data().to_vec();
        drop(stake_account);

        let (_amount, power) = voting_power_from_passive_stake_data(&stake_data)
            .ok_or(GovernanceError::NoVotingPower)?;

        if power == 0 {
            return Err(GovernanceError::NoVotingPower.into());
        }
        power
    };

    // Apply vote.
    match vote {
        Vote::For => {
            proposal.votes_for = proposal
                .votes_for
                .checked_add(voting_power)
                .ok_or(GovernanceError::ArithmeticOverflow)?;
        }
        Vote::Against => {
            proposal.votes_against = proposal
                .votes_against
                .checked_add(voting_power)
                .ok_or(GovernanceError::ArithmeticOverflow)?;
        }
        Vote::Abstain => {
            proposal.votes_abstain = proposal
                .votes_abstain
                .checked_add(voting_power)
                .ok_or(GovernanceError::ArithmeticOverflow)?;
        }
        Vote::Veto => {
            proposal.veto_votes = proposal
                .veto_votes
                .checked_add(voting_power)
                .ok_or(GovernanceError::ArithmeticOverflow)?;
        }
    }

    // Save updated proposal.
    save_proposal(invoke_context, 1, &proposal)?;

    // Write vote record to prevent double-voting.
    let vote_record = VoteRecord {
        proposal_id,
        voter,
        vote,
        weight: voting_power,
        voted_epoch: clock.epoch,
    };

    {
        let mut vote_record_account = instruction_context.try_borrow_instruction_account(4)?;
        let mut data = vote_record_account.get_data().to_vec();
        if data.len() < VoteRecord::SERIALIZED_SIZE {
            data.resize(VoteRecord::SERIALIZED_SIZE, 0);
        }
        vote_record
            .serialize_into(&mut data)
            .map_err(|_| GovernanceError::InvalidAccountData)?;
        vote_record_account.set_data_from_slice(&data)?;
    }

    ic_msg!(
        invoke_context,
        "CastVote: voter={}, proposal={}, vote={:?}, weight={}",
        voter,
        proposal_id,
        vote,
        voting_power
    );
    Ok(())
}

/// `ExecuteProposal`
///
/// Accounts:
///   0. `[signer]`           — Executor (anyone if active, authority if inactive).
///   1. `[writable]`         — Proposal account.
///   2. `[writable]`         — Governance config account.
fn process_execute_proposal(
    invoke_context: &InvokeContext,
    proposal_id: u64,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(3)?;

    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(GovernanceError::MissingAuthoritySignature.into());
    }
    let executor = *instruction_context.get_key_of_instruction_account(0)?;

    let config = load_governance_config(invoke_context, 2)?;
    let mut proposal = load_proposal(invoke_context, 1)?;

    if proposal.id != proposal_id {
        return Err(GovernanceError::InvalidAccountData.into());
    }

    // When governance is inactive, only the authority can execute.
    if !config.is_active && executor != config.authority {
        ic_msg!(
            invoke_context,
            "ExecuteProposal: governance inactive, only authority can execute"
        );
        return Err(GovernanceError::MultisigOnly.into());
    }

    if proposal.executed {
        return Err(GovernanceError::ProposalAlreadyExecuted.into());
    }

    let clock = invoke_context.get_sysvar_cache().get_clock()?;

    // Handle state transitions for active governance.
    if config.is_active {
        match proposal.status {
            ProposalStatus::Active => {
                // Voting period must have ended.
                if clock.epoch < proposal.voting_ends_epoch {
                    return Err(GovernanceError::VotingPeriodNotEnded.into());
                }

                // Check if veto threshold was reached.
                let total_votes = proposal
                    .votes_for
                    .checked_add(proposal.votes_against)
                    .ok_or(GovernanceError::ArithmeticOverflow)?
                    .checked_add(proposal.votes_abstain)
                    .ok_or(GovernanceError::ArithmeticOverflow)?
                    .checked_add(proposal.veto_votes)
                    .ok_or(GovernanceError::ArithmeticOverflow)?;

                if total_votes == 0 {
                    proposal.status = ProposalStatus::Expired;
                    save_proposal(invoke_context, 1, &proposal)?;
                    ic_msg!(invoke_context, "ExecuteProposal: proposal expired (no votes)");
                    return Err(GovernanceError::ProposalExpired.into());
                }

                // Check veto: veto_votes / total_votes >= veto_threshold_bps / 10_000
                let veto_pct = (proposal.veto_votes as u128)
                    .checked_mul(BPS_DENOMINATOR as u128)
                    .ok_or(GovernanceError::ArithmeticOverflow)?
                    .checked_div(total_votes as u128)
                    .ok_or(GovernanceError::ArithmeticOverflow)?;

                if veto_pct >= config.veto_threshold_bps as u128 {
                    proposal.status = ProposalStatus::Vetoed;
                    save_proposal(invoke_context, 1, &proposal)?;
                    ic_msg!(invoke_context, "ExecuteProposal: proposal vetoed");
                    return Err(GovernanceError::VetoThresholdReached.into());
                }

                // Check pass threshold: votes_for / (votes_for + votes_against) >= pass_threshold
                let decisive_votes = proposal
                    .votes_for
                    .checked_add(proposal.votes_against)
                    .ok_or(GovernanceError::ArithmeticOverflow)?;

                let pass_threshold = effective_pass_threshold(&proposal, &config);

                if decisive_votes == 0 {
                    proposal.status = ProposalStatus::Rejected;
                    save_proposal(invoke_context, 1, &proposal)?;
                    return Err(GovernanceError::PassThresholdNotMet.into());
                }

                let for_pct = (proposal.votes_for as u128)
                    .checked_mul(BPS_DENOMINATOR as u128)
                    .ok_or(GovernanceError::ArithmeticOverflow)?
                    .checked_div(decisive_votes as u128)
                    .ok_or(GovernanceError::ArithmeticOverflow)?;

                if for_pct < pass_threshold as u128 {
                    proposal.status = ProposalStatus::Rejected;
                    save_proposal(invoke_context, 1, &proposal)?;
                    ic_msg!(invoke_context, "ExecuteProposal: pass threshold not met");
                    return Err(GovernanceError::PassThresholdNotMet.into());
                }

                // Proposal passes — move to Timelocked.
                proposal.status = ProposalStatus::Timelocked;
                save_proposal(invoke_context, 1, &proposal)?;
                ic_msg!(
                    invoke_context,
                    "ExecuteProposal: proposal {} passed, now timelocked until epoch {}",
                    proposal_id,
                    proposal.execution_epoch
                );
                return Ok(());
            }
            ProposalStatus::Passed | ProposalStatus::Timelocked => {
                // Check timelock.
                if clock.epoch < proposal.execution_epoch {
                    return Err(GovernanceError::TimelockNotExpired.into());
                }
                // Fall through to execution below.
            }
            _ => {
                return Err(GovernanceError::InvalidProposalStatus.into());
            }
        }
    } else {
        // Governance inactive — proposal must be Timelocked (created by authority).
        if proposal.status != ProposalStatus::Timelocked {
            return Err(GovernanceError::InvalidProposalStatus.into());
        }
        if clock.epoch < proposal.execution_epoch {
            return Err(GovernanceError::TimelockNotExpired.into());
        }
    }

    // === Execute the proposal ===
    //
    // Note: Actual execution of ParameterChange, TreasurySpend, ProgramUpgrade,
    // FeatureToggle, and EmergencyUnlock would require cross-program invocations
    // (CPI) to the respective programs.  In this initial implementation we mark
    // the proposal as executed and log the action.  The CPI plumbing is added
    // when those target programs are integrated.
    //
    // TextProposal has no on-chain effect.

    proposal.status = ProposalStatus::Executed;
    proposal.executed = true;
    save_proposal(invoke_context, 1, &proposal)?;

    match &proposal.proposal_type {
        ProposalType::ParameterChange { param_id, new_value } => {
            ic_msg!(
                invoke_context,
                "ExecuteProposal: ParameterChange param_id={} new_value={}",
                param_id,
                new_value
            );
        }
        ProposalType::TreasurySpend {
            recipient,
            amount,
            memo: _,
        } => {
            ic_msg!(
                invoke_context,
                "ExecuteProposal: TreasurySpend {} lamports to {}",
                amount,
                recipient
            );
        }
        ProposalType::EmergencyUnlock { target_account } => {
            ic_msg!(
                invoke_context,
                "ExecuteProposal: EmergencyUnlock target={}",
                target_account
            );
        }
        ProposalType::ProgramUpgrade {
            program_id,
            buffer_account,
        } => {
            ic_msg!(
                invoke_context,
                "ExecuteProposal: ProgramUpgrade program={} buffer={}",
                program_id,
                buffer_account
            );
        }
        ProposalType::FeatureToggle {
            feature_id,
            enabled,
        } => {
            ic_msg!(
                invoke_context,
                "ExecuteProposal: FeatureToggle feature_id={} enabled={}",
                feature_id,
                enabled
            );
        }
        ProposalType::TextProposal => {
            ic_msg!(
                invoke_context,
                "ExecuteProposal: TextProposal (signaling only)"
            );
        }
    }

    ic_msg!(
        invoke_context,
        "ExecuteProposal: proposal {} executed by {}",
        proposal_id,
        executor
    );
    Ok(())
}

/// `CancelProposal`
///
/// Accounts:
///   0. `[signer]`           — Emergency multisig.
///   1. `[writable]`         — Proposal account.
///   2. `[]`                 — Governance config account.
fn process_cancel_proposal(
    invoke_context: &InvokeContext,
    proposal_id: u64,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(3)?;

    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(GovernanceError::MissingAuthoritySignature.into());
    }
    let signer = *instruction_context.get_key_of_instruction_account(0)?;

    let config = load_governance_config(invoke_context, 2)?;

    // Only the emergency multisig can cancel.
    if signer != config.emergency_multisig {
        ic_msg!(
            invoke_context,
            "CancelProposal: signer {} is not the emergency multisig {}",
            signer,
            config.emergency_multisig
        );
        return Err(GovernanceError::NotEmergencyMultisig.into());
    }

    let mut proposal = load_proposal(invoke_context, 1)?;

    if proposal.id != proposal_id {
        return Err(GovernanceError::InvalidAccountData.into());
    }

    // Can cancel proposals that are Active, Passed, or Timelocked.
    match proposal.status {
        ProposalStatus::Active | ProposalStatus::Passed | ProposalStatus::Timelocked => {}
        _ => {
            ic_msg!(
                invoke_context,
                "CancelProposal: cannot cancel proposal in status {:?}",
                proposal.status
            );
            return Err(GovernanceError::InvalidProposalStatus.into());
        }
    }

    proposal.status = ProposalStatus::Cancelled;
    save_proposal(invoke_context, 1, &proposal)?;

    ic_msg!(
        invoke_context,
        "CancelProposal: proposal {} cancelled by emergency multisig {}",
        proposal_id,
        signer
    );
    Ok(())
}

/// `VetoProposal`
///
/// Accounts:
///   0. `[signer]`           — Caller (anyone can trigger veto check).
///   1. `[writable]`         — Proposal account.
///   2. `[]`                 — Governance config account.
fn process_veto_proposal(
    invoke_context: &InvokeContext,
    proposal_id: u64,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(3)?;

    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(GovernanceError::MissingAuthoritySignature.into());
    }

    let config = load_governance_config(invoke_context, 2)?;
    let mut proposal = load_proposal(invoke_context, 1)?;

    if proposal.id != proposal_id {
        return Err(GovernanceError::InvalidAccountData.into());
    }

    // Can only veto Active or Timelocked proposals.
    match proposal.status {
        ProposalStatus::Active | ProposalStatus::Timelocked => {}
        _ => {
            return Err(GovernanceError::InvalidProposalStatus.into());
        }
    }

    // Check if veto threshold is reached.
    let total_votes = proposal
        .votes_for
        .checked_add(proposal.votes_against)
        .ok_or(GovernanceError::ArithmeticOverflow)?
        .checked_add(proposal.votes_abstain)
        .ok_or(GovernanceError::ArithmeticOverflow)?
        .checked_add(proposal.veto_votes)
        .ok_or(GovernanceError::ArithmeticOverflow)?;

    if total_votes == 0 {
        return Err(GovernanceError::VetoThresholdReached.into());
    }

    let veto_pct = (proposal.veto_votes as u128)
        .checked_mul(BPS_DENOMINATOR as u128)
        .ok_or(GovernanceError::ArithmeticOverflow)?
        .checked_div(total_votes as u128)
        .ok_or(GovernanceError::ArithmeticOverflow)?;

    if veto_pct < config.veto_threshold_bps as u128 {
        ic_msg!(
            invoke_context,
            "VetoProposal: veto threshold not reached ({}bps < {}bps)",
            veto_pct,
            config.veto_threshold_bps
        );
        return Err(GovernanceError::InvalidProposalStatus.into());
    }

    proposal.status = ProposalStatus::Vetoed;
    save_proposal(invoke_context, 1, &proposal)?;

    ic_msg!(
        invoke_context,
        "VetoProposal: proposal {} vetoed (veto votes = {})",
        proposal_id,
        proposal.veto_votes
    );
    Ok(())
}

/// `ActivateGovernance`
///
/// One-way transition from multisig-only mode to full governance.
///
/// Accounts:
///   0. `[signer]`           — Current authority (multisig).
///   1. `[writable]`         — Governance config account.
fn process_activate_governance(
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(2)?;

    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(GovernanceError::MissingAuthoritySignature.into());
    }
    let signer = *instruction_context.get_key_of_instruction_account(0)?;

    let mut config = load_governance_config(invoke_context, 1)?;

    if signer != config.authority {
        ic_msg!(invoke_context, "ActivateGovernance: authority mismatch");
        return Err(GovernanceError::AuthorityMismatch.into());
    }

    if config.is_active {
        return Err(GovernanceError::GovernanceAlreadyActive.into());
    }

    config.is_active = true;
    save_governance_config(invoke_context, 1, &config)?;

    ic_msg!(
        invoke_context,
        "ActivateGovernance: governance activated by {}",
        signer
    );
    Ok(())
}

/// `UpdateConfig`
///
/// Accounts:
///   0. `[signer]`           — Current authority.
///   1. `[writable]`         — Governance config account.
#[allow(clippy::too_many_arguments)]
fn process_update_config(
    invoke_context: &InvokeContext,
    proposal_threshold: u64,
    voting_period_epochs: u64,
    quorum_bps: u16,
    pass_threshold_bps: u16,
    veto_threshold_bps: u16,
    timelock_epochs: u64,
    emergency_multisig: Pubkey,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(2)?;

    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(GovernanceError::MissingAuthoritySignature.into());
    }
    let signer = *instruction_context.get_key_of_instruction_account(0)?;

    let mut config = load_governance_config(invoke_context, 1)?;

    if signer != config.authority {
        ic_msg!(invoke_context, "UpdateConfig: authority mismatch");
        return Err(GovernanceError::AuthorityMismatch.into());
    }

    // Validate new config values.
    if quorum_bps == 0 || quorum_bps > 10_000 {
        return Err(GovernanceError::InvalidConfigValue.into());
    }
    if pass_threshold_bps == 0 || pass_threshold_bps > 10_000 {
        return Err(GovernanceError::InvalidConfigValue.into());
    }
    if veto_threshold_bps == 0 || veto_threshold_bps > 10_000 {
        return Err(GovernanceError::InvalidConfigValue.into());
    }
    if voting_period_epochs == 0 {
        return Err(GovernanceError::InvalidConfigValue.into());
    }

    // Apply updates.
    config.proposal_threshold = proposal_threshold;
    config.voting_period_epochs = voting_period_epochs;
    config.quorum_bps = quorum_bps;
    config.pass_threshold_bps = pass_threshold_bps;
    config.veto_threshold_bps = veto_threshold_bps;
    config.timelock_epochs = timelock_epochs;
    config.emergency_multisig = emergency_multisig;

    save_governance_config(invoke_context, 1, &config)?;

    ic_msg!(
        invoke_context,
        "UpdateConfig: updated by {} — quorum={}bps pass={}bps veto={}bps timelock={}ep",
        signer,
        quorum_bps,
        pass_threshold_bps,
        veto_threshold_bps,
        timelock_epochs
    );
    Ok(())
}

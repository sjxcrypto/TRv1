//! Instruction processing for the TRv1 Developer Rewards program.

#![allow(clippy::arithmetic_side_effects)]

use {
    crate::{
        constants::{
            COOLDOWN_SLOTS, MAX_PROGRAM_FEE_SHARE_BPS,
            MIN_COMPUTE_UNITS_THRESHOLD, TOTAL_BPS,
        },
        error::DeveloperRewardsError,
        instruction::DeveloperRewardsInstruction,
        state::{EpochFeeTracker, ProgramRevenueConfig, RevenueSplit},
    },
    borsh::BorshDeserialize,
    solana_instruction::error::InstructionError,
    solana_program_runtime::{
        declare_process_instruction, invoke_context::InvokeContext,
    },
    solana_pubkey::Pubkey,
    solana_svm_log_collector::ic_msg,
};

/// Default compute-unit budget for developer-rewards instructions.
pub const DEFAULT_COMPUTE_UNITS: u64 = 2_000;

// ─────────────────────────────────────────────────────────────────────────────
// Program ID
// ─────────────────────────────────────────────────────────────────────────────

solana_pubkey::declare_id!("DevRew11111111111111111111111111111111111111");

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

declare_process_instruction!(Entrypoint, DEFAULT_COMPUTE_UNITS, |invoke_context| {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();

    let instruction = DeveloperRewardsInstruction::try_from_slice(instruction_data)
        .map_err(|_| InstructionError::InvalidInstructionData)?;

    match instruction {
        DeveloperRewardsInstruction::RegisterRevenueRecipient {
            program_id,
            recipient,
        } => process_register(invoke_context, &program_id, &recipient),

        DeveloperRewardsInstruction::UpdateRevenueRecipient {
            program_id,
            new_recipient,
        } => process_update_recipient(invoke_context, &program_id, &new_recipient),

        DeveloperRewardsInstruction::AddRevenueSplit {
            program_id,
            splits,
        } => process_add_revenue_split(invoke_context, &program_id, &splits),

        DeveloperRewardsInstruction::ClaimDeveloperFees { program_id } => {
            process_claim(invoke_context, &program_id)
        }

        DeveloperRewardsInstruction::CreditDeveloperFees {
            program_id,
            amount,
            compute_units_consumed,
        } => process_credit(invoke_context, &program_id, amount, compute_units_consumed),
    }
});

// ─────────────────────────────────────────────────────────────────────────────
// RegisterRevenueRecipient
// ─────────────────────────────────────────────────────────────────────────────

fn process_register(
    invoke_context: &InvokeContext,
    program_id: &Pubkey,
    recipient: &Pubkey,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    // Account 0: signer (upgrade authority)
    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(InstructionError::MissingRequiredSignature);
    }
    let signer_key = *instruction_context.get_key_of_instruction_account(0)?;

    // Account 1: ProgramRevenueConfig PDA (writable)
    {
        let config_account = instruction_context.try_borrow_instruction_account(1)?;
        let existing_data = config_account.get_data();
        if !existing_data.is_empty() && existing_data[0] != 0 {
            return Err(DeveloperRewardsError::ConfigAlreadyExists.into());
        }
    }

    // Account 2: programdata account — verify upgrade authority matches signer
    {
        let programdata_account = instruction_context.try_borrow_instruction_account(2)?;
        let pd_data = programdata_account.get_data();
        if pd_data.len() < 45 {
            return Err(DeveloperRewardsError::UnauthorizedSigner.into());
        }
        if pd_data[12] != 1 {
            return Err(DeveloperRewardsError::UnauthorizedSigner.into());
        }
        let upgrade_authority = Pubkey::new_from_array(
            pd_data[13..45]
                .try_into()
                .map_err(|_| InstructionError::InvalidAccountData)?,
        );
        if upgrade_authority != signer_key {
            return Err(DeveloperRewardsError::UnauthorizedSigner.into());
        }
    }

    // Get current slot from Clock sysvar
    let clock = invoke_context.get_sysvar_cache().get_clock()?;
    let current_slot = clock.slot;

    // Build the config.
    let config = ProgramRevenueConfig {
        version: 1,
        program_id: *program_id,
        revenue_recipient: *recipient,
        update_authority: signer_key,
        is_active: true,
        revenue_splits: Vec::new(),
        total_fees_earned: 0,
        epoch_fees_earned: 0,
        last_epoch: 0,
        eligible_after_slot: current_slot.saturating_add(COOLDOWN_SLOTS),
        unclaimed_fees: 0,
    };

    // Serialize into the account.
    let serialized = borsh::to_vec(&config)
        .map_err(|_| InstructionError::InvalidAccountData)?;
    let mut config_account = instruction_context.try_borrow_instruction_account(1)?;
    let data_len = config_account.get_data().len();
    if data_len < serialized.len() {
        return Err(DeveloperRewardsError::AccountDataTooSmall.into());
    }
    config_account.set_data_from_slice(&serialized)?;

    ic_msg!(
        invoke_context,
        "Registered revenue config for program {} → recipient {}",
        program_id,
        recipient
    );

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// UpdateRevenueRecipient
// ─────────────────────────────────────────────────────────────────────────────

fn process_update_recipient(
    invoke_context: &InvokeContext,
    _program_id: &Pubkey,
    new_recipient: &Pubkey,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    // Account 0: signer (must be update authority)
    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(InstructionError::MissingRequiredSignature);
    }
    let signer_key = *instruction_context.get_key_of_instruction_account(0)?;

    // Account 1: ProgramRevenueConfig PDA (writable)
    let mut config_account = instruction_context.try_borrow_instruction_account(1)?;
    let mut config = deserialize_config(&config_account)?;

    if config.update_authority != signer_key {
        return Err(DeveloperRewardsError::UnauthorizedUpdateAuthority.into());
    }

    config.revenue_recipient = *new_recipient;
    serialize_config(&config, &mut config_account)?;

    ic_msg!(
        invoke_context,
        "Updated revenue recipient for program {} → {}",
        config.program_id,
        new_recipient
    );

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// AddRevenueSplit
// ─────────────────────────────────────────────────────────────────────────────

fn process_add_revenue_split(
    invoke_context: &InvokeContext,
    _program_id: &Pubkey,
    splits: &[RevenueSplit],
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    // Validate split vector.
    if splits.len() > 10 {
        return Err(DeveloperRewardsError::TooManySplitRecipients.into());
    }

    let mut total_bps: u32 = 0;
    for (i, s) in splits.iter().enumerate() {
        if s.share_bps == 0 {
            return Err(DeveloperRewardsError::ZeroShareInSplit.into());
        }
        total_bps = total_bps.saturating_add(s.share_bps as u32);

        for other in splits.iter().skip(i + 1) {
            if s.recipient == other.recipient {
                return Err(DeveloperRewardsError::DuplicateRecipient.into());
            }
        }
    }
    if total_bps != TOTAL_BPS as u32 {
        return Err(DeveloperRewardsError::InvalidSplitTotal.into());
    }

    // Account 0: signer (must be update authority)
    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(InstructionError::MissingRequiredSignature);
    }
    let signer_key = *instruction_context.get_key_of_instruction_account(0)?;

    // Account 1: ProgramRevenueConfig PDA (writable)
    let mut config_account = instruction_context.try_borrow_instruction_account(1)?;
    let mut config = deserialize_config(&config_account)?;

    if config.update_authority != signer_key {
        return Err(DeveloperRewardsError::UnauthorizedUpdateAuthority.into());
    }

    config.revenue_splits = splits.to_vec();
    serialize_config(&config, &mut config_account)?;

    ic_msg!(
        invoke_context,
        "Updated revenue splits for program {} ({} recipients)",
        config.program_id,
        splits.len()
    );

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// ClaimDeveloperFees
// ─────────────────────────────────────────────────────────────────────────────

fn process_claim(
    invoke_context: &InvokeContext,
    _program_id: &Pubkey,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    // Account 0: signer (permissionless — funds go to registered recipient).
    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(InstructionError::MissingRequiredSignature);
    }

    // Account 1: ProgramRevenueConfig PDA (writable)
    let claim_amount;
    let config_program_id;
    {
        let mut config_account = instruction_context.try_borrow_instruction_account(1)?;
        let mut config = deserialize_config(&config_account)?;

        if !config.is_active {
            return Err(DeveloperRewardsError::ConfigNotActive.into());
        }
        if config.unclaimed_fees == 0 {
            return Err(DeveloperRewardsError::NoFeesToClaim.into());
        }

        claim_amount = config.unclaimed_fees;
        config_program_id = config.program_id;
        config.unclaimed_fees = 0;

        serialize_config(&config, &mut config_account)?;
    }

    // Account 2: Developer fee pool (writable) — source of funds.
    {
        let mut pool_account = instruction_context.try_borrow_instruction_account(2)?;
        if pool_account.get_lamports() < claim_amount {
            return Err(DeveloperRewardsError::InsufficientPoolFunds.into());
        }
        pool_account.checked_sub_lamports(claim_amount)?;
    }

    // Account 3+: recipient(s)
    {
        let mut recipient_account = instruction_context.try_borrow_instruction_account(3)?;
        recipient_account.checked_add_lamports(claim_amount)?;
    }

    ic_msg!(
        invoke_context,
        "Claimed {} lamports for program {}",
        claim_amount,
        config_program_id
    );

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// CreditDeveloperFees (runtime-invoked)
// ─────────────────────────────────────────────────────────────────────────────

fn process_credit(
    invoke_context: &InvokeContext,
    _program_id: &Pubkey,
    amount: u64,
    compute_units_consumed: u64,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    // Anti-gaming: minimum CU threshold
    if compute_units_consumed < MIN_COMPUTE_UNITS_THRESHOLD {
        return Err(DeveloperRewardsError::BelowMinComputeUnits.into());
    }

    // Get current slot/epoch from Clock sysvar
    let clock = invoke_context.get_sysvar_cache().get_clock()?;
    let current_slot = clock.slot;
    let current_epoch = clock.epoch;

    // Account 1: ProgramRevenueConfig PDA (writable)
    let mut config;
    {
        let config_account = instruction_context.try_borrow_instruction_account(1)?;
        config = deserialize_config(&config_account)?;
    }

    if !config.is_active {
        return Err(DeveloperRewardsError::ConfigNotActive.into());
    }

    // Anti-gaming: 7-day cooldown
    if current_slot < config.eligible_after_slot {
        return Err(DeveloperRewardsError::CooldownNotElapsed.into());
    }

    // Epoch roll-over
    if config.last_epoch != current_epoch {
        config.epoch_fees_earned = 0;
        config.last_epoch = current_epoch;
    }

    // Anti-gaming: per-epoch cap (10% of total dev fees)
    // We read the epoch tracker from account 2

    let mut tracker_account = instruction_context.try_borrow_instruction_account(2)?;
    let mut tracker = deserialize_tracker(&tracker_account)?;

    if tracker.epoch != current_epoch {
        tracker.epoch = current_epoch;
        tracker.total_developer_fees = 0;
    }

    let projected_program = config.epoch_fees_earned.saturating_add(amount);
    let projected_total = tracker.total_developer_fees.saturating_add(amount);

    let max_allowed = (projected_total as u128)
        .saturating_mul(MAX_PROGRAM_FEE_SHARE_BPS as u128)
        .checked_div(TOTAL_BPS as u128)
        .unwrap_or(0) as u64;

    if projected_program > max_allowed {
        return Err(DeveloperRewardsError::EpochFeeCapExceeded.into());
    }

    // Credit the fees
    config.epoch_fees_earned = projected_program;
    config.total_fees_earned = config.total_fees_earned.saturating_add(amount);
    config.unclaimed_fees = config.unclaimed_fees.saturating_add(amount);
    tracker.total_developer_fees = projected_total;

    serialize_tracker(&tracker, &mut tracker_account)?;
    drop(tracker_account);

    let mut config_account = instruction_context.try_borrow_instruction_account(1)?;
    serialize_config(&config, &mut config_account)?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn deserialize_config(
    account: &solana_transaction_context::instruction_accounts::BorrowedInstructionAccount<'_, '_>,
) -> Result<ProgramRevenueConfig, InstructionError> {
    let data = account.get_data();
    if data.is_empty() || data[0] == 0 {
        return Err(DeveloperRewardsError::ConfigNotFound.into());
    }
    ProgramRevenueConfig::try_from_slice(data)
        .map_err(|_| InstructionError::InvalidAccountData)
}

fn serialize_config(
    config: &ProgramRevenueConfig,
    account: &mut solana_transaction_context::instruction_accounts::BorrowedInstructionAccount<'_, '_>,
) -> Result<(), InstructionError> {
    let serialized = borsh::to_vec(config)
        .map_err(|_| InstructionError::InvalidAccountData)?;
    let data_len = account.get_data().len();
    if data_len < serialized.len() {
        return Err(DeveloperRewardsError::AccountDataTooSmall.into());
    }
    account.set_data_from_slice(&serialized)?;
    Ok(())
}

fn deserialize_tracker(
    account: &solana_transaction_context::instruction_accounts::BorrowedInstructionAccount<'_, '_>,
) -> Result<EpochFeeTracker, InstructionError> {
    let data = account.get_data();
    if data.is_empty() || data.iter().all(|&b| b == 0) {
        return Ok(EpochFeeTracker {
            version: 1,
            ..Default::default()
        });
    }
    EpochFeeTracker::try_from_slice(data)
        .map_err(|_| InstructionError::InvalidAccountData)
}

fn serialize_tracker(
    tracker: &EpochFeeTracker,
    account: &mut solana_transaction_context::instruction_accounts::BorrowedInstructionAccount<'_, '_>,
) -> Result<(), InstructionError> {
    let serialized = borsh::to_vec(tracker)
        .map_err(|_| InstructionError::InvalidAccountData)?;
    let data_len = account.get_data().len();
    if data_len < serialized.len() {
        return Err(DeveloperRewardsError::AccountDataTooSmall.into());
    }
    account.set_data_from_slice(&serialized)?;
    Ok(())
}

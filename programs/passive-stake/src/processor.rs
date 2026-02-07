//! Instruction processing logic for the Passive Stake program.

use {
    crate::{
        constants::{
            early_unlock_penalty_bps_for_tier, is_valid_tier, reward_rate_bps_for_tier,
            vote_weight_bps_for_tier, BPS_DENOMINATOR, PERMANENT_LOCK_DAYS, SECONDS_PER_DAY,
            TIER_NO_LOCK,
        },
        error::PassiveStakeError,
        instruction::PassiveStakeInstruction,
        state::{PassiveStakeAccount, PASSIVE_STAKE_ACCOUNT_DISCRIMINATOR},
    },
    log::*,
    solana_bincode::limited_deserialize,
    solana_instruction::error::InstructionError,
    solana_program_runtime::{
        declare_process_instruction, invoke_context::InvokeContext,
    },
    solana_svm_log_collector::ic_msg,
};

/// Default compute-unit budget for passive-stake instructions.
pub const DEFAULT_COMPUTE_UNITS: u64 = 750;

// ---------------------------------------------------------------------------
// Program ID
// ---------------------------------------------------------------------------

// TRv1 passive-stake program id — a deterministic address derived from the
// program name.  In production this would be registered in solana-sdk-ids;
// for now we define it locally.
solana_pubkey::declare_id!("Pass1veStake1111111111111111111111111111111");

// ---------------------------------------------------------------------------
// Entrypoint
// ---------------------------------------------------------------------------

declare_process_instruction!(Entrypoint, DEFAULT_COMPUTE_UNITS, |invoke_context| {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();

    let instruction: PassiveStakeInstruction =
        limited_deserialize(instruction_data, solana_packet::PACKET_DATA_SIZE as u64)?;

    trace!("passive_stake process_instruction: {instruction:?}");

    match instruction {
        PassiveStakeInstruction::InitializePassiveStake { lock_days, amount } => {
            process_initialize_passive_stake(invoke_context, lock_days, amount)
        }
        PassiveStakeInstruction::ClaimRewards => process_claim_rewards(invoke_context),
        PassiveStakeInstruction::Unlock => process_unlock(invoke_context),
        PassiveStakeInstruction::EarlyUnlock => process_early_unlock(invoke_context),
        PassiveStakeInstruction::CalculateEpochRewards {
            current_epoch,
            validator_reward_rate,
        } => process_calculate_epoch_rewards(invoke_context, current_epoch, validator_reward_rate),
    }
});

// ---------------------------------------------------------------------------
// Instruction handlers
// ---------------------------------------------------------------------------

/// `InitializePassiveStake { lock_days, amount }`
///
/// Accounts:
///   0. `[signer, writable]` — Funding authority (source of lamports).
///   1. `[writable]`         — Passive stake account (pre-created, owned by this program).
fn process_initialize_passive_stake(
    invoke_context: &InvokeContext,
    lock_days: u64,
    amount: u64,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    // --- Validate tier ---
    if !is_valid_tier(lock_days) {
        ic_msg!(invoke_context, "InitializePassiveStake: invalid lock tier {lock_days}");
        return Err(PassiveStakeError::InvalidLockTier.into());
    }

    if amount == 0 {
        ic_msg!(invoke_context, "InitializePassiveStake: amount must be > 0");
        return Err(PassiveStakeError::ZeroStakeAmount.into());
    }

    // --- Check accounts ---
    instruction_context.check_number_of_instruction_accounts(2)?;

    // Authority must be a signer.
    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(PassiveStakeError::MissingAuthoritySignature.into());
    }

    let authority_pubkey = *instruction_context.get_key_of_instruction_account(0)?;

    // --- Verify stake account is owned by this program and uninitialised ---
    {
        let stake_account = instruction_context.try_borrow_instruction_account(1)?;
        if stake_account.get_owner() != &id() {
            ic_msg!(invoke_context, "InitializePassiveStake: stake account not owned by passive-stake program");
            return Err(PassiveStakeError::InvalidAccountOwner.into());
        }
        let data = stake_account.get_data();
        if !data.is_empty() && data[0] == PASSIVE_STAKE_ACCOUNT_DISCRIMINATOR {
            ic_msg!(invoke_context, "InitializePassiveStake: account already initialised");
            return Err(PassiveStakeError::AccountAlreadyInitialized.into());
        }
    }

    // --- Transfer lamports from authority to stake account ---
    {
        let mut authority_account = instruction_context.try_borrow_instruction_account(0)?;
        if authority_account.get_lamports() < amount {
            ic_msg!(invoke_context, "InitializePassiveStake: insufficient lamports");
            return Err(PassiveStakeError::InsufficientLamports.into());
        }
        authority_account.checked_sub_lamports(amount)?;
    }
    {
        let mut stake_account = instruction_context.try_borrow_instruction_account(1)?;
        stake_account.checked_add_lamports(amount)?;
    }

    // --- Read clock for timestamps ---
    let clock = invoke_context.get_sysvar_cache().get_clock()?;
    let now = clock.unix_timestamp;
    let current_epoch = clock.epoch;

    // --- Compute lock end ---
    let (lock_end, is_permanent) = if lock_days == PERMANENT_LOCK_DAYS {
        (0i64, true)
    } else if lock_days == TIER_NO_LOCK {
        (0i64, false)
    } else {
        let duration_secs = (lock_days as i64)
            .checked_mul(SECONDS_PER_DAY)
            .ok_or(PassiveStakeError::ArithmeticOverflow)?;
        let end = now
            .checked_add(duration_secs)
            .ok_or(PassiveStakeError::ArithmeticOverflow)?;
        (end, false)
    };

    let vote_weight = vote_weight_bps_for_tier(lock_days)
        .ok_or(PassiveStakeError::InvalidLockTier)?;

    // --- Write state ---
    let state = PassiveStakeAccount {
        authority: authority_pubkey,
        amount,
        lock_days,
        lock_start: now,
        lock_end,
        unclaimed_rewards: 0,
        last_reward_epoch: current_epoch,
        is_permanent,
        vote_weight_bps: vote_weight,
    };

    let mut stake_account = instruction_context.try_borrow_instruction_account(1)?;
    let mut data = stake_account.get_data().to_vec();
    if data.len() < PassiveStakeAccount::SERIALIZED_SIZE {
        data.resize(PassiveStakeAccount::SERIALIZED_SIZE, 0);
    }
    state
        .serialize_into(&mut data)
        .map_err(|_| PassiveStakeError::InvalidAccountData)?;
    stake_account.set_data_from_slice(&data)?;

    ic_msg!(
        invoke_context,
        "InitializePassiveStake: {} lamports locked for {} days by {}",
        amount,
        lock_days,
        authority_pubkey
    );
    Ok(())
}

/// `ClaimRewards`
///
/// Accounts:
///   0. `[signer, writable]` — Authority (reward recipient).
///   1. `[writable]`         — Passive stake account.
///   2. `[writable]`         — Rewards pool account (lamport source).
fn process_claim_rewards(invoke_context: &InvokeContext) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(3)?;

    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(PassiveStakeError::MissingAuthoritySignature.into());
    }

    let authority_pubkey = *instruction_context.get_key_of_instruction_account(0)?;

    // --- Load & validate stake account ---
    let rewards_to_claim;
    {
        let mut stake_account = instruction_context.try_borrow_instruction_account(1)?;
        if stake_account.get_owner() != &id() {
            return Err(PassiveStakeError::InvalidAccountOwner.into());
        }

        let data = stake_account.get_data().to_vec();
        let mut state = PassiveStakeAccount::deserialize(&data)
            .map_err(|_| PassiveStakeError::InvalidAccountData)?;

        if state.authority != authority_pubkey {
            ic_msg!(invoke_context, "ClaimRewards: authority mismatch");
            return Err(PassiveStakeError::MissingAuthoritySignature.into());
        }

        if state.unclaimed_rewards == 0 {
            return Err(PassiveStakeError::NoRewardsToClaim.into());
        }

        rewards_to_claim = state.unclaimed_rewards;
        state.unclaimed_rewards = 0;

        let mut buf = stake_account.get_data().to_vec();
        state
            .serialize_into(&mut buf)
            .map_err(|_| PassiveStakeError::InvalidAccountData)?;
        stake_account.set_data_from_slice(&buf)?;
    }

    // --- Transfer rewards from pool to authority ---
    {
        let mut pool_account = instruction_context.try_borrow_instruction_account(2)?;
        if pool_account.get_lamports() < rewards_to_claim {
            ic_msg!(invoke_context, "ClaimRewards: reward pool has insufficient lamports");
            return Err(PassiveStakeError::InsufficientLamports.into());
        }
        pool_account.checked_sub_lamports(rewards_to_claim)?;
    }
    {
        let mut authority_account = instruction_context.try_borrow_instruction_account(0)?;
        authority_account.checked_add_lamports(rewards_to_claim)?;
    }

    ic_msg!(
        invoke_context,
        "ClaimRewards: {} lamports claimed by {}",
        rewards_to_claim,
        authority_pubkey
    );
    Ok(())
}

/// `Unlock`
///
/// Accounts:
///   0. `[signer, writable]` — Authority (receives principal).
///   1. `[writable]`         — Passive stake account.
fn process_unlock(invoke_context: &InvokeContext) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(2)?;

    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(PassiveStakeError::MissingAuthoritySignature.into());
    }

    let authority_pubkey = *instruction_context.get_key_of_instruction_account(0)?;

    let principal;
    {
        let mut stake_account = instruction_context.try_borrow_instruction_account(1)?;
        if stake_account.get_owner() != &id() {
            return Err(PassiveStakeError::InvalidAccountOwner.into());
        }

        let data = stake_account.get_data().to_vec();
        let state = PassiveStakeAccount::deserialize(&data)
            .map_err(|_| PassiveStakeError::InvalidAccountData)?;

        if state.authority != authority_pubkey {
            return Err(PassiveStakeError::MissingAuthoritySignature.into());
        }

        if state.is_permanent {
            ic_msg!(invoke_context, "Unlock: permanent locks cannot be unlocked");
            return Err(PassiveStakeError::EarlyUnlockNotAllowed.into());
        }

        // No-lock accounts can always withdraw.
        if state.lock_days != TIER_NO_LOCK {
            let clock = invoke_context.get_sysvar_cache().get_clock()?;
            if clock.unix_timestamp < state.lock_end {
                ic_msg!(
                    invoke_context,
                    "Unlock: lock expires at {}, current time is {}",
                    state.lock_end,
                    clock.unix_timestamp
                );
                return Err(PassiveStakeError::LockNotExpired.into());
            }
        }

        principal = state.amount;

        // Zero out the account data (mark as closed).
        let zeroed = vec![0u8; stake_account.get_data().len()];
        stake_account.set_data_from_slice(&zeroed)?;
        // Move all lamports (principal + any remaining rent) to authority.
        let remaining = stake_account.get_lamports();
        stake_account.checked_sub_lamports(remaining)?;
    }

    {
        let mut authority_account = instruction_context.try_borrow_instruction_account(0)?;
        authority_account.checked_add_lamports(principal)?;
    }

    ic_msg!(
        invoke_context,
        "Unlock: {} lamports returned to {}",
        principal,
        authority_pubkey
    );
    Ok(())
}

/// `EarlyUnlock`
///
/// Accounts:
///   0. `[signer, writable]` — Authority (receives principal minus penalty).
///   1. `[writable]`         — Passive stake account.
fn process_early_unlock(invoke_context: &InvokeContext) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(2)?;

    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(PassiveStakeError::MissingAuthoritySignature.into());
    }

    let authority_pubkey = *instruction_context.get_key_of_instruction_account(0)?;

    let return_amount;
    let penalty;
    {
        let mut stake_account = instruction_context.try_borrow_instruction_account(1)?;
        if stake_account.get_owner() != &id() {
            return Err(PassiveStakeError::InvalidAccountOwner.into());
        }

        let data = stake_account.get_data().to_vec();
        let state = PassiveStakeAccount::deserialize(&data)
            .map_err(|_| PassiveStakeError::InvalidAccountData)?;

        if state.authority != authority_pubkey {
            return Err(PassiveStakeError::MissingAuthoritySignature.into());
        }

        if state.is_permanent {
            ic_msg!(invoke_context, "EarlyUnlock: permanent locks cannot be unlocked");
            return Err(PassiveStakeError::EarlyUnlockNotAllowed.into());
        }

        let penalty_bps = early_unlock_penalty_bps_for_tier(state.lock_days)
            .ok_or(PassiveStakeError::InvalidLockTier)?;

        // penalty = amount * penalty_bps / 10_000
        penalty = state
            .amount
            .checked_mul(penalty_bps)
            .ok_or(PassiveStakeError::ArithmeticOverflow)?
            .checked_div(BPS_DENOMINATOR)
            .ok_or(PassiveStakeError::ArithmeticOverflow)?;

        return_amount = state
            .amount
            .checked_sub(penalty)
            .ok_or(PassiveStakeError::ArithmeticOverflow)?;

        // Zero out account data (close the position).
        let zeroed = vec![0u8; stake_account.get_data().len()];
        stake_account.set_data_from_slice(&zeroed)?;

        // Remove all lamports from the stake account.
        // The penalty portion is effectively burned (removed from circulation).
        let total_lamports = stake_account.get_lamports();
        stake_account.checked_sub_lamports(total_lamports)?;
    }

    // Credit only the post-penalty amount back to the authority.
    // The penalty lamports are destroyed (burned) — they do not go anywhere.
    {
        let mut authority_account = instruction_context.try_borrow_instruction_account(0)?;
        authority_account.checked_add_lamports(return_amount)?;
    }

    ic_msg!(
        invoke_context,
        "EarlyUnlock: {} lamports returned, {} lamports burned as penalty for {}",
        return_amount,
        penalty,
        authority_pubkey
    );
    Ok(())
}

/// `CalculateEpochRewards { current_epoch, validator_reward_rate }`
///
/// Accounts:
///   0. `[writable]` — Passive stake account.
fn process_calculate_epoch_rewards(
    invoke_context: &InvokeContext,
    current_epoch: u64,
    validator_reward_rate: u64,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(1)?;

    let mut stake_account = instruction_context.try_borrow_instruction_account(0)?;
    if stake_account.get_owner() != &id() {
        return Err(PassiveStakeError::InvalidAccountOwner.into());
    }

    let data = stake_account.get_data().to_vec();
    let mut state = PassiveStakeAccount::deserialize(&data)
        .map_err(|_| PassiveStakeError::InvalidAccountData)?;

    // Only advance if we haven't already processed this epoch.
    if current_epoch <= state.last_reward_epoch {
        ic_msg!(
            invoke_context,
            "CalculateEpochRewards: already processed epoch {}",
            current_epoch
        );
        return Ok(());
    }

    // Number of epochs that have elapsed since the last reward calculation.
    let epochs_elapsed = current_epoch
        .checked_sub(state.last_reward_epoch)
        .ok_or(PassiveStakeError::ArithmeticOverflow)?;

    // Look up the tier's reward-rate multiplier (bps of the validator rate).
    let tier_rate_bps = reward_rate_bps_for_tier(state.lock_days)
        .ok_or(PassiveStakeError::InvalidLockTier)?;

    // reward_per_epoch = amount * validator_reward_rate * tier_rate_bps / (BPS² * epochs_per_year)
    //
    // We simplify by assuming ~365.25 epochs/year (one epoch per day on mainnet).
    // For more accuracy the runtime can pass an adjusted validator_reward_rate
    // that already accounts for epoch length.
    //
    // Per-epoch reward ≈ amount × (validator_rate / 10_000) × (tier_rate / 10_000) / 365
    //
    // To avoid precision loss we compute with u128 intermediates.
    let amount = state.amount as u128;
    let v_rate = validator_reward_rate as u128;
    let t_rate = tier_rate_bps as u128;
    let denom = (BPS_DENOMINATOR as u128)
        .checked_mul(BPS_DENOMINATOR as u128)
        .ok_or(PassiveStakeError::ArithmeticOverflow)?
        .checked_mul(365)
        .ok_or(PassiveStakeError::ArithmeticOverflow)?;

    let reward_per_epoch = amount
        .checked_mul(v_rate)
        .ok_or(PassiveStakeError::ArithmeticOverflow)?
        .checked_mul(t_rate)
        .ok_or(PassiveStakeError::ArithmeticOverflow)?
        .checked_div(denom)
        .ok_or(PassiveStakeError::ArithmeticOverflow)?;

    let total_new_rewards = reward_per_epoch
        .checked_mul(epochs_elapsed as u128)
        .ok_or(PassiveStakeError::ArithmeticOverflow)?;

    // Saturate to u64.
    let total_new_rewards_u64: u64 = total_new_rewards
        .try_into()
        .unwrap_or(u64::MAX);

    state.unclaimed_rewards = state
        .unclaimed_rewards
        .checked_add(total_new_rewards_u64)
        .ok_or(PassiveStakeError::ArithmeticOverflow)?;

    state.last_reward_epoch = current_epoch;

    let mut buf = stake_account.get_data().to_vec();
    state
        .serialize_into(&mut buf)
        .map_err(|_| PassiveStakeError::InvalidAccountData)?;
    stake_account.set_data_from_slice(&buf)?;

    ic_msg!(
        invoke_context,
        "CalculateEpochRewards: {} new reward lamports for {} epochs (tier {}d)",
        total_new_rewards_u64,
        epochs_elapsed,
        state.lock_days
    );
    Ok(())
}

//! Instruction processing logic for the Treasury program.

use {
    crate::{
        error::TreasuryError,
        instruction::TreasuryInstruction,
        state::{TreasuryConfig, TREASURY_CONFIG_DISCRIMINATOR},
    },
    log::*,
    solana_bincode::limited_deserialize,
    solana_instruction::error::InstructionError,
    solana_program_runtime::{declare_process_instruction, invoke_context::InvokeContext},
    solana_pubkey::Pubkey,
    solana_svm_log_collector::ic_msg,
};

/// Maximum memo length in bytes.
pub const MAX_MEMO_LEN: usize = 256;

/// Default compute-unit budget for treasury instructions.
pub const DEFAULT_COMPUTE_UNITS: u64 = 750;

// ---------------------------------------------------------------------------
// Program ID
// ---------------------------------------------------------------------------

// TRv1 treasury program id — a deterministic address derived from the
// program name.  In production this would be registered in solana-sdk-ids;
// for now we define it locally.
solana_pubkey::declare_id!("Treasury11111111111111111111111111111111111");

// ---------------------------------------------------------------------------
// Entrypoint
// ---------------------------------------------------------------------------

declare_process_instruction!(Entrypoint, DEFAULT_COMPUTE_UNITS, |invoke_context| {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();

    let instruction: TreasuryInstruction =
        limited_deserialize(instruction_data, solana_packet::PACKET_DATA_SIZE as u64)?;

    trace!("treasury process_instruction: {instruction:?}");

    match instruction {
        TreasuryInstruction::InitializeTreasury {
            authority,
            treasury_account,
        } => process_initialize_treasury(invoke_context, authority, treasury_account),
        TreasuryInstruction::Disburse {
            amount,
            recipient,
            memo,
        } => process_disburse(invoke_context, amount, recipient, memo),
        TreasuryInstruction::UpdateAuthority { new_authority } => {
            process_update_authority(invoke_context, new_authority)
        }
        TreasuryInstruction::ActivateGovernance => process_activate_governance(invoke_context),
    }
});

// ---------------------------------------------------------------------------
// Instruction handlers
// ---------------------------------------------------------------------------

/// `InitializeTreasury { authority, treasury_account }`
///
/// Accounts:
///   0. `[signer, writable]` — Initialiser.
///   1. `[writable]`         — Treasury config account (pre-allocated, uninitialised).
fn process_initialize_treasury(
    invoke_context: &InvokeContext,
    authority: Pubkey,
    treasury_account: Pubkey,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(2)?;

    // Initialiser must sign.
    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(TreasuryError::MissingAuthoritySignature.into());
    }

    // --- Verify config account is owned by this program and uninitialised ---
    {
        let config_account = instruction_context.try_borrow_instruction_account(1)?;
        if config_account.get_owner() != &id() {
            ic_msg!(
                invoke_context,
                "InitializeTreasury: config account not owned by treasury program"
            );
            return Err(TreasuryError::InvalidAccountOwner.into());
        }
        let data = config_account.get_data();
        if !data.is_empty() && data[0] == TREASURY_CONFIG_DISCRIMINATOR {
            ic_msg!(
                invoke_context,
                "InitializeTreasury: config account already initialised"
            );
            return Err(TreasuryError::AlreadyInitialized.into());
        }
    }

    // --- Read clock for epoch tracking ---
    let clock = invoke_context.get_sysvar_cache().get_clock()?;

    // --- Write initial state ---
    let config = TreasuryConfig {
        authority,
        treasury_account,
        governance_active: false,
        total_received: 0,
        total_disbursed: 0,
        last_updated_epoch: clock.epoch,
    };

    let mut config_account = instruction_context.try_borrow_instruction_account(1)?;
    let mut data = config_account.get_data().to_vec();
    if data.len() < TreasuryConfig::SERIALIZED_SIZE {
        data.resize(TreasuryConfig::SERIALIZED_SIZE, 0);
    }
    config
        .serialize_into(&mut data)
        .map_err(|_| TreasuryError::InvalidAccountData)?;
    config_account.set_data_from_slice(&data)?;

    ic_msg!(
        invoke_context,
        "InitializeTreasury: authority={}, treasury_account={}",
        authority,
        treasury_account
    );
    Ok(())
}

/// `Disburse { amount, recipient, memo }`
///
/// Accounts:
///   0. `[signer]`   — Authority.
///   1. `[writable]`  — Treasury config account.
///   2. `[writable]`  — Treasury token account (source of lamports).
///   3. `[writable]`  — Recipient account.
fn process_disburse(
    invoke_context: &InvokeContext,
    amount: u64,
    recipient: Pubkey,
    memo: String,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(4)?;

    // --- Validate inputs ---
    if amount == 0 {
        return Err(TreasuryError::ZeroDisbursement.into());
    }
    if memo.len() > MAX_MEMO_LEN {
        return Err(TreasuryError::MemoTooLong.into());
    }

    // --- Authority must sign ---
    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(TreasuryError::MissingAuthoritySignature.into());
    }
    let signer_pubkey = *instruction_context.get_key_of_instruction_account(0)?;

    // --- Load & validate config ---
    let clock = invoke_context.get_sysvar_cache().get_clock()?;
    {
        let mut config_account = instruction_context.try_borrow_instruction_account(1)?;
        if config_account.get_owner() != &id() {
            return Err(TreasuryError::InvalidAccountOwner.into());
        }

        let data = config_account.get_data().to_vec();
        let mut config = TreasuryConfig::deserialize(&data)
            .map_err(|_| TreasuryError::NotInitialized)?;

        if config.authority != signer_pubkey {
            ic_msg!(invoke_context, "Disburse: authority mismatch");
            return Err(TreasuryError::AuthorityMismatch.into());
        }

        // --- Validate recipient account matches instruction data ---
        let recipient_key = *instruction_context.get_key_of_instruction_account(3)?;
        if recipient_key != recipient {
            ic_msg!(invoke_context, "Disburse: recipient mismatch");
            return Err(TreasuryError::RecipientMismatch.into());
        }

        // --- Update tracking ---
        config.total_disbursed = config
            .total_disbursed
            .checked_add(amount)
            .ok_or(TreasuryError::ArithmeticOverflow)?;
        config.last_updated_epoch = clock.epoch;

        let mut buf = config_account.get_data().to_vec();
        config
            .serialize_into(&mut buf)
            .map_err(|_| TreasuryError::InvalidAccountData)?;
        config_account.set_data_from_slice(&buf)?;
    }

    // --- Transfer lamports from treasury account to recipient ---
    {
        let mut treasury_account = instruction_context.try_borrow_instruction_account(2)?;
        if treasury_account.get_lamports() < amount {
            ic_msg!(
                invoke_context,
                "Disburse: insufficient funds ({} < {})",
                treasury_account.get_lamports(),
                amount
            );
            return Err(TreasuryError::InsufficientFunds.into());
        }
        treasury_account.checked_sub_lamports(amount)?;
    }
    {
        let mut recipient_account = instruction_context.try_borrow_instruction_account(3)?;
        recipient_account.checked_add_lamports(amount)?;
    }

    ic_msg!(
        invoke_context,
        "Disburse: {} lamports to {} — memo: {}",
        amount,
        recipient,
        memo
    );
    Ok(())
}

/// `UpdateAuthority { new_authority }`
///
/// Accounts:
///   0. `[signer]`   — Current authority.
///   1. `[writable]`  — Treasury config account.
fn process_update_authority(
    invoke_context: &InvokeContext,
    new_authority: Pubkey,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(2)?;

    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(TreasuryError::MissingAuthoritySignature.into());
    }
    let signer_pubkey = *instruction_context.get_key_of_instruction_account(0)?;

    let clock = invoke_context.get_sysvar_cache().get_clock()?;

    let mut config_account = instruction_context.try_borrow_instruction_account(1)?;
    if config_account.get_owner() != &id() {
        return Err(TreasuryError::InvalidAccountOwner.into());
    }

    let data = config_account.get_data().to_vec();
    let mut config =
        TreasuryConfig::deserialize(&data).map_err(|_| TreasuryError::NotInitialized)?;

    if config.authority != signer_pubkey {
        ic_msg!(invoke_context, "UpdateAuthority: authority mismatch");
        return Err(TreasuryError::AuthorityMismatch.into());
    }

    let old_authority = config.authority;
    config.authority = new_authority;
    config.last_updated_epoch = clock.epoch;

    let mut buf = config_account.get_data().to_vec();
    config
        .serialize_into(&mut buf)
        .map_err(|_| TreasuryError::InvalidAccountData)?;
    config_account.set_data_from_slice(&buf)?;

    ic_msg!(
        invoke_context,
        "UpdateAuthority: {} → {}",
        old_authority,
        new_authority
    );
    Ok(())
}

/// `ActivateGovernance`
///
/// Accounts:
///   0. `[signer]`   — Current authority.
///   1. `[writable]`  — Treasury config account.
fn process_activate_governance(
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;

    instruction_context.check_number_of_instruction_accounts(2)?;

    if !instruction_context.is_instruction_account_signer(0)? {
        return Err(TreasuryError::MissingAuthoritySignature.into());
    }
    let signer_pubkey = *instruction_context.get_key_of_instruction_account(0)?;

    let clock = invoke_context.get_sysvar_cache().get_clock()?;

    let mut config_account = instruction_context.try_borrow_instruction_account(1)?;
    if config_account.get_owner() != &id() {
        return Err(TreasuryError::InvalidAccountOwner.into());
    }

    let data = config_account.get_data().to_vec();
    let mut config =
        TreasuryConfig::deserialize(&data).map_err(|_| TreasuryError::NotInitialized)?;

    if config.authority != signer_pubkey {
        ic_msg!(invoke_context, "ActivateGovernance: authority mismatch");
        return Err(TreasuryError::AuthorityMismatch.into());
    }

    if config.governance_active {
        ic_msg!(invoke_context, "ActivateGovernance: governance is already active");
        return Err(TreasuryError::GovernanceAlreadyActive.into());
    }

    config.governance_active = true;
    config.last_updated_epoch = clock.epoch;

    let mut buf = config_account.get_data().to_vec();
    config
        .serialize_into(&mut buf)
        .map_err(|_| TreasuryError::InvalidAccountData)?;
    config_account.set_data_from_slice(&buf)?;

    ic_msg!(
        invoke_context,
        "ActivateGovernance: governance activated by {}",
        signer_pubkey
    );
    Ok(())
}

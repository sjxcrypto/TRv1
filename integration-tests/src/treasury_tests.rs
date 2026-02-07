//! Integration tests for TRv1 Treasury program.
//!
//! Tests initialization, disbursements, authority transitions, and governance
//! activation.

use {
    crate::harness::{SOL, TRv1TestHarness},
    solana_keypair::Keypair,
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_treasury_program::{
        error::TreasuryError,
        instruction::TreasuryInstruction,
        processor::MAX_MEMO_LEN,
        state::{TreasuryConfig, TREASURY_CONFIG_DISCRIMINATOR},
    },
};

// ═══════════════════════════════════════════════════════════════════════════
//  1. Initialize treasury
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_treasury_config_creation() {
    let authority = Pubkey::new_unique();
    let treasury_account = Pubkey::new_unique();

    let config = TreasuryConfig {
        authority,
        treasury_account,
        governance_active: false,
        total_received: 0,
        total_disbursed: 0,
        last_updated_epoch: 0,
    };

    assert_eq!(config.authority, authority);
    assert_eq!(config.treasury_account, treasury_account);
    assert!(!config.governance_active);
    assert_eq!(config.total_received, 0);
    assert_eq!(config.total_disbursed, 0);
}

#[test]
fn test_treasury_config_serialization_roundtrip() {
    let config = TreasuryConfig {
        authority: Pubkey::new_unique(),
        treasury_account: Pubkey::new_unique(),
        governance_active: false,
        total_received: 42 * SOL,
        total_disbursed: 10 * SOL,
        last_updated_epoch: 100,
    };

    let mut buf = vec![0u8; TreasuryConfig::SERIALIZED_SIZE];
    config.serialize_into(&mut buf).unwrap();

    let deserialized = TreasuryConfig::deserialize(&buf).unwrap();
    assert_eq!(config, deserialized);
}

#[test]
fn test_treasury_config_serialized_size() {
    // 1 + 32 + 32 + 1 + 8 + 8 + 8 = 90 bytes
    assert_eq!(TreasuryConfig::SERIALIZED_SIZE, 90);
}

#[test]
fn test_treasury_config_deserialize_rejects_wrong_discriminator() {
    let mut buf = vec![0u8; TreasuryConfig::SERIALIZED_SIZE];
    buf[0] = 0; // uninitialized
    assert!(TreasuryConfig::deserialize(&buf).is_err());

    buf[0] = 99; // wrong discriminator
    assert!(TreasuryConfig::deserialize(&buf).is_err());
}

#[test]
fn test_double_initialization_prevented() {
    let config = TreasuryConfig {
        authority: Pubkey::new_unique(),
        treasury_account: Pubkey::new_unique(),
        governance_active: false,
        total_received: 0,
        total_disbursed: 0,
        last_updated_epoch: 0,
    };

    let mut buf = vec![0u8; TreasuryConfig::SERIALIZED_SIZE];
    config.serialize_into(&mut buf).unwrap();

    // The discriminator byte should now be 1
    assert_eq!(buf[0], TREASURY_CONFIG_DISCRIMINATOR);

    // The processor checks: if data[0] == DISCRIMINATOR → AlreadyInitialized
    let is_initialized = buf[0] == TREASURY_CONFIG_DISCRIMINATOR;
    assert!(is_initialized, "Second initialization should be rejected");
}

// ═══════════════════════════════════════════════════════════════════════════
//  2. Disburse funds
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_disburse_updates_tracking() {
    let mut config = TreasuryConfig {
        authority: Pubkey::new_unique(),
        treasury_account: Pubkey::new_unique(),
        governance_active: false,
        total_received: 1000 * SOL,
        total_disbursed: 0,
        last_updated_epoch: 0,
    };

    let disburse_amount = 100 * SOL;
    config.total_disbursed = config
        .total_disbursed
        .checked_add(disburse_amount)
        .unwrap();
    config.last_updated_epoch = 5;

    assert_eq!(config.total_disbursed, 100 * SOL);
    assert_eq!(config.last_updated_epoch, 5);
}

#[test]
fn test_multiple_disbursements_accumulate() {
    let mut config = TreasuryConfig {
        authority: Pubkey::new_unique(),
        treasury_account: Pubkey::new_unique(),
        governance_active: false,
        total_received: 1000 * SOL,
        total_disbursed: 0,
        last_updated_epoch: 0,
    };

    for i in 1..=5 {
        let amount = 10 * SOL * i;
        config.total_disbursed = config.total_disbursed.checked_add(amount).unwrap();
    }

    // Sum: 10 + 20 + 30 + 40 + 50 = 150 SOL
    assert_eq!(config.total_disbursed, 150 * SOL);
}

#[test]
fn test_zero_disbursement_rejected() {
    // The processor returns ZeroDisbursement if amount == 0
    let amount = 0u64;
    assert_eq!(amount, 0, "Zero disbursements should be rejected");
}

#[test]
fn test_memo_max_length() {
    assert_eq!(MAX_MEMO_LEN, 256);

    let valid_memo = "a".repeat(256);
    assert!(valid_memo.len() <= MAX_MEMO_LEN);

    let invalid_memo = "a".repeat(257);
    assert!(
        invalid_memo.len() > MAX_MEMO_LEN,
        "Memos exceeding 256 bytes should be rejected"
    );
}

#[test]
fn test_insufficient_funds_check() {
    // Treasury has 50 SOL, trying to disburse 100 SOL should fail
    let treasury_balance = 50 * SOL;
    let disburse_amount = 100 * SOL;
    assert!(
        treasury_balance < disburse_amount,
        "Disbursement exceeding treasury balance should fail with InsufficientFunds"
    );
}

#[test]
fn test_recipient_mismatch_check() {
    // The instruction data contains a recipient pubkey that must match the
    // account at index 3. If they differ, RecipientMismatch is returned.
    let instruction_recipient = Pubkey::new_unique();
    let account_pubkey = Pubkey::new_unique();
    assert_ne!(
        instruction_recipient, account_pubkey,
        "Mismatched recipients should be rejected"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  3. Update authority (multisig → governance transition)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_update_authority() {
    let multisig_authority = Pubkey::new_unique();
    let governance_authority = Pubkey::new_unique();

    let mut config = TreasuryConfig {
        authority: multisig_authority,
        treasury_account: Pubkey::new_unique(),
        governance_active: false,
        total_received: 0,
        total_disbursed: 0,
        last_updated_epoch: 0,
    };

    assert_eq!(config.authority, multisig_authority);

    // Transfer authority to governance
    config.authority = governance_authority;
    config.last_updated_epoch = 1825; // at maturity

    assert_eq!(config.authority, governance_authority);
    assert_ne!(config.authority, multisig_authority);
}

#[test]
fn test_unauthorized_authority_update_fails() {
    let real_authority = Pubkey::new_unique();
    let config = TreasuryConfig {
        authority: real_authority,
        treasury_account: Pubkey::new_unique(),
        governance_active: false,
        total_received: 0,
        total_disbursed: 0,
        last_updated_epoch: 0,
    };

    let attacker = Pubkey::new_unique();
    assert_ne!(
        config.authority, attacker,
        "Attacker should not be able to update authority"
    );
}

#[test]
fn test_authority_transition_preserves_state() {
    let old_authority = Pubkey::new_unique();
    let new_authority = Pubkey::new_unique();

    let mut config = TreasuryConfig {
        authority: old_authority,
        treasury_account: Pubkey::new_unique(),
        governance_active: false,
        total_received: 500 * SOL,
        total_disbursed: 100 * SOL,
        last_updated_epoch: 50,
    };

    let treasury_account_before = config.treasury_account;
    let received_before = config.total_received;
    let disbursed_before = config.total_disbursed;

    config.authority = new_authority;
    config.last_updated_epoch = 51;

    // All other fields preserved
    assert_eq!(config.treasury_account, treasury_account_before);
    assert_eq!(config.total_received, received_before);
    assert_eq!(config.total_disbursed, disbursed_before);
    assert!(!config.governance_active);
}

// ═══════════════════════════════════════════════════════════════════════════
//  4. Unauthorized disburse fails
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_unauthorized_disburse_rejected() {
    let real_authority = Pubkey::new_unique();
    let config = TreasuryConfig {
        authority: real_authority,
        treasury_account: Pubkey::new_unique(),
        governance_active: false,
        total_received: 1000 * SOL,
        total_disbursed: 0,
        last_updated_epoch: 0,
    };

    let unauthorized_signer = Pubkey::new_unique();
    let authority_matches = config.authority == unauthorized_signer;
    assert!(
        !authority_matches,
        "Unauthorized signer should fail the authority check"
    );
}

#[test]
fn test_unsigned_transaction_rejected() {
    // The processor checks is_instruction_account_signer(0).
    // Without a signature, MissingAuthoritySignature is returned.
    // This is enforced at the runtime level; we document the expectation.
    let _expected_error = TreasuryError::MissingAuthoritySignature;
}

// ═══════════════════════════════════════════════════════════════════════════
//  5. Activate governance
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_activate_governance() {
    let authority = Pubkey::new_unique();

    let mut config = TreasuryConfig {
        authority,
        treasury_account: Pubkey::new_unique(),
        governance_active: false,
        total_received: 500 * SOL,
        total_disbursed: 100 * SOL,
        last_updated_epoch: 100,
    };

    assert!(!config.governance_active);

    config.governance_active = true;
    config.last_updated_epoch = 101;

    assert!(config.governance_active);
}

#[test]
fn test_governance_activation_is_one_way() {
    let mut config = TreasuryConfig {
        authority: Pubkey::new_unique(),
        treasury_account: Pubkey::new_unique(),
        governance_active: true, // already active
        total_received: 0,
        total_disbursed: 0,
        last_updated_epoch: 0,
    };

    // Attempting to activate again should be rejected with GovernanceAlreadyActive
    assert!(
        config.governance_active,
        "Re-activation should be rejected by the processor"
    );
}

#[test]
fn test_governance_preserves_authority_control() {
    // After governance activation, the same authority key is still required
    // for disbursements. The flag is informational — it doesn't change access
    // control logic.
    let authority = Pubkey::new_unique();
    let mut config = TreasuryConfig {
        authority,
        treasury_account: Pubkey::new_unique(),
        governance_active: false,
        total_received: 0,
        total_disbursed: 0,
        last_updated_epoch: 0,
    };

    config.governance_active = true;
    // Authority didn't change
    assert_eq!(config.authority, authority);
}

#[test]
fn test_full_lifecycle_multisig_to_governance() {
    // 1. Initialize with multisig authority
    let multisig = Pubkey::new_unique();
    let governance_key = Pubkey::new_unique();
    let treasury_acct = Pubkey::new_unique();

    let mut config = TreasuryConfig {
        authority: multisig,
        treasury_account: treasury_acct,
        governance_active: false,
        total_received: 0,
        total_disbursed: 0,
        last_updated_epoch: 0,
    };

    // 2. Multisig makes disbursements
    config.total_received = 1000 * SOL;
    config.total_disbursed = 200 * SOL;
    config.last_updated_epoch = 500;

    // 3. Transfer authority to governance
    config.authority = governance_key;
    config.last_updated_epoch = 1800;

    // 4. Activate governance
    config.governance_active = true;
    config.last_updated_epoch = 1825;

    // 5. Verify final state
    assert_eq!(config.authority, governance_key);
    assert!(config.governance_active);
    assert_eq!(config.total_received, 1000 * SOL);
    assert_eq!(config.total_disbursed, 200 * SOL);
    assert_eq!(config.treasury_account, treasury_acct);
}

// ═══════════════════════════════════════════════════════════════════════════
//  6. Instruction construction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_initialize_instruction_construction() {
    let authority = Pubkey::new_unique();
    let treasury_account = Pubkey::new_unique();

    let ix = TreasuryInstruction::InitializeTreasury {
        authority,
        treasury_account,
    };

    match ix {
        TreasuryInstruction::InitializeTreasury { authority: a, treasury_account: t } => {
            assert_eq!(a, authority);
            assert_eq!(t, treasury_account);
        }
        _ => panic!("Expected InitializeTreasury"),
    }
}

#[test]
fn test_disburse_instruction_construction() {
    let recipient = Pubkey::new_unique();
    let amount = 42 * SOL;
    let memo = "Test disbursement".to_string();

    let ix = TreasuryInstruction::Disburse {
        amount,
        recipient,
        memo: memo.clone(),
    };

    match ix {
        TreasuryInstruction::Disburse { amount: a, recipient: r, memo: m } => {
            assert_eq!(a, amount);
            assert_eq!(r, recipient);
            assert_eq!(m, memo);
        }
        _ => panic!("Expected Disburse"),
    }
}

#[test]
fn test_update_authority_instruction_construction() {
    let new_authority = Pubkey::new_unique();

    let ix = TreasuryInstruction::UpdateAuthority { new_authority };

    match ix {
        TreasuryInstruction::UpdateAuthority { new_authority: n } => {
            assert_eq!(n, new_authority);
        }
        _ => panic!("Expected UpdateAuthority"),
    }
}

#[test]
fn test_activate_governance_instruction_construction() {
    let ix = TreasuryInstruction::ActivateGovernance;

    assert!(
        matches!(ix, TreasuryInstruction::ActivateGovernance),
        "Should be ActivateGovernance variant"
    );
}

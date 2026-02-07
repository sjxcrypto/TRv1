use {
    super::Bank,
    crate::{bank::CollectorFeeDetails, reward_info::RewardInfo, trv1_constants},
    log::debug,
    solana_account::{ReadableAccount, WritableAccount},
    solana_fee::FeeFeatures,
    solana_fee_structure::FeeBudgetLimits,
    solana_pubkey::Pubkey,
    solana_reward_info::RewardType,
    solana_runtime_transaction::transaction_with_meta::TransactionWithMeta,
    solana_svm::rent_calculator::{get_account_rent_state, transition_allowed},
    solana_system_interface::program as system_program,
    std::{result::Result, sync::atomic::Ordering::Relaxed},
    thiserror::Error,
};

#[derive(Error, Debug, PartialEq)]
enum DepositFeeError {
    #[error("fee account became rent paying")]
    InvalidRentPayingAccount,
    #[error("lamport overflow")]
    LamportOverflow,
    #[error("invalid fee account owner")]
    InvalidAccountOwner,
}

/// TRv1 fee distribution model with four-way split.
///
/// At launch: 10% burn / 0% validator / 45% treasury / 45% dApp developer
/// At maturity (epoch 1825+): 25% burn / 25% validator / 25% treasury / 25% dApp developer
/// The transition is linear over ~5 years of daily epochs.
///
/// For the dApp developer share: if the transaction invokes a program with a
/// known revenue recipient, the dev share goes to that address. For simple
/// transfers (no program), the dev share is added to burn instead.
#[derive(Default)]
pub struct FeeDistribution {
    /// Amount deposited to the validator (leader)
    deposit: u64,
    /// Amount burned
    burn: u64,
    /// Amount sent to treasury
    treasury: u64,
    /// Amount sent to dApp developer(s) — for now added to burn if no program
    dev: u64,
}

impl FeeDistribution {
    pub fn get_deposit(&self) -> u64 {
        self.deposit
    }

    pub fn get_treasury(&self) -> u64 {
        self.treasury
    }

    pub fn get_dev(&self) -> u64 {
        self.dev
    }
}

impl Bank {
    // TRv1: Distribute collected transaction fees using epoch-dependent four-way split.
    //
    // The fee distribution transitions linearly over FEE_TRANSITION_EPOCHS:
    //   Launch:   10% burn / 0% validator / 45% treasury / 45% dApp developer
    //   Maturity: 25% burn / 25% validator / 25% treasury / 25% dApp developer
    //
    // For now, treasury and dev shares that can't be delivered are added to burn.
    // TODO: Implement treasury account delivery and per-program revenue recipients.
    pub(super) fn distribute_transaction_fee_details(&self) {
        let fee_details = self.collector_fee_details.read().unwrap();
        if fee_details.total_transaction_fee() == 0 {
            // nothing to distribute, exit early
            return;
        }

        let FeeDistribution {
            deposit,
            burn,
            treasury,
            dev,
        } = self.calculate_reward_and_burn_fee_details(&fee_details);

        // For now, treasury and dev shares go to burn since we haven't
        // implemented the treasury account or per-program revenue recipients yet.
        // TODO: Send `treasury` to the configured treasury pubkey
        // TODO: Send `dev` to program revenue recipients
        let undeliverable = treasury.saturating_add(dev);
        let total_burn = self
            .deposit_or_burn_fee(deposit)
            .saturating_add(burn)
            .saturating_add(undeliverable);
        self.capitalization.fetch_sub(total_burn, Relaxed);
    }

    pub fn calculate_reward_for_transaction(
        &self,
        transaction: &impl TransactionWithMeta,
        fee_budget_limits: &FeeBudgetLimits,
    ) -> u64 {
        let (_last_hash, last_lamports_per_signature) =
            self.last_blockhash_and_lamports_per_signature();
        let fee_details = solana_fee::calculate_fee_details(
            transaction,
            last_lamports_per_signature == 0,
            self.fee_structure().lamports_per_signature,
            fee_budget_limits.prioritization_fee,
            FeeFeatures::from(self.feature_set.as_ref()),
        );
        let FeeDistribution {
            deposit: reward,
            burn: _,
            treasury: _,
            dev: _,
        } = self.calculate_reward_and_burn_fee_details(&CollectorFeeDetails::from(fee_details));
        reward
    }

    /// TRv1: Calculate four-way fee split based on current epoch.
    pub fn calculate_reward_and_burn_fee_details(
        &self,
        fee_details: &CollectorFeeDetails,
    ) -> FeeDistribution {
        if fee_details.transaction_fee == 0 {
            return FeeDistribution::default();
        }

        let total_fee = fee_details
            .transaction_fee
            .saturating_add(fee_details.priority_fee);
        let current_epoch = self.epoch();

        let (burn_pct, validator_pct, treasury_pct, dev_pct) =
            trv1_constants::fee_distribution_for_epoch(current_epoch);

        let burn = (total_fee as f64 * burn_pct) as u64;
        let deposit = (total_fee as f64 * validator_pct) as u64;
        let treasury = (total_fee as f64 * treasury_pct) as u64;
        let dev = (total_fee as f64 * dev_pct) as u64;

        // Any remainder from rounding goes to burn
        let distributed = burn
            .saturating_add(deposit)
            .saturating_add(treasury)
            .saturating_add(dev);
        let remainder_burn = total_fee.saturating_sub(distributed);

        FeeDistribution {
            deposit,
            burn: burn.saturating_add(remainder_burn),
            treasury,
            dev,
        }
    }

    /// Attempts to deposit the given `deposit` amount into the fee collector account.
    ///
    /// Returns the original `deposit` amount if the deposit failed and must be burned, otherwise 0.
    fn deposit_or_burn_fee(&self, deposit: u64) -> u64 {
        if deposit == 0 {
            return 0;
        }

        match self.deposit_fees(&self.leader_id, deposit) {
            Ok(post_balance) => {
                self.rewards.write().unwrap().push((
                    self.leader_id,
                    RewardInfo {
                        reward_type: RewardType::Fee,
                        lamports: deposit as i64,
                        post_balance,
                        commission_bps: None,
                    },
                ));
                0
            }
            Err(err) => {
                debug!(
                    "Burned {} lamport tx fee instead of sending to {} due to {}",
                    deposit, self.leader_id, err
                );
                datapoint_warn!(
                    "bank-burned_fee",
                    ("slot", self.slot(), i64),
                    ("num_lamports", deposit, i64),
                    ("error", err.to_string(), String),
                );
                deposit
            }
        }
    }

    // Deposits fees into a specified account and if successful, returns the new balance of that account
    fn deposit_fees(&self, pubkey: &Pubkey, fees: u64) -> Result<u64, DepositFeeError> {
        let mut account = self
            .get_account_with_fixed_root_no_cache(pubkey)
            .unwrap_or_default();

        if !system_program::check_id(account.owner()) {
            return Err(DepositFeeError::InvalidAccountOwner);
        }

        let recipient_pre_rent_state = get_account_rent_state(
            &self.rent_collector().rent,
            account.lamports(),
            account.data().len(),
        );
        let distribution = account.checked_add_lamports(fees);
        if distribution.is_err() {
            return Err(DepositFeeError::LamportOverflow);
        }

        let recipient_post_rent_state = get_account_rent_state(
            &self.rent_collector().rent,
            account.lamports(),
            account.data().len(),
        );
        let rent_state_transition_allowed =
            transition_allowed(&recipient_pre_rent_state, &recipient_post_rent_state);
        if !rent_state_transition_allowed {
            return Err(DepositFeeError::InvalidRentPayingAccount);
        }

        self.store_account(pubkey, &account);
        Ok(account.lamports())
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::genesis_utils::{create_genesis_config, create_genesis_config_with_leader},
        solana_account::AccountSharedData,
        solana_pubkey as pubkey,
        solana_rent::Rent,
        solana_signer::Signer,
        std::sync::RwLock,
    };

    #[test]
    fn test_deposit_or_burn_zero_fee() {
        let genesis = create_genesis_config(0);
        let bank = Bank::new_for_tests(&genesis.genesis_config);
        assert_eq!(bank.deposit_or_burn_fee(0), 0);
    }

    #[test]
    fn test_deposit_or_burn_fee() {
        #[derive(PartialEq)]
        enum Scenario {
            Normal,
            InvalidOwner,
        }

        struct TestCase {
            scenario: Scenario,
        }

        impl TestCase {
            fn new(scenario: Scenario) -> Self {
                Self { scenario }
            }
        }

        for test_case in [
            TestCase::new(Scenario::Normal),
            TestCase::new(Scenario::InvalidOwner),
        ] {
            let mut genesis = create_genesis_config(0);
            let rent = Rent::default();
            let min_rent_exempt_balance = rent.minimum_balance(0);
            genesis.genesis_config.rent = rent; // Ensure rent is non-zero, as genesis_utils sets Rent::free by default
            let bank = Bank::new_for_tests(&genesis.genesis_config);

            let deposit = 100;
            let mut burn = 100;

            match test_case.scenario {
                Scenario::InvalidOwner => {
                    // ensure that account owner is invalid and fee distribution will fail
                    let account =
                        AccountSharedData::new(min_rent_exempt_balance, 0, &Pubkey::new_unique());
                    bank.store_account(bank.leader_id(), &account);
                }
                Scenario::Normal => {
                    let account =
                        AccountSharedData::new(min_rent_exempt_balance, 0, &system_program::id());
                    bank.store_account(bank.leader_id(), &account);
                }
            }

            let initial_burn = burn;
            let initial_leader_id_balance = bank.get_balance(bank.leader_id());
            burn += bank.deposit_or_burn_fee(deposit);
            let new_leader_id_balance = bank.get_balance(bank.leader_id());

            if test_case.scenario == Scenario::InvalidOwner {
                assert_eq!(initial_leader_id_balance, new_leader_id_balance);
                assert_eq!(initial_burn + deposit, burn);
                let locked_rewards = bank.rewards.read().unwrap();
                assert!(
                    locked_rewards.is_empty(),
                    "There should be no rewards distributed"
                );
            } else {
                assert_eq!(initial_leader_id_balance + deposit, new_leader_id_balance);

                assert_eq!(initial_burn, burn);

                let locked_rewards = bank.rewards.read().unwrap();
                assert_eq!(
                    locked_rewards.len(),
                    1,
                    "There should be one reward distributed"
                );

                let reward_info = &locked_rewards[0];
                assert_eq!(
                    reward_info.1.lamports, deposit as i64,
                    "The reward amount should match the expected deposit"
                );
                assert_eq!(
                    reward_info.1.reward_type,
                    RewardType::Fee,
                    "The reward type should be Fee"
                );
            }
        }
    }

    #[test]
    fn test_deposit_fees() {
        let initial_balance = 1_000_000_000;
        let genesis = create_genesis_config(initial_balance);
        let bank = Bank::new_for_tests(&genesis.genesis_config);
        let pubkey = genesis.mint_keypair.pubkey();
        let deposit_amount = 500;

        assert_eq!(
            bank.deposit_fees(&pubkey, deposit_amount),
            Ok(initial_balance + deposit_amount),
            "New balance should be the sum of the initial balance and deposit amount"
        );
    }

    #[test]
    fn test_deposit_fees_with_overflow() {
        let initial_balance = u64::MAX;
        let genesis = create_genesis_config(initial_balance);
        let bank = Bank::new_for_tests(&genesis.genesis_config);
        let pubkey = genesis.mint_keypair.pubkey();
        let deposit_amount = 500;

        assert_eq!(
            bank.deposit_fees(&pubkey, deposit_amount),
            Err(DepositFeeError::LamportOverflow),
            "Expected an error due to lamport overflow"
        );
    }

    #[test]
    fn test_deposit_fees_invalid_account_owner() {
        let initial_balance = 1000;
        let genesis = create_genesis_config_with_leader(0, &pubkey::new_rand(), initial_balance);
        let bank = Bank::new_for_tests(&genesis.genesis_config);
        let pubkey = genesis.voting_keypair.pubkey();
        let deposit_amount = 500;

        assert_eq!(
            bank.deposit_fees(&pubkey, deposit_amount),
            Err(DepositFeeError::InvalidAccountOwner),
            "Expected an error due to invalid account owner"
        );
    }

    #[test]
    fn test_distribute_transaction_fee_details_normal() {
        let genesis = create_genesis_config(0);
        let mut bank = Bank::new_for_tests(&genesis.genesis_config);
        let transaction_fee = 100;
        let priority_fee = 200;
        bank.collector_fee_details = RwLock::new(CollectorFeeDetails {
            transaction_fee,
            priority_fee,
        });

        // TRv1: At epoch 0, validator gets 0%, so deposit should be 0
        // Treasury (45%) and dev (45%) currently go to burn since not yet implemented
        // Burn is 10% + treasury + dev + rounding remainder
        let total_fee = transaction_fee + priority_fee;
        let fee_dist = bank.calculate_reward_and_burn_fee_details(&CollectorFeeDetails {
            transaction_fee,
            priority_fee,
        });

        // At epoch 0: validator_pct = 0%, so deposit = 0
        assert_eq!(fee_dist.deposit, 0, "At epoch 0, validator gets 0%");

        let initial_capitalization = bank.capitalization();
        bank.distribute_transaction_fee_details();

        // Since deposit is 0, all fees should be subtracted from capitalization
        // (burn + undeliverable treasury + dev)
        assert_eq!(
            initial_capitalization - total_fee,
            bank.capitalization(),
            "All fees should be removed from capitalization"
        );
    }

    #[test]
    fn test_distribute_transaction_fee_details_zero() {
        let genesis = create_genesis_config(0);
        let bank = Bank::new_for_tests(&genesis.genesis_config);
        assert_eq!(
            *bank.collector_fee_details.read().unwrap(),
            CollectorFeeDetails::default()
        );

        let initial_capitalization = bank.capitalization();
        let initial_leader_id_balance = bank.get_balance(bank.leader_id());
        bank.distribute_transaction_fee_details();
        let new_leader_id_balance = bank.get_balance(bank.leader_id());

        assert_eq!(initial_leader_id_balance, new_leader_id_balance);
        assert_eq!(initial_capitalization, bank.capitalization());
        let locked_rewards = bank.rewards.read().unwrap();
        assert!(
            locked_rewards.is_empty(),
            "There should be no rewards distributed"
        );
    }

    #[test]
    fn test_distribute_transaction_fee_details_overflow_failure() {
        let genesis = create_genesis_config(0);
        let mut bank = Bank::new_for_tests(&genesis.genesis_config);
        let transaction_fee = 100;
        let priority_fee = 200;
        let total_fee = transaction_fee + priority_fee;
        bank.collector_fee_details = RwLock::new(CollectorFeeDetails {
            transaction_fee,
            priority_fee,
        });

        // ensure that account balance will overflow and fee distribution will fail
        let account = AccountSharedData::new(u64::MAX, 0, &system_program::id());
        bank.store_account(bank.leader_id(), &account);

        let initial_capitalization = bank.capitalization();
        let initial_leader_id_balance = bank.get_balance(bank.leader_id());
        bank.distribute_transaction_fee_details();
        let new_leader_id_balance = bank.get_balance(bank.leader_id());

        // At epoch 0, validator gets 0%, so no deposit attempt, balance unchanged
        assert_eq!(initial_leader_id_balance, new_leader_id_balance);
        assert_eq!(initial_capitalization - total_fee, bank.capitalization());
        let locked_rewards = bank.rewards.read().unwrap();
        assert!(
            locked_rewards.is_empty(),
            "There should be no rewards distributed"
        );
    }
}

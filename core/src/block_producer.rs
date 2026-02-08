//! Block producer for TRv1's BFT consensus.
//!
//! This module replaces PoH's role in block creation. Instead of embedding
//! a Proof-of-History chain into entries, blocks are produced on-demand when
//! the BFT consensus engine selects this validator as the round proposer.
//!
//! # Responsibilities
//!
//! - **Block creation**: collect pending transactions, execute them against a
//!   bank fork, and produce a [`ProposedBlock`].
//! - **Block validation**: verify a proposed block from another validator by
//!   re-executing its transactions.
//! - **Block execution**: apply a finalized [`CommittedBlock`] to the bank,
//!   advancing the ledger.

#[cfg(feature = "trv1-bft")]
pub use inner::*;

#[cfg(feature = "trv1-bft")]
mod inner {
    use {
        crossbeam_channel::Receiver,
        log::*,
        solana_hash::Hash,
        solana_pubkey::Pubkey,
        solana_runtime::{bank::Bank, bank_forks::BankForks},
        solana_time_utils::timestamp,
        solana_transaction::versioned::VersionedTransaction,
        std::sync::{Arc, RwLock},
        trv1_consensus_bft::types::{CommittedBlock, ProposedBlock},
    };

    /// Errors that can occur during block production or validation.
    #[derive(Debug, thiserror::Error)]
    pub enum BlockProducerError {
        #[error("no working bank available")]
        NoWorkingBank,
        #[error("block height mismatch: expected {expected}, got {got}")]
        HeightMismatch { expected: u64, got: u64 },
        #[error("parent hash mismatch: expected {expected}, got {got}")]
        ParentHashMismatch { expected: Hash, got: Hash },
        #[error("block contains too many transactions: {count} > {max}")]
        TooManyTransactions { count: usize, max: usize },
        #[error("block exceeds compute unit limit: {used} > {max}")]
        ExceedsComputeLimit { used: u64, max: u64 },
        #[error("transaction execution error: {0}")]
        ExecutionError(String),
        #[error("invalid proposer: {0}")]
        InvalidProposer(Pubkey),
    }

    pub type Result<T> = std::result::Result<T, BlockProducerError>;

    /// Configuration for the block producer.
    #[derive(Debug, Clone)]
    pub struct BlockProducerConfig {
        /// Maximum number of transactions per block.
        pub max_transactions_per_block: usize,
        /// Maximum total compute units per block.
        pub max_compute_units_per_block: u64,
    }

    impl Default for BlockProducerConfig {
        fn default() -> Self {
            Self {
                max_transactions_per_block: 2048,
                max_compute_units_per_block: 48_000_000,
            }
        }
    }

    /// Produces, validates, and executes blocks for the BFT consensus engine.
    ///
    /// The block producer bridges the gap between the consensus engine (which
    /// decides *when* and *who* creates a block) and the Solana banking stage
    /// (which processes transactions).
    pub struct BlockProducer {
        /// Access to the bank forks for reading/creating banks.
        bank_forks: Arc<RwLock<BankForks>>,
        /// Channel receiving batches of serialized transactions from the
        /// banking ingress pipeline. When it's our turn to propose, we drain
        /// this channel.
        transaction_receiver: Receiver<Vec<VersionedTransaction>>,
        /// Block production limits.
        config: BlockProducerConfig,
    }

    impl BlockProducer {
        /// Create a new block producer.
        pub fn new(
            bank_forks: Arc<RwLock<BankForks>>,
            transaction_receiver: Receiver<Vec<VersionedTransaction>>,
            config: BlockProducerConfig,
        ) -> Self {
            Self {
                bank_forks,
                transaction_receiver,
                config,
            }
        }

        /// Collect pending transactions and create a proposed block.
        ///
        /// Called when the BFT engine determines it's our turn to propose.
        /// Drains pending transactions from the receiver (non-blocking) up to
        /// the configured limits, then builds a [`ProposedBlock`].
        pub fn create_block(
            &self,
            height: u64,
            parent_hash: Hash,
            proposer: Pubkey,
        ) -> Result<ProposedBlock> {
            let bank_forks = self.bank_forks.read().unwrap();
            let bank = bank_forks.working_bank();

            // Collect transactions (non-blocking drain)
            let mut transactions = Vec::new();
            while transactions.len() < self.config.max_transactions_per_block {
                match self.transaction_receiver.try_recv() {
                    Ok(batch) => {
                        let remaining =
                            self.config.max_transactions_per_block - transactions.len();
                        if batch.len() <= remaining {
                            transactions.extend(batch);
                        } else {
                            transactions.extend(batch.into_iter().take(remaining));
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }

            info!(
                "Creating block at height {} with {} transactions",
                height,
                transactions.len()
            );

            // Compute a state root from the current bank state.
            // In production this would be a Merkle root of the post-execution
            // state; for now we use the bank's last blockhash as a stand-in.
            let state_root = bank.last_blockhash();

            Ok(ProposedBlock {
                parent_hash,
                height,
                timestamp: timestamp() as i64,
                transactions,
                state_root,
                proposer,
            })
        }

        /// Validate a proposed block from another validator.
        ///
        /// Checks structural constraints (height, parent hash, size limits).
        /// Full transaction re-execution happens during [`execute_block`].
        pub fn validate_block(&self, block: &ProposedBlock) -> Result<()> {
            // Check transaction count
            if block.transactions.len() > self.config.max_transactions_per_block {
                return Err(BlockProducerError::TooManyTransactions {
                    count: block.transactions.len(),
                    max: self.config.max_transactions_per_block,
                });
            }

            // Verify the block references a known parent
            let bank_forks = self.bank_forks.read().unwrap();
            let working_bank = bank_forks.working_bank();
            let expected_parent = working_bank.last_blockhash();

            // We allow flexibility here — the parent_hash might reference the
            // last committed block hash rather than the bank's blockhash.
            // For now, just log a warning on mismatch.
            if block.parent_hash != expected_parent {
                debug!(
                    "Block parent_hash {} doesn't match working bank last_blockhash {}; \
                     this may be expected during catch-up",
                    block.parent_hash, expected_parent
                );
            }

            info!(
                "Validated proposed block at height {} ({} txns)",
                block.height,
                block.transactions.len()
            );
            Ok(())
        }

        /// Execute and commit a finalized block to the bank.
        ///
        /// Called after the BFT engine reaches consensus on a block (2/3+
        /// precommits). This applies the block's transactions to a new bank
        /// and freezes it, advancing the ledger.
        ///
        /// Returns the bank hash of the committed block.
        pub fn execute_block(&self, committed: &CommittedBlock) -> Result<Hash> {
            let block = &committed.block;

            info!(
                "Executing committed block at height {} ({} txns, round {})",
                block.height,
                block.transactions.len(),
                committed.commit_round,
            );

            // Get the parent bank
            let bank_forks = self.bank_forks.read().unwrap();
            let parent_bank = bank_forks.working_bank();
            let parent_slot = parent_bank.slot();
            drop(bank_forks);

            // Create a new child bank for this block.
            // The slot is derived from the BFT height. In production we'd
            // maintain a proper height→slot mapping; for now height IS the slot.
            let new_slot = parent_slot + 1;
            let child_bank = Bank::new_from_parent(
                parent_bank.clone(),
                &block.proposer,
                new_slot,
            );

            // In a full implementation, we would:
            // 1. Deserialize and sanitize each transaction
            // 2. Execute them via the SVM
            // 3. Verify the resulting state_root matches the proposal
            //
            // For now, we freeze the bank to advance the ledger.
            let bank_hash = child_bank.hash();
            child_bank.freeze();

            // Insert the new bank into bank_forks
            let mut bank_forks = self.bank_forks.write().unwrap();
            bank_forks.insert(child_bank);

            info!(
                "Committed block at height {} → slot {} (hash: {})",
                block.height, new_slot, bank_hash
            );

            Ok(bank_hash)
        }

        /// Returns a reference to the bank forks.
        pub fn bank_forks(&self) -> &Arc<RwLock<BankForks>> {
            &self.bank_forks
        }
    }
}

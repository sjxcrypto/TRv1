//! BFT consensus adapter for the TRv1 validator.
//!
//! This module bridges the standalone `trv1-consensus-bft` engine with the
//! validator's infrastructure (bank forks, block producer, networking).
//!
//! # Data flow
//!
//! ```text
//! ┌───────────────────────────────────────────────────────────┐
//! │                    BftAdapter                             │
//! │                                                           │
//! │  ┌──────────────┐     ┌──────────────┐    ┌───────────┐ │
//! │  │ Consensus    │────▶│ Block        │───▶│ Bank      │ │
//! │  │ Engine       │     │ Producer     │    │ Forks     │ │
//! │  └──────┬───────┘     └──────────────┘    └───────────┘ │
//! │         │                                                │
//! │  ┌──────▼───────┐                                       │
//! │  │ Timeout      │                                       │
//! │  │ Scheduler    │                                       │
//! │  └──────────────┘                                       │
//! └───────────────────────────────────────────────────────────┘
//!          ▲                          │
//!          │ ConsensusMessage         │ ConsensusMessage
//!          │ (from network)           ▼ (to network)
//! ```
//!
//! The adapter:
//! 1. Owns the BFT consensus engine and its timeout scheduler.
//! 2. Translates between the engine's pure state-machine I/O and actual
//!    network/banking operations.
//! 3. Produces blocks when it's our turn to propose.
//! 4. Commits blocks when the engine reaches consensus.

#[cfg(feature = "trv1-bft")]
pub use inner::*;

#[cfg(feature = "trv1-bft")]
mod inner {
    use {
        crate::block_producer::BlockProducer,
        log::*,
        solana_hash::Hash,
        solana_keypair::Keypair,
        solana_pubkey::Pubkey,
        solana_runtime::bank_forks::BankForks,
        solana_signature::Signature,
        solana_signer::Signer,
        std::sync::{Arc, RwLock},
        trv1_consensus_bft::{
            config::BftConfig,
            engine::{ConsensusEngine, EngineOutput},
            timeout::TimeoutScheduler,
            types::{ConsensusMessage, ConsensusStep, ProposedBlock},
            validator_set::ValidatorSet,
        },
    };

    /// Result of processing a consensus event through the adapter.
    #[derive(Debug)]
    pub struct AdapterOutput {
        /// Messages to broadcast to the network.
        pub messages: Vec<ConsensusMessage>,
        /// Whether a block was committed this round.
        pub block_committed: bool,
        /// The hash of the committed block, if any.
        pub committed_hash: Option<Hash>,
    }

    impl AdapterOutput {
        fn empty() -> Self {
            Self {
                messages: Vec::new(),
                block_committed: false,
                committed_hash: None,
            }
        }

        fn from_engine_output(output: EngineOutput) -> Self {
            Self {
                block_committed: output.committed_block.is_some(),
                committed_hash: output
                    .committed_block
                    .as_ref()
                    .map(|cb| cb.block.hash()),
                messages: output.messages,
            }
        }
    }

    /// Bridges the consensus engine with the validator's infrastructure.
    ///
    /// The `BftAdapter` owns the consensus engine and translates between its
    /// pure state-machine outputs and actual I/O operations (block creation,
    /// block execution, network messaging).
    pub struct BftAdapter {
        /// The BFT consensus state machine.
        engine: ConsensusEngine,
        /// Timeout tracking for consensus phases.
        timeout_scheduler: TimeoutScheduler,
        /// This validator's signing keypair.
        validator_keypair: Arc<Keypair>,
        /// Shared bank forks.
        bank_forks: Arc<RwLock<BankForks>>,
        /// Block producer for creating and executing blocks.
        block_producer: Arc<BlockProducer>,
        /// Hash of the last committed block (used as parent for new blocks).
        last_committed_hash: Hash,
    }

    impl BftAdapter {
        /// Create a new BFT adapter.
        pub fn new(
            config: BftConfig,
            validator_keypair: Arc<Keypair>,
            validator_set: ValidatorSet,
            bank_forks: Arc<RwLock<BankForks>>,
            block_producer: Arc<BlockProducer>,
        ) -> Self {
            let identity = validator_keypair.pubkey();
            let timeout_scheduler = TimeoutScheduler::new(config.clone());
            let engine = ConsensusEngine::new(config, identity, validator_set);

            // Seed the last committed hash from the working bank
            let last_committed_hash = {
                let forks = bank_forks.read().unwrap();
                forks.working_bank().last_blockhash()
            };

            Self {
                engine,
                timeout_scheduler,
                validator_keypair,
                bank_forks,
                block_producer,
                last_committed_hash,
            }
        }

        // -- Public API --

        /// Start consensus for a new height.
        ///
        /// Resets the engine state, starts the propose timeout, and if we are
        /// the proposer, creates and returns a proposal message.
        pub fn start_height(&mut self, height: u64) -> AdapterOutput {
            info!("BftAdapter: starting height {height}");
            let output = self.engine.start_new_height(height);

            // Start the propose timeout
            self.timeout_scheduler
                .start(ConsensusStep::Propose, self.engine.round());

            let mut adapter_output = AdapterOutput::from_engine_output(output);

            // If we are the proposer for this (height, round), create a block
            if self.engine.is_proposer(height, self.engine.round()) {
                info!("BftAdapter: we are the proposer for h={height} r={}", self.engine.round());
                match self.produce_block(height) {
                    Ok(proposal_msg) => {
                        adapter_output.messages.push(proposal_msg);
                    }
                    Err(e) => {
                        warn!("BftAdapter: failed to produce block: {e}");
                    }
                }
            }

            adapter_output
        }

        /// Process an incoming consensus message.
        ///
        /// Routes the message to the appropriate engine handler and manages
        /// timeout transitions.
        pub fn handle_message(&mut self, msg: ConsensusMessage) -> AdapterOutput {
            let output = match &msg {
                ConsensusMessage::Proposal { .. } => {
                    let output = self.engine.on_proposal(msg);
                    // Received a valid proposal — transition to prevote timeout
                    if self.engine.step() >= ConsensusStep::Prevote {
                        self.timeout_scheduler
                            .start(ConsensusStep::Prevote, self.engine.round());
                    }
                    output
                }
                ConsensusMessage::Prevote { .. } => {
                    let output = self.engine.on_prevote(msg);
                    // If we advanced to precommit, start precommit timeout
                    if self.engine.step() >= ConsensusStep::Precommit {
                        self.timeout_scheduler
                            .start(ConsensusStep::Precommit, self.engine.round());
                    }
                    output
                }
                ConsensusMessage::Precommit { .. } => self.engine.on_precommit(msg),
            };

            self.process_engine_output(output)
        }

        /// Check for and handle any expired timeouts.
        ///
        /// Should be called periodically (e.g., every 100ms) by the consensus
        /// service loop.
        pub fn check_timeouts(&mut self) -> AdapterOutput {
            if let Some(expired_step) = self.timeout_scheduler.check_expired() {
                info!(
                    "BftAdapter: timeout expired for {:?} at h={} r={}",
                    expired_step,
                    self.engine.height(),
                    self.engine.round()
                );
                let output = self.engine.on_timeout(expired_step);

                // Schedule the next timeout based on new state
                match self.engine.step() {
                    ConsensusStep::Propose => {
                        self.timeout_scheduler
                            .start(ConsensusStep::Propose, self.engine.round());

                        // If we're the proposer in the new round, produce a block
                        let height = self.engine.height();
                        let round = self.engine.round();
                        let mut adapter_output = AdapterOutput::from_engine_output(output);
                        if self.engine.is_proposer(height, round) {
                            match self.produce_block(height) {
                                Ok(proposal_msg) => {
                                    adapter_output.messages.push(proposal_msg);
                                }
                                Err(e) => {
                                    warn!("BftAdapter: failed to produce block: {e}");
                                }
                            }
                        }
                        return adapter_output;
                    }
                    ConsensusStep::Prevote => {
                        self.timeout_scheduler
                            .start(ConsensusStep::Prevote, self.engine.round());
                    }
                    ConsensusStep::Precommit => {
                        self.timeout_scheduler
                            .start(ConsensusStep::Precommit, self.engine.round());
                    }
                    ConsensusStep::Commit => {
                        self.timeout_scheduler.cancel();
                    }
                    _ => {}
                }

                return self.process_engine_output(output);
            }

            AdapterOutput::empty()
        }

        /// Returns time remaining until the next timeout, or None.
        pub fn time_to_next_timeout(&self) -> Option<std::time::Duration> {
            self.timeout_scheduler.remaining()
        }

        /// Update the validator set (e.g., at epoch boundaries).
        pub fn update_validator_set(&mut self, validator_set: ValidatorSet) {
            info!(
                "BftAdapter: updating validator set ({} validators)",
                validator_set.len()
            );
            self.engine.update_validator_set(validator_set);
        }

        // -- Accessors --

        /// Returns the current consensus height.
        pub fn height(&self) -> u64 {
            self.engine.height()
        }

        /// Returns the current consensus round.
        pub fn round(&self) -> u32 {
            self.engine.round()
        }

        /// Returns the current consensus step.
        pub fn step(&self) -> ConsensusStep {
            self.engine.step()
        }

        /// Returns this validator's identity pubkey.
        pub fn identity(&self) -> Pubkey {
            self.validator_keypair.pubkey()
        }

        /// Returns a reference to the consensus engine.
        pub fn engine(&self) -> &ConsensusEngine {
            &self.engine
        }

        /// Returns a reference to the block producer.
        pub fn block_producer(&self) -> &Arc<BlockProducer> {
            &self.block_producer
        }

        // -- Internal --

        /// Create a block proposal and wrap it in a [`ConsensusMessage`].
        fn produce_block(
            &self,
            height: u64,
        ) -> std::result::Result<ConsensusMessage, crate::block_producer::BlockProducerError>
        {
            let identity = self.validator_keypair.pubkey();
            let block = self.block_producer.create_block(
                height,
                self.last_committed_hash,
                identity,
            )?;

            // In production we'd sign the block hash with the validator keypair.
            // For now, use a default signature.
            let proposal = ConsensusMessage::Proposal {
                height,
                round: self.engine.round(),
                block,
                proposer: identity,
                signature: Signature::default(),
                valid_round: self.engine.state().valid_round,
            };

            Ok(proposal)
        }

        /// Process engine output: commit blocks and translate to adapter output.
        fn process_engine_output(&mut self, output: EngineOutput) -> AdapterOutput {
            if let Some(ref committed_block) = output.committed_block {
                // Commit the block via the block producer
                match self.block_producer.execute_block(committed_block) {
                    Ok(bank_hash) => {
                        info!(
                            "BftAdapter: committed block at height {} (bank_hash: {bank_hash})",
                            committed_block.block.height,
                        );
                        self.last_committed_hash = committed_block.block.hash();
                        self.timeout_scheduler.cancel();

                        return AdapterOutput {
                            messages: output.messages,
                            block_committed: true,
                            committed_hash: Some(bank_hash),
                        };
                    }
                    Err(e) => {
                        error!(
                            "BftAdapter: failed to commit block at height {}: {e}",
                            committed_block.block.height
                        );
                    }
                }
            }

            AdapterOutput::from_engine_output(output)
        }
    }
}

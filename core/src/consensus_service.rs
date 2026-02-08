//! TRv1 BFT Consensus Service
//!
//! A long-running service that drives the BFT consensus loop. It:
//!
//! 1. Listens for incoming consensus messages from the network.
//! 2. Feeds them to the BFT adapter (which wraps the consensus engine).
//! 3. Broadcasts outgoing consensus messages to peers.
//! 4. Handles timeouts and round advancement.
//! 5. Advances to the next height after each committed block.
//!
//! # Architecture
//!
//! ```text
//!  ┌──────────────────────────────────────────────────┐
//!  │              ConsensusService                     │
//!  │                                                   │
//!  │  ┌────────────┐    ┌────────────┐                │
//!  │  │ Network RX │───▶│ BftAdapter │──▶ outgoing TX │
//!  │  │ (inbound)  │    │            │                │
//!  │  └────────────┘    └─────┬──────┘                │
//!  │                          │                       │
//!  │                    ┌─────▼──────┐                │
//!  │                    │ Block      │                │
//!  │                    │ Producer   │                │
//!  │                    └─────┬──────┘                │
//!  │                          │                       │
//!  │                    ┌─────▼──────┐                │
//!  │                    │ Bank Forks │                │
//!  │                    └────────────┘                │
//!  └──────────────────────────────────────────────────┘
//! ```

#[cfg(feature = "trv1-bft")]
pub use inner::*;

#[cfg(feature = "trv1-bft")]
mod inner {
    use {
        crate::{
            bft_adapter::{AdapterOutput, BftAdapter},
            block_producer::{BlockProducer, BlockProducerConfig},
        },
        crossbeam_channel::{Receiver, Sender},
        log::*,
        solana_gossip::cluster_info::ClusterInfo,
        solana_keypair::Keypair,
        solana_runtime::bank_forks::BankForks,
        solana_transaction::versioned::VersionedTransaction,
        std::{
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc, RwLock,
            },
            thread::{self, Builder, JoinHandle},
            time::Duration,
        },
        trv1_consensus_bft::{
            config::BftConfig,
            types::{ConsensusMessage, ConsensusStep},
            validator_set::ValidatorSet,
        },
    };

    /// How often to poll for timeouts when no messages are arriving.
    const TIMEOUT_POLL_INTERVAL_MS: u64 = 50;

    /// Configuration for the consensus service.
    #[derive(Debug, Clone)]
    pub struct ConsensusServiceConfig {
        /// BFT consensus engine configuration.
        pub bft_config: BftConfig,
        /// Block production limits.
        pub block_producer_config: BlockProducerConfig,
        /// Starting height for consensus (usually the latest committed + 1).
        pub start_height: u64,
    }

    impl Default for ConsensusServiceConfig {
        fn default() -> Self {
            Self {
                bft_config: BftConfig::default(),
                block_producer_config: BlockProducerConfig::default(),
                start_height: 1,
            }
        }
    }

    /// A service that runs the BFT consensus event loop.
    ///
    /// The service spawns a dedicated thread that:
    /// - Receives consensus messages from the network layer.
    /// - Feeds them through the [`BftAdapter`].
    /// - Sends resulting outbound messages back to the network.
    /// - Monitors timeouts and drives round/height advancement.
    pub struct ConsensusService {
        thread: JoinHandle<()>,
    }

    impl ConsensusService {
        /// Create and start the consensus service.
        ///
        /// # Arguments
        ///
        /// * `config` — Service configuration including BFT params.
        /// * `validator_keypair` — This validator's signing identity.
        /// * `validator_set` — Initial stake-weighted validator set.
        /// * `bank_forks` — Shared bank forks for block execution.
        /// * `_cluster_info` — Cluster gossip (reserved for future peer
        ///   discovery integration).
        /// * `consensus_msg_receiver` — Inbound consensus messages from
        ///   the network layer.
        /// * `consensus_msg_sender` — Outbound consensus messages to
        ///   broadcast to the network.
        /// * `transaction_receiver` — Pending transactions for block
        ///   production.
        /// * `exit` — Global shutdown flag.
        #[allow(clippy::too_many_arguments)]
        pub fn new(
            config: ConsensusServiceConfig,
            validator_keypair: Arc<Keypair>,
            validator_set: ValidatorSet,
            bank_forks: Arc<RwLock<BankForks>>,
            _cluster_info: Arc<ClusterInfo>,
            consensus_msg_receiver: Receiver<ConsensusMessage>,
            consensus_msg_sender: Sender<ConsensusMessage>,
            transaction_receiver: Receiver<Vec<VersionedTransaction>>,
            exit: Arc<AtomicBool>,
        ) -> Self {
            let block_producer = Arc::new(BlockProducer::new(
                bank_forks.clone(),
                transaction_receiver,
                config.block_producer_config.clone(),
            ));

            let mut adapter = BftAdapter::new(
                config.bft_config.clone(),
                validator_keypair,
                validator_set,
                bank_forks,
                block_producer,
            );

            let start_height = config.start_height;
            let block_time_ms = config.bft_config.block_time_ms;

            let thread = Builder::new()
                .name("trv1BftConsensus".to_string())
                .spawn(move || {
                    Self::run(
                        &mut adapter,
                        start_height,
                        block_time_ms,
                        &consensus_msg_receiver,
                        &consensus_msg_sender,
                        &exit,
                    );
                })
                .expect("failed to spawn BFT consensus thread");

            Self { thread }
        }

        /// Join the consensus service thread.
        pub fn join(self) -> thread::Result<()> {
            self.thread.join()
        }

        /// Main consensus loop.
        fn run(
            adapter: &mut BftAdapter,
            start_height: u64,
            block_time_ms: u64,
            consensus_msg_receiver: &Receiver<ConsensusMessage>,
            consensus_msg_sender: &Sender<ConsensusMessage>,
            exit: &Arc<AtomicBool>,
        ) {
            info!(
                "ConsensusService: starting at height {start_height} \
                 (identity: {})",
                adapter.identity()
            );

            let mut current_height = start_height;

            // Start the first height
            let initial_output = adapter.start_height(current_height);
            Self::broadcast_messages(&initial_output, consensus_msg_sender);

            loop {
                if exit.load(Ordering::Relaxed) {
                    info!("ConsensusService: exit signal received, shutting down");
                    break;
                }

                // Calculate how long to wait for the next message.
                // Use the minimum of:
                // - time to next timeout
                // - a fixed poll interval (to stay responsive to exit)
                let wait_duration = adapter
                    .time_to_next_timeout()
                    .map(|d| d.min(Duration::from_millis(TIMEOUT_POLL_INTERVAL_MS)))
                    .unwrap_or(Duration::from_millis(TIMEOUT_POLL_INTERVAL_MS));

                // Try to receive a consensus message
                match consensus_msg_receiver.recv_timeout(wait_duration) {
                    Ok(msg) => {
                        trace!(
                            "ConsensusService: received {:?} for h={} r={}",
                            msg_kind(&msg),
                            msg.height(),
                            msg.round()
                        );

                        let output = adapter.handle_message(msg);
                        Self::broadcast_messages(&output, consensus_msg_sender);

                        // If a block was committed, advance to next height
                        if output.block_committed {
                            current_height += 1;
                            info!(
                                "ConsensusService: block committed, advancing to height {}",
                                current_height
                            );

                            // Brief pause to respect the target block time.
                            // In production we'd use a more sophisticated
                            // timer that accounts for actual elapsed time.
                            thread::sleep(Duration::from_millis(block_time_ms / 2));

                            let new_output = adapter.start_height(current_height);
                            Self::broadcast_messages(&new_output, consensus_msg_sender);
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        // No message — check for timeouts
                        let output = adapter.check_timeouts();
                        if !output.messages.is_empty() || output.block_committed {
                            Self::broadcast_messages(&output, consensus_msg_sender);

                            if output.block_committed {
                                current_height += 1;
                                info!(
                                    "ConsensusService: block committed (via timeout path), \
                                     advancing to height {}",
                                    current_height
                                );
                                thread::sleep(Duration::from_millis(block_time_ms / 2));
                                let new_output = adapter.start_height(current_height);
                                Self::broadcast_messages(&new_output, consensus_msg_sender);
                            }
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        info!(
                            "ConsensusService: message channel disconnected, shutting down"
                        );
                        break;
                    }
                }
            }

            info!("ConsensusService: consensus loop exited at height {current_height}");
        }

        /// Send all outbound messages through the network sender.
        fn broadcast_messages(
            output: &AdapterOutput,
            sender: &Sender<ConsensusMessage>,
        ) {
            for msg in &output.messages {
                trace!("ConsensusService: broadcasting {:?}", msg_kind(msg));
                if let Err(e) = sender.send(msg.clone()) {
                    warn!("ConsensusService: failed to send outbound message: {e}");
                }
            }
        }
    }

    /// Helper: extract a short tag for logging.
    fn msg_kind(msg: &ConsensusMessage) -> &'static str {
        match msg {
            ConsensusMessage::Proposal { .. } => "Proposal",
            ConsensusMessage::Prevote { .. } => "Prevote",
            ConsensusMessage::Precommit { .. } => "Precommit",
        }
    }
}

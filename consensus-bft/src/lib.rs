//! TRv1 Tendermint-style BFT Consensus Engine
//!
//! This crate implements a Tendermint-inspired Byzantine Fault Tolerant (BFT)
//! consensus protocol, replacing Solana's Proof of History (PoH) with
//! deterministic finality through a three-phase commit protocol:
//!
//! 1. **Propose** — A stake-weighted round-robin leader proposes a block.
//! 2. **Prevote** — Validators evaluate the proposal and broadcast prevotes.
//! 3. **Precommit** — Upon observing 2/3+ prevotes, validators broadcast precommits.
//! 4. **Commit** — Upon observing 2/3+ precommits, the block is committed with
//!    deterministic finality.
//!
//! # Key Properties
//!
//! - **Deterministic finality**: Once committed, a block cannot be reverted
//!   (unlike Solana's probabilistic optimistic confirmation).
//! - **1-second block time** with ~6-second finality under normal operation.
//! - **Liveness**: The protocol makes progress as long as 2/3+ of stake is
//!   online and honest, through timeout-driven round advancement.
//! - **Safety**: No two conflicting blocks can be committed at the same height,
//!   as long as less than 1/3 of stake is Byzantine.
//! - **Accountability**: Double-sign evidence is collected and can be used for
//!   slashing.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │              ConsensusEngine                 │
//! │  ┌─────────┐  ┌──────────┐  ┌───────────┐  │
//! │  │ Config  │  │ Validator│  │ Evidence  │  │
//! │  │         │  │   Set    │  │ Collector │  │
//! │  └─────────┘  └──────────┘  └───────────┘  │
//! │  ┌─────────────────────────────────────┐    │
//! │  │        ConsensusState               │    │
//! │  │  height, round, step, locks, votes  │    │
//! │  └─────────────────────────────────────┘    │
//! │  ┌──────────┐  ┌───────────────────┐       │
//! │  │ Proposer │  │ TimeoutScheduler  │       │
//! │  │ Selection│  │                   │       │
//! │  └──────────┘  └───────────────────┘       │
//! └─────────────────────────────────────────────┘
//! ```

pub mod config;
pub mod engine;
pub mod evidence;
pub mod proposer;
pub mod timeout;
pub mod types;
pub mod validator_set;

// Re-exports for convenience
pub use config::BftConfig;
pub use engine::{ConsensusEngine, EngineOutput};
pub use evidence::{DoubleSignEvidence, EvidenceCollector, EvidenceKind};
pub use proposer::{is_proposer, proposer_for_round};
pub use timeout::TimeoutScheduler;
pub use types::{
    CommittedBlock, ConsensusMessage, ConsensusState, ConsensusStep, ProposedBlock,
};
pub use validator_set::{ValidatorInfo, ValidatorSet};

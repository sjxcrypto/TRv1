//! Core types for the BFT consensus protocol.
//!
//! Defines message types (Proposal, Prevote, Precommit), block types
//! (ProposedBlock, CommittedBlock), and the consensus state machine state.

use {
    solana_hash::Hash,
    solana_pubkey::Pubkey,
    solana_signature::Signature,
    solana_transaction::versioned::VersionedTransaction,
    std::collections::HashMap,
};

// ---------------------------------------------------------------------------
// Consensus messages
// ---------------------------------------------------------------------------

/// Messages exchanged between validators during consensus rounds.
#[derive(Debug, Clone)]
pub enum ConsensusMessage {
    /// A block proposal broadcast by the round's designated proposer.
    Proposal {
        height: u64,
        round: u32,
        block: ProposedBlock,
        proposer: Pubkey,
        signature: Signature,
        /// Tendermint "polka" rule: if the proposer already saw 2/3+ prevotes
        /// for this value in a prior round, it attaches `valid_round` so that
        /// locked validators can unlock.
        valid_round: Option<u32>,
    },

    /// A prevote cast by a validator after evaluating a proposal.
    /// `block_hash == None` represents a "nil" prevote (no valid proposal seen).
    Prevote {
        height: u64,
        round: u32,
        block_hash: Option<Hash>,
        voter: Pubkey,
        signature: Signature,
    },

    /// A precommit cast after observing 2/3+ prevotes for a value (or nil).
    /// `block_hash == None` represents a "nil" precommit.
    Precommit {
        height: u64,
        round: u32,
        block_hash: Option<Hash>,
        voter: Pubkey,
        signature: Signature,
    },
}

impl ConsensusMessage {
    /// Returns the height this message belongs to.
    pub fn height(&self) -> u64 {
        match self {
            ConsensusMessage::Proposal { height, .. }
            | ConsensusMessage::Prevote { height, .. }
            | ConsensusMessage::Precommit { height, .. } => *height,
        }
    }

    /// Returns the round this message belongs to.
    pub fn round(&self) -> u32 {
        match self {
            ConsensusMessage::Proposal { round, .. }
            | ConsensusMessage::Prevote { round, .. }
            | ConsensusMessage::Precommit { round, .. } => *round,
        }
    }

    /// Returns the pubkey of the message sender.
    pub fn sender(&self) -> &Pubkey {
        match self {
            ConsensusMessage::Proposal { proposer, .. } => proposer,
            ConsensusMessage::Prevote { voter, .. }
            | ConsensusMessage::Precommit { voter, .. } => voter,
        }
    }

    /// Returns the signature on this message.
    pub fn signature(&self) -> &Signature {
        match self {
            ConsensusMessage::Proposal { signature, .. }
            | ConsensusMessage::Prevote { signature, .. }
            | ConsensusMessage::Precommit { signature, .. } => signature,
        }
    }
}

// ---------------------------------------------------------------------------
// Block types
// ---------------------------------------------------------------------------

/// A block proposed by a leader during the Propose phase.
#[derive(Debug, Clone)]
pub struct ProposedBlock {
    /// Hash of the parent (previous committed) block.
    pub parent_hash: Hash,
    /// Block height (monotonically increasing).
    pub height: u64,
    /// Unix timestamp in milliseconds when the block was proposed.
    pub timestamp: i64,
    /// Transactions included in this block.
    pub transactions: Vec<VersionedTransaction>,
    /// Merkle root of the post-execution state.
    pub state_root: Hash,
    /// Public key of the proposer.
    pub proposer: Pubkey,
}

impl ProposedBlock {
    /// Compute a deterministic hash for this block.
    /// Uses parent_hash, height, timestamp, state_root, and proposer.
    /// Transactions are captured via state_root.
    pub fn hash(&self) -> Hash {
        // Build a composite hash from the block's deterministic fields.
        solana_sha256_hasher::hashv(&[
            self.parent_hash.as_ref(),
            &self.height.to_le_bytes(),
            &self.timestamp.to_le_bytes(),
            self.state_root.as_ref(),
            self.proposer.as_ref(),
        ])
    }
}

/// A block that has been committed by 2/3+ of the validator set.
#[derive(Debug, Clone)]
pub struct CommittedBlock {
    /// The original proposed block.
    pub block: ProposedBlock,
    /// Signatures from validators that precommitted this block.
    pub commit_signatures: Vec<(Pubkey, Signature)>,
    /// The round in which consensus was reached.
    pub commit_round: u32,
}

// ---------------------------------------------------------------------------
// Consensus state
// ---------------------------------------------------------------------------

/// The step within a single consensus round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConsensusStep {
    /// Waiting for the round to begin.
    NewRound,
    /// Waiting for a proposal from the designated leader.
    Propose,
    /// Collecting prevotes from validators.
    Prevote,
    /// Collecting precommits from validators.
    Precommit,
    /// Block has been committed; ready to advance height.
    Commit,
}

impl std::fmt::Display for ConsensusStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsensusStep::NewRound => write!(f, "NewRound"),
            ConsensusStep::Propose => write!(f, "Propose"),
            ConsensusStep::Prevote => write!(f, "Prevote"),
            ConsensusStep::Precommit => write!(f, "Precommit"),
            ConsensusStep::Commit => write!(f, "Commit"),
        }
    }
}

/// Internal state of the consensus engine for a given height.
#[derive(Debug, Clone)]
pub struct ConsensusState {
    /// Current block height being decided.
    pub height: u64,
    /// Current round within this height.
    pub round: u32,
    /// Current step within the round.
    pub step: ConsensusStep,

    // -- Tendermint lock variables --
    /// Hash of the value this validator is locked on (if any).
    pub locked_value: Option<Hash>,
    /// Round in which the lock was acquired.
    pub locked_round: Option<u32>,
    /// Hash of the value that has received a valid polka (2/3+ prevotes).
    pub valid_value: Option<Hash>,
    /// Round in which the valid polka was observed.
    pub valid_round: Option<u32>,

    // -- Vote collection --
    /// Prevotes collected for the current round. Key = voter pubkey, Value = block hash (None = nil).
    pub prevotes: HashMap<Pubkey, Option<Hash>>,
    /// Precommits collected for the current round. Key = voter pubkey, Value = block hash (None = nil).
    pub precommits: HashMap<Pubkey, Option<Hash>>,

    /// The proposed block for this round (if received).
    pub proposal: Option<ProposedBlock>,
}

impl ConsensusState {
    /// Create a fresh state for a new height.
    pub fn new(height: u64) -> Self {
        Self {
            height,
            round: 0,
            step: ConsensusStep::NewRound,
            locked_value: None,
            locked_round: None,
            valid_value: None,
            valid_round: None,
            prevotes: HashMap::new(),
            precommits: HashMap::new(),
            proposal: None,
        }
    }

    /// Reset vote collections for a new round while preserving lock state.
    pub fn advance_round(&mut self, new_round: u32) {
        self.round = new_round;
        self.step = ConsensusStep::NewRound;
        self.prevotes.clear();
        self.precommits.clear();
        self.proposal = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proposed_block_hash_deterministic() {
        let block = ProposedBlock {
            parent_hash: Hash::default(),
            height: 1,
            timestamp: 1000,
            transactions: vec![],
            state_root: Hash::default(),
            proposer: Pubkey::default(),
        };
        let h1 = block.hash();
        let h2 = block.hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_proposed_block_hash_changes_with_height() {
        let block1 = ProposedBlock {
            parent_hash: Hash::default(),
            height: 1,
            timestamp: 1000,
            transactions: vec![],
            state_root: Hash::default(),
            proposer: Pubkey::default(),
        };
        let block2 = ProposedBlock {
            parent_hash: Hash::default(),
            height: 2,
            timestamp: 1000,
            transactions: vec![],
            state_root: Hash::default(),
            proposer: Pubkey::default(),
        };
        assert_ne!(block1.hash(), block2.hash());
    }

    #[test]
    fn test_consensus_state_new() {
        let state = ConsensusState::new(42);
        assert_eq!(state.height, 42);
        assert_eq!(state.round, 0);
        assert_eq!(state.step, ConsensusStep::NewRound);
        assert!(state.locked_value.is_none());
        assert!(state.locked_round.is_none());
        assert!(state.prevotes.is_empty());
        assert!(state.precommits.is_empty());
    }

    #[test]
    fn test_consensus_state_advance_round_preserves_lock() {
        let mut state = ConsensusState::new(1);
        state.locked_value = Some(Hash::default());
        state.locked_round = Some(0);
        state.prevotes.insert(Pubkey::default(), Some(Hash::default()));

        state.advance_round(1);

        assert_eq!(state.round, 1);
        assert_eq!(state.step, ConsensusStep::NewRound);
        // Lock preserved
        assert!(state.locked_value.is_some());
        assert_eq!(state.locked_round, Some(0));
        // Votes cleared
        assert!(state.prevotes.is_empty());
        assert!(state.precommits.is_empty());
    }

    #[test]
    fn test_consensus_message_accessors() {
        let msg = ConsensusMessage::Prevote {
            height: 10,
            round: 2,
            block_hash: Some(Hash::default()),
            voter: Pubkey::default(),
            signature: Signature::default(),
        };
        assert_eq!(msg.height(), 10);
        assert_eq!(msg.round(), 2);
        assert_eq!(*msg.sender(), Pubkey::default());
    }

    #[test]
    fn test_consensus_step_display() {
        assert_eq!(format!("{}", ConsensusStep::NewRound), "NewRound");
        assert_eq!(format!("{}", ConsensusStep::Propose), "Propose");
        assert_eq!(format!("{}", ConsensusStep::Prevote), "Prevote");
        assert_eq!(format!("{}", ConsensusStep::Precommit), "Precommit");
        assert_eq!(format!("{}", ConsensusStep::Commit), "Commit");
    }
}

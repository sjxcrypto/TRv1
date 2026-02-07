//! The core BFT consensus state machine.
//!
//! Implements the Tendermint-style three-phase consensus protocol:
//! Propose → Prevote → Precommit → Commit.
//!
//! The engine is deterministic: given the same sequence of messages, it
//! will always produce the same state transitions and output messages.
//! All I/O and networking is handled externally; this module is pure
//! state-machine logic.

use {
    crate::{
        config::BftConfig,
        evidence::EvidenceCollector,
        proposer,
        types::{
            CommittedBlock, ConsensusMessage, ConsensusState, ConsensusStep,
        },
        validator_set::ValidatorSet,
    },
    log::*,
    solana_hash::Hash,
    solana_pubkey::Pubkey,
    solana_signature::Signature,
};

/// Result of processing a consensus event.
#[derive(Debug)]
pub struct EngineOutput {
    /// Messages to broadcast to the network.
    pub messages: Vec<ConsensusMessage>,
    /// If consensus was reached, the committed block.
    pub committed_block: Option<CommittedBlock>,
}

impl EngineOutput {
    fn empty() -> Self {
        Self {
            messages: Vec::new(),
            committed_block: None,
        }
    }

    fn with_messages(messages: Vec<ConsensusMessage>) -> Self {
        Self {
            messages,
            committed_block: None,
        }
    }

    fn with_commit(committed_block: CommittedBlock) -> Self {
        Self {
            messages: Vec::new(),
            committed_block: Some(committed_block),
        }
    }
}

/// The BFT consensus engine.
///
/// Processes incoming consensus messages and timeout events,
/// producing outgoing messages and committed blocks.
pub struct ConsensusEngine {
    /// Configuration parameters.
    config: BftConfig,
    /// This validator's identity.
    identity: Pubkey,
    /// The current validator set (stake-weighted).
    validator_set: ValidatorSet,
    /// Current consensus state.
    state: ConsensusState,
    /// Evidence collector for double-sign detection.
    evidence: EvidenceCollector,
    /// Whether we've already sent a prevote this round.
    sent_prevote: bool,
    /// Whether we've already sent a precommit this round.
    sent_precommit: bool,
}

impl ConsensusEngine {
    /// Create a new consensus engine.
    pub fn new(config: BftConfig, identity: Pubkey, validator_set: ValidatorSet) -> Self {
        Self {
            config,
            identity,
            validator_set,
            state: ConsensusState::new(0),
            evidence: EvidenceCollector::new(),
            sent_prevote: false,
            sent_precommit: false,
        }
    }

    // -- Public API --

    /// Begin consensus for a new height. Resets state and starts round 0.
    pub fn start_new_height(&mut self, height: u64) -> EngineOutput {
        info!("Starting consensus for height {height}");
        self.state = ConsensusState::new(height);
        self.sent_prevote = false;
        self.sent_precommit = false;
        self.evidence.prune(height.saturating_sub(100));
        self.start_round(0)
    }

    /// Process an incoming proposal message.
    pub fn on_proposal(&mut self, proposal: ConsensusMessage) -> EngineOutput {
        let ConsensusMessage::Proposal {
            height,
            round,
            ref block,
            proposer,
            signature: _,
            valid_round,
        } = proposal
        else {
            warn!("on_proposal called with non-Proposal message");
            return EngineOutput::empty();
        };

        // Ignore messages for wrong height
        if height != self.state.height {
            return EngineOutput::empty();
        }

        // Ignore messages for past rounds (but allow future rounds)
        if round < self.state.round {
            return EngineOutput::empty();
        }

        // Verify proposer is correct for this round
        let expected_proposer = proposer::proposer_for_round(&self.validator_set, height, round);
        if expected_proposer != Some(proposer) {
            warn!(
                "Invalid proposer {proposer} for height={height} round={round}, expected {:?}",
                expected_proposer
            );
            return EngineOutput::empty();
        }

        // Verify proposer is in the validator set
        if !self.validator_set.contains(&proposer) {
            return EngineOutput::empty();
        }

        // If this is for a future round, jump to it
        if round > self.state.round {
            self.state.advance_round(round);
            self.sent_prevote = false;
            self.sent_precommit = false;
        }

        // Store the proposal
        self.state.proposal = Some(block.clone());
        self.state.step = ConsensusStep::Propose;

        // Determine our prevote according to Tendermint rules
        let block_hash = block.hash();
        let our_prevote = self.determine_prevote(&block_hash, valid_round);

        // Transition to Prevote step
        self.state.step = ConsensusStep::Prevote;

        if self.sent_prevote {
            return EngineOutput::empty();
        }

        self.sent_prevote = true;
        let prevote = self.make_prevote(our_prevote);
        // Record our own prevote
        self.state.prevotes.insert(self.identity, our_prevote);

        // Check if our prevote creates a quorum (e.g., we were the last needed)
        let mut output = EngineOutput::with_messages(vec![prevote]);
        self.try_advance_from_prevotes(&mut output);
        output
    }

    /// Process an incoming prevote message.
    pub fn on_prevote(&mut self, prevote: ConsensusMessage) -> EngineOutput {
        let ConsensusMessage::Prevote {
            height,
            round,
            block_hash,
            voter,
            ..
        } = &prevote
        else {
            return EngineOutput::empty();
        };

        if *height != self.state.height {
            return EngineOutput::empty();
        }

        // Check for double-signing
        self.evidence.check_and_record(&prevote);

        // Ignore votes for rounds we've moved past
        if *round < self.state.round {
            return EngineOutput::empty();
        }

        // Verify voter is in the validator set
        if !self.validator_set.contains(voter) {
            return EngineOutput::empty();
        }

        // If this vote is for a future round and we see 2/3+ for that round,
        // we might need to skip ahead. For now, collect votes for current round.
        if *round > self.state.round {
            // TODO: track future-round votes and skip if quorum reached
            return EngineOutput::empty();
        }

        // Record the prevote
        self.state.prevotes.insert(*voter, *block_hash);

        let mut output = EngineOutput::empty();
        self.try_advance_from_prevotes(&mut output);
        output
    }

    /// Process an incoming precommit message.
    pub fn on_precommit(&mut self, precommit: ConsensusMessage) -> EngineOutput {
        let ConsensusMessage::Precommit {
            height,
            round,
            block_hash,
            voter,
            signature: _,
        } = &precommit
        else {
            return EngineOutput::empty();
        };

        if *height != self.state.height {
            return EngineOutput::empty();
        }

        // Check for double-signing
        self.evidence.check_and_record(&precommit);

        if *round < self.state.round {
            return EngineOutput::empty();
        }

        if !self.validator_set.contains(voter) {
            return EngineOutput::empty();
        }

        if *round > self.state.round {
            return EngineOutput::empty();
        }

        // Record the precommit
        self.state.precommits.insert(*voter, *block_hash);

        // Check if we have 2/3+ precommits for a block
        self.try_commit()
    }

    /// Handle a timeout event for the given step.
    pub fn on_timeout(&mut self, step: ConsensusStep) -> EngineOutput {
        match step {
            ConsensusStep::Propose => {
                // Propose timeout: we didn't receive a valid proposal.
                // Send a nil prevote.
                if self.state.step <= ConsensusStep::Propose {
                    self.state.step = ConsensusStep::Prevote;
                    if !self.sent_prevote {
                        self.sent_prevote = true;
                        let prevote = self.make_prevote(None);
                        self.state.prevotes.insert(self.identity, None);
                        return EngineOutput::with_messages(vec![prevote]);
                    }
                }
                EngineOutput::empty()
            }
            ConsensusStep::Prevote => {
                // Prevote timeout: we have 2/3+ prevotes but not for a single value.
                // Send a nil precommit.
                if self.state.step <= ConsensusStep::Prevote {
                    self.state.step = ConsensusStep::Precommit;
                    if !self.sent_precommit {
                        self.sent_precommit = true;
                        let precommit = self.make_precommit(None);
                        self.state.precommits.insert(self.identity, None);
                        return EngineOutput::with_messages(vec![precommit]);
                    }
                }
                EngineOutput::empty()
            }
            ConsensusStep::Precommit => {
                // Precommit timeout: move to next round.
                let next_round = self.state.round + 1;
                if next_round >= self.config.max_rounds_per_height {
                    warn!(
                        "Max rounds ({}) reached at height {}",
                        self.config.max_rounds_per_height, self.state.height
                    );
                    // Still advance but log the warning
                }
                self.start_round(next_round)
            }
            ConsensusStep::NewRound => {
                // Treat like propose timeout
                self.on_timeout(ConsensusStep::Propose)
            }
            ConsensusStep::Commit => {
                // Should never timeout in commit
                EngineOutput::empty()
            }
        }
    }

    /// Check if this validator is the proposer for the given height and round.
    pub fn is_proposer(&self, height: u64, round: u32) -> bool {
        proposer::is_proposer(&self.validator_set, &self.identity, height, round)
    }

    // -- Accessors --

    /// Returns a reference to the current consensus state.
    pub fn state(&self) -> &ConsensusState {
        &self.state
    }

    /// Returns a reference to the evidence collector.
    pub fn evidence(&self) -> &EvidenceCollector {
        &self.evidence
    }

    /// Returns a mutable reference to the evidence collector.
    pub fn evidence_mut(&mut self) -> &mut EvidenceCollector {
        &mut self.evidence
    }

    /// Returns the current height.
    pub fn height(&self) -> u64 {
        self.state.height
    }

    /// Returns the current round.
    pub fn round(&self) -> u32 {
        self.state.round
    }

    /// Returns the current step.
    pub fn step(&self) -> ConsensusStep {
        self.state.step
    }

    /// Returns the identity pubkey.
    pub fn identity(&self) -> &Pubkey {
        &self.identity
    }

    /// Returns a reference to the validator set.
    pub fn validator_set(&self) -> &ValidatorSet {
        &self.validator_set
    }

    /// Update the validator set (e.g., at epoch boundaries).
    pub fn update_validator_set(&mut self, validator_set: ValidatorSet) {
        self.validator_set = validator_set;
    }

    /// Returns the config.
    pub fn config(&self) -> &BftConfig {
        &self.config
    }

    // -- Internal logic --

    /// Start a new round within the current height.
    fn start_round(&mut self, round: u32) -> EngineOutput {
        info!(
            "Starting round {round} at height {}",
            self.state.height
        );
        self.state.advance_round(round);
        self.sent_prevote = false;
        self.sent_precommit = false;
        self.state.step = ConsensusStep::Propose;

        // If we are the proposer, the caller is responsible for creating
        // and broadcasting the proposal. We just signal it.
        EngineOutput::empty()
    }

    /// Determine what to prevote for, following Tendermint lock/polka rules.
    ///
    /// Rules:
    /// 1. If we're locked on a value and the proposal matches our lock → prevote for it.
    /// 2. If we're locked on a value and the proposal has valid_round >= locked_round
    ///    with a polka for the proposed value → we can unlock and prevote for it.
    /// 3. If we're not locked → prevote for the proposal if it's valid.
    /// 4. Otherwise → nil prevote.
    fn determine_prevote(
        &self,
        block_hash: &Hash,
        valid_round: Option<u32>,
    ) -> Option<Hash> {
        // If we're locked on a value
        if let Some(ref locked_hash) = self.state.locked_value {
            if locked_hash == block_hash {
                // Rule 1: proposal matches our lock
                return Some(*block_hash);
            }

            // Rule 2: check if valid_round >= locked_round (polka unlock)
            if let (Some(vr), Some(lr)) = (valid_round, self.state.locked_round) {
                if vr >= lr {
                    // The proposer claims a polka in valid_round.
                    // In a full implementation, we'd verify this claim.
                    // For now, trust the proposer's valid_round.
                    return Some(*block_hash);
                }
            }

            // Locked on a different value, nil prevote
            return None;
        }

        // Rule 3: not locked, prevote for the proposal
        // (In production, we'd validate the block here)
        Some(*block_hash)
    }

    /// Try to advance the state machine based on collected prevotes.
    fn try_advance_from_prevotes(&mut self, output: &mut EngineOutput) {
        // Check for 2/3+ prevotes for a specific hash
        let block_hash = self.find_quorum_prevote_hash();

        if let Some(hash) = block_hash {
            // We have a polka for this hash
            info!("Polka reached for hash {hash} at h={} r={}", self.state.height, self.state.round);

            // Update valid value
            self.state.valid_value = Some(hash);
            self.state.valid_round = Some(self.state.round);

            // Lock on this value
            self.state.locked_value = Some(hash);
            self.state.locked_round = Some(self.state.round);

            // Transition to precommit
            self.state.step = ConsensusStep::Precommit;

            if !self.sent_precommit {
                self.sent_precommit = true;
                let precommit = self.make_precommit(Some(hash));
                self.state.precommits.insert(self.identity, Some(hash));
                output.messages.push(precommit);
            }
        } else if self.has_any_quorum_prevotes() {
            // 2/3+ prevotes total, but not for a single value (nil polka).
            // Transition to precommit with nil.
            if self.state.step < ConsensusStep::Precommit {
                self.state.step = ConsensusStep::Precommit;
                if !self.sent_precommit {
                    self.sent_precommit = true;
                    let precommit = self.make_precommit(None);
                    self.state.precommits.insert(self.identity, None);
                    output.messages.push(precommit);
                }
            }
        }
    }

    /// Try to commit based on collected precommits.
    fn try_commit(&mut self) -> EngineOutput {
        // Check for 2/3+ precommits for a specific hash
        let commit_hash = self.find_quorum_precommit_hash();

        if let Some(hash) = commit_hash {
            // We have a commit!
            if let Some(ref proposal) = self.state.proposal {
                if proposal.hash() == hash {
                    info!("Committed block at h={} r={}", self.state.height, self.state.round);
                    self.state.step = ConsensusStep::Commit;

                    let commit_sigs: Vec<(Pubkey, Signature)> = self
                        .state
                        .precommits
                        .iter()
                        .filter(|(_, v)| *v == &Some(hash))
                        .map(|(k, _)| (*k, Signature::default())) // TODO: store actual signatures
                        .collect();

                    return EngineOutput::with_commit(CommittedBlock {
                        block: proposal.clone(),
                        commit_signatures: commit_sigs,
                        commit_round: self.state.round,
                    });
                }
            }
            // We have precommits for a hash but don't have the block.
            // In production, we'd request the block.
            warn!(
                "Have 2/3+ precommits for {hash} but missing proposal at h={} r={}",
                self.state.height, self.state.round
            );
        }

        // Check for 2/3+ nil precommits — trigger round advancement
        if self.has_quorum_nil_precommits() {
            // This effectively triggers a precommit timeout
            // The timeout handler will advance the round
        }

        EngineOutput::empty()
    }

    // -- Quorum calculations --

    /// Check if there are 2/3+ prevotes for a specific block hash.
    pub fn has_quorum_prevotes(&self, block_hash: &Option<Hash>) -> bool {
        let quorum = self.validator_set.quorum_stake(self.config.finality_threshold);
        let stake: u64 = self
            .state
            .prevotes
            .iter()
            .filter(|(_, v)| *v == block_hash)
            .map(|(k, _)| self.validator_set.stake_of(k))
            .sum();
        stake >= quorum
    }

    /// Check if there are 2/3+ precommits for a specific block hash.
    pub fn has_quorum_precommits(&self, block_hash: &Option<Hash>) -> bool {
        let quorum = self.validator_set.quorum_stake(self.config.finality_threshold);
        let stake: u64 = self
            .state
            .precommits
            .iter()
            .filter(|(_, v)| *v == block_hash)
            .map(|(k, _)| self.validator_set.stake_of(k))
            .sum();
        stake >= quorum
    }

    /// Find the block hash that has a quorum of prevotes, if any.
    fn find_quorum_prevote_hash(&self) -> Option<Hash> {
        let quorum = self.validator_set.quorum_stake(self.config.finality_threshold);

        // Group prevotes by hash
        let mut stake_by_hash: std::collections::HashMap<Option<Hash>, u64> =
            std::collections::HashMap::new();
        for (voter, hash) in &self.state.prevotes {
            let stake = self.validator_set.stake_of(voter);
            *stake_by_hash.entry(*hash).or_default() += stake;
        }

        // Find a non-nil hash with quorum
        for (hash, stake) in &stake_by_hash {
            if let Some(h) = hash {
                if *stake >= quorum {
                    return Some(*h);
                }
            }
        }
        None
    }

    /// Find the block hash that has a quorum of precommits, if any.
    fn find_quorum_precommit_hash(&self) -> Option<Hash> {
        let quorum = self.validator_set.quorum_stake(self.config.finality_threshold);

        let mut stake_by_hash: std::collections::HashMap<Option<Hash>, u64> =
            std::collections::HashMap::new();
        for (voter, hash) in &self.state.precommits {
            let stake = self.validator_set.stake_of(voter);
            *stake_by_hash.entry(*hash).or_default() += stake;
        }

        for (hash, stake) in &stake_by_hash {
            if let Some(h) = hash {
                if *stake >= quorum {
                    return Some(*h);
                }
            }
        }
        None
    }

    /// Check if total prevote stake meets quorum (regardless of which hash).
    fn has_any_quorum_prevotes(&self) -> bool {
        let quorum = self.validator_set.quorum_stake(self.config.finality_threshold);
        let total: u64 = self
            .state
            .prevotes
            .keys()
            .map(|k| self.validator_set.stake_of(k))
            .sum();
        total >= quorum
    }

    /// Check if there's a quorum of nil precommits.
    fn has_quorum_nil_precommits(&self) -> bool {
        self.has_quorum_precommits(&None)
    }

    // -- Message construction --

    fn make_prevote(&self, block_hash: Option<Hash>) -> ConsensusMessage {
        ConsensusMessage::Prevote {
            height: self.state.height,
            round: self.state.round,
            block_hash,
            voter: self.identity,
            signature: Signature::default(), // TODO: sign with keypair
        }
    }

    fn make_precommit(&self, block_hash: Option<Hash>) -> ConsensusMessage {
        ConsensusMessage::Precommit {
            height: self.state.height,
            round: self.state.round,
            block_hash,
            voter: self.identity,
            signature: Signature::default(), // TODO: sign with keypair
        }
    }
}

// Implement PartialOrd for ConsensusStep to enable step comparisons
impl PartialOrd for ConsensusStep {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ConsensusStep {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        fn step_ord(s: &ConsensusStep) -> u8 {
            match s {
                ConsensusStep::NewRound => 0,
                ConsensusStep::Propose => 1,
                ConsensusStep::Prevote => 2,
                ConsensusStep::Precommit => 3,
                ConsensusStep::Commit => 4,
            }
        }
        step_ord(self).cmp(&step_ord(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ProposedBlock;

    /// Helper: create a validator set with N validators of equal stake.
    fn make_validator_set(n: usize, stake: u64) -> (Vec<Pubkey>, ValidatorSet) {
        let pks: Vec<Pubkey> = (0..n).map(|_| Pubkey::new_unique()).collect();
        let vs = ValidatorSet::new(pks.iter().map(|pk| (*pk, stake)).collect());
        (pks, vs)
    }

    /// Helper: create a simple proposed block.
    fn make_block(height: u64, proposer: Pubkey) -> ProposedBlock {
        ProposedBlock {
            parent_hash: Hash::default(),
            height,
            timestamp: 1000,
            transactions: vec![],
            state_root: Hash::new_unique(),
            proposer,
        }
    }

    /// Helper: create a proposal message.
    fn make_proposal(
        height: u64,
        round: u32,
        block: &ProposedBlock,
        proposer: Pubkey,
        valid_round: Option<u32>,
    ) -> ConsensusMessage {
        ConsensusMessage::Proposal {
            height,
            round,
            block: block.clone(),
            proposer,
            signature: Signature::default(),
            valid_round,
        }
    }

    fn make_prevote_msg(
        height: u64,
        round: u32,
        block_hash: Option<Hash>,
        voter: Pubkey,
    ) -> ConsensusMessage {
        ConsensusMessage::Prevote {
            height,
            round,
            block_hash,
            voter,
            signature: Signature::default(),
        }
    }

    fn make_precommit_msg(
        height: u64,
        round: u32,
        block_hash: Option<Hash>,
        voter: Pubkey,
    ) -> ConsensusMessage {
        ConsensusMessage::Precommit {
            height,
            round,
            block_hash,
            voter,
            signature: Signature::default(),
        }
    }

    // ============================
    // Happy path: full round
    // ============================

    #[test]
    fn test_full_round_happy_path() {
        // 4 validators with equal stake (need 3 for 2/3+)
        let (pks, vs) = make_validator_set(4, 100);
        let config = BftConfig::default();

        // Use the first validator as our identity
        let our_pk = pks[0];
        let mut engine = ConsensusEngine::new(config, our_pk, vs.clone());

        // Start height 1
        let _output = engine.start_new_height(1);
        assert_eq!(engine.height(), 1);
        assert_eq!(engine.round(), 0);

        // Find who the proposer is
        let proposer_pk = proposer::proposer_for_round(&vs, 1, 0).unwrap();
        let block = make_block(1, proposer_pk);
        let block_hash = block.hash();

        // Send the proposal
        let proposal = make_proposal(1, 0, &block, proposer_pk, None);
        let output = engine.on_proposal(proposal);

        // Engine should emit a prevote for the block
        assert!(!output.messages.is_empty());
        let prevote = &output.messages[0];
        match prevote {
            ConsensusMessage::Prevote { block_hash: h, .. } => {
                assert_eq!(*h, Some(block_hash));
            }
            _ => panic!("Expected prevote"),
        }

        // Collect prevotes from other validators
        // We need 3 out of 4 for quorum (including ours, which was already recorded)
        let mut precommit_found = false;
        for pk in &pks[1..] {
            let prevote = make_prevote_msg(1, 0, Some(block_hash), *pk);
            let output = engine.on_prevote(prevote);

            // At some point, we should see a precommit
            for msg in &output.messages {
                if matches!(msg, ConsensusMessage::Precommit { .. }) {
                    precommit_found = true;
                }
            }
        }

        assert!(
            precommit_found,
            "Engine should have sent a precommit after seeing quorum prevotes"
        );

        // Now send precommits from other validators
        let mut committed = false;
        for pk in &pks[1..] {
            let precommit = make_precommit_msg(1, 0, Some(block_hash), *pk);
            let output = engine.on_precommit(precommit);
            if output.committed_block.is_some() {
                committed = true;
                let cb = output.committed_block.unwrap();
                assert_eq!(cb.block.height, 1);
                assert_eq!(cb.commit_round, 0);
                break;
            }
        }

        assert!(committed, "Block should have been committed");
        assert_eq!(engine.step(), ConsensusStep::Commit);
    }

    // ============================
    // Timeout escalation
    // ============================

    #[test]
    fn test_propose_timeout_sends_nil_prevote() {
        let (pks, vs) = make_validator_set(4, 100);
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs);
        engine.start_new_height(1);

        // Timeout on propose → nil prevote
        let output = engine.on_timeout(ConsensusStep::Propose);
        assert_eq!(output.messages.len(), 1);
        match &output.messages[0] {
            ConsensusMessage::Prevote { block_hash, .. } => {
                assert!(block_hash.is_none(), "Should be nil prevote");
            }
            _ => panic!("Expected prevote"),
        }
        assert_eq!(engine.step(), ConsensusStep::Prevote);
    }

    #[test]
    fn test_prevote_timeout_sends_nil_precommit() {
        let (pks, vs) = make_validator_set(4, 100);
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs);
        engine.start_new_height(1);

        // First, get past propose
        engine.on_timeout(ConsensusStep::Propose);

        // Timeout on prevote → nil precommit
        let output = engine.on_timeout(ConsensusStep::Prevote);
        assert_eq!(output.messages.len(), 1);
        match &output.messages[0] {
            ConsensusMessage::Precommit { block_hash, .. } => {
                assert!(block_hash.is_none(), "Should be nil precommit");
            }
            _ => panic!("Expected precommit"),
        }
        assert_eq!(engine.step(), ConsensusStep::Precommit);
    }

    #[test]
    fn test_precommit_timeout_advances_round() {
        let (pks, vs) = make_validator_set(4, 100);
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs);
        engine.start_new_height(1);

        // Timeout through all phases
        engine.on_timeout(ConsensusStep::Propose);
        engine.on_timeout(ConsensusStep::Prevote);
        let _output = engine.on_timeout(ConsensusStep::Precommit);

        // Should be in round 1 now
        assert_eq!(engine.round(), 1);
        assert_eq!(engine.step(), ConsensusStep::Propose);
    }

    #[test]
    fn test_multiple_round_escalation() {
        let (pks, vs) = make_validator_set(4, 100);
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs);
        engine.start_new_height(1);

        // Escalate through 3 rounds
        for expected_round in 0..3u32 {
            assert_eq!(engine.round(), expected_round);
            engine.on_timeout(ConsensusStep::Propose);
            engine.on_timeout(ConsensusStep::Prevote);
            engine.on_timeout(ConsensusStep::Precommit);
        }
        assert_eq!(engine.round(), 3);
    }

    // ============================
    // Lock/unlock rules
    // ============================

    #[test]
    fn test_lock_on_polka() {
        let (pks, vs) = make_validator_set(4, 100);
        let proposer_pk = proposer::proposer_for_round(&vs, 1, 0).unwrap();
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs.clone());
        engine.start_new_height(1);

        let block = make_block(1, proposer_pk);
        let block_hash = block.hash();

        // Receive proposal
        let proposal = make_proposal(1, 0, &block, proposer_pk, None);
        engine.on_proposal(proposal);

        // Send 2/3+ prevotes
        for pk in &pks[1..3] {
            engine.on_prevote(make_prevote_msg(1, 0, Some(block_hash), *pk));
        }

        // Engine should now be locked on block_hash
        assert_eq!(engine.state().locked_value, Some(block_hash));
        assert_eq!(engine.state().locked_round, Some(0));
    }

    #[test]
    fn test_locked_validator_prevotes_for_locked_value() {
        let (pks, vs) = make_validator_set(4, 100);
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs.clone());
        engine.start_new_height(1);

        // Manually set a lock
        let locked_hash = Hash::new_unique();
        engine.state.locked_value = Some(locked_hash);
        engine.state.locked_round = Some(0);

        // Advance to round 1
        engine.state.advance_round(1);
        engine.sent_prevote = false;
        engine.sent_precommit = false;
        engine.state.step = ConsensusStep::Propose;

        // Proposer for round 1 proposes a block
        let proposer_pk = proposer::proposer_for_round(&vs, 1, 1).unwrap();
        let block = make_block(1, proposer_pk);
        let proposal = make_proposal(1, 1, &block, proposer_pk, None);
        let output = engine.on_proposal(proposal);

        // If the block hash != locked_hash, the engine should prevote nil
        // (because we're locked and proposal doesn't match)
        if block.hash() != locked_hash {
            assert!(!output.messages.is_empty());
            match &output.messages[0] {
                ConsensusMessage::Prevote { block_hash, .. } => {
                    assert!(
                        block_hash.is_none(),
                        "Locked validator should nil-prevote for non-matching proposal"
                    );
                }
                _ => panic!("Expected prevote"),
            }
        }
    }

    #[test]
    fn test_unlock_with_valid_round() {
        let (pks, vs) = make_validator_set(4, 100);
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs.clone());
        engine.start_new_height(1);

        // Lock on something in round 0
        let old_hash = Hash::new_unique();
        engine.state.locked_value = Some(old_hash);
        engine.state.locked_round = Some(0);

        // Advance to round 2
        engine.state.advance_round(2);
        engine.sent_prevote = false;
        engine.sent_precommit = false;
        engine.state.step = ConsensusStep::Propose;

        // Proposer for round 2 proposes a different block with valid_round=1 >= locked_round=0
        let proposer_pk = proposer::proposer_for_round(&vs, 1, 2).unwrap();
        let block = make_block(1, proposer_pk);
        let proposal = make_proposal(1, 2, &block, proposer_pk, Some(1));
        let output = engine.on_proposal(proposal);

        // Should prevote for the new block (unlocked due to valid_round >= locked_round)
        assert!(!output.messages.is_empty());
        match &output.messages[0] {
            ConsensusMessage::Prevote { block_hash, .. } => {
                assert_eq!(
                    *block_hash,
                    Some(block.hash()),
                    "Should unlock and prevote for new block"
                );
            }
            _ => panic!("Expected prevote"),
        }
    }

    // ============================
    // Edge cases
    // ============================

    #[test]
    fn test_ignore_wrong_height() {
        let (pks, vs) = make_validator_set(4, 100);
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs.clone());
        engine.start_new_height(5);

        let proposer_pk = proposer::proposer_for_round(&vs, 3, 0).unwrap();
        let block = make_block(3, proposer_pk);
        let proposal = make_proposal(3, 0, &block, proposer_pk, None);
        let output = engine.on_proposal(proposal);

        // Should ignore — wrong height
        assert!(output.messages.is_empty());
    }

    #[test]
    fn test_ignore_past_round() {
        let (pks, vs) = make_validator_set(4, 100);
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs.clone());
        engine.start_new_height(1);

        // Advance to round 2
        engine.on_timeout(ConsensusStep::Propose);
        engine.on_timeout(ConsensusStep::Prevote);
        engine.on_timeout(ConsensusStep::Precommit);
        engine.on_timeout(ConsensusStep::Propose);
        engine.on_timeout(ConsensusStep::Prevote);
        engine.on_timeout(ConsensusStep::Precommit);
        assert_eq!(engine.round(), 2);

        // Send a prevote for round 0 — should be ignored
        let prevote = make_prevote_msg(1, 0, Some(Hash::new_unique()), pks[1]);
        let output = engine.on_prevote(prevote);
        assert!(output.messages.is_empty());
    }

    #[test]
    fn test_ignore_invalid_proposer() {
        let (pks, vs) = make_validator_set(4, 100);
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs.clone());
        engine.start_new_height(1);

        // Find who is NOT the proposer
        let correct_proposer = proposer::proposer_for_round(&vs, 1, 0).unwrap();
        let wrong_proposer = pks.iter().find(|p| **p != correct_proposer).unwrap();

        let block = make_block(1, *wrong_proposer);
        let proposal = make_proposal(1, 0, &block, *wrong_proposer, None);
        let output = engine.on_proposal(proposal);

        assert!(
            output.messages.is_empty(),
            "Should reject proposal from wrong proposer"
        );
    }

    #[test]
    fn test_ignore_unknown_voter() {
        let (pks, vs) = make_validator_set(4, 100);
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs);
        engine.start_new_height(1);

        let unknown = Pubkey::new_unique();
        let prevote = make_prevote_msg(1, 0, Some(Hash::new_unique()), unknown);
        let output = engine.on_prevote(prevote);
        assert!(output.messages.is_empty());
    }

    #[test]
    fn test_future_round_proposal() {
        let (pks, vs) = make_validator_set(4, 100);
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs.clone());
        engine.start_new_height(1);

        // Send a proposal for round 3 (we're in round 0)
        let proposer_pk = proposer::proposer_for_round(&vs, 1, 3).unwrap();
        let block = make_block(1, proposer_pk);
        let proposal = make_proposal(1, 3, &block, proposer_pk, None);
        let output = engine.on_proposal(proposal);

        // Engine should jump to round 3 and process
        assert_eq!(engine.round(), 3);
        assert!(!output.messages.is_empty()); // Should emit prevote
    }

    #[test]
    fn test_commit_timeout_is_noop() {
        let (pks, vs) = make_validator_set(4, 100);
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs);
        engine.start_new_height(1);

        let output = engine.on_timeout(ConsensusStep::Commit);
        assert!(output.messages.is_empty());
        assert!(output.committed_block.is_none());
    }

    #[test]
    fn test_step_ordering() {
        assert!(ConsensusStep::NewRound < ConsensusStep::Propose);
        assert!(ConsensusStep::Propose < ConsensusStep::Prevote);
        assert!(ConsensusStep::Prevote < ConsensusStep::Precommit);
        assert!(ConsensusStep::Precommit < ConsensusStep::Commit);
    }

    #[test]
    fn test_is_proposer() {
        let (_pks, vs) = make_validator_set(4, 100);
        let proposer_pk = proposer::proposer_for_round(&vs, 1, 0).unwrap();
        let engine = ConsensusEngine::new(BftConfig::default(), proposer_pk, vs);
        assert!(engine.is_proposer(1, 0));
    }

    #[test]
    fn test_double_prevote_no_duplicate_message() {
        // Ensure the engine doesn't send two prevotes in the same round
        let (pks, vs) = make_validator_set(4, 100);
        let mut engine = ConsensusEngine::new(BftConfig::default(), pks[0], vs.clone());
        engine.start_new_height(1);

        // Timeout on propose → nil prevote
        let output1 = engine.on_timeout(ConsensusStep::Propose);
        assert_eq!(output1.messages.len(), 1); // one prevote

        // Timeout on propose again — no duplicate
        let output2 = engine.on_timeout(ConsensusStep::Propose);
        assert!(output2.messages.is_empty());
    }
}

//! Double-sign detection and evidence collection.
//!
//! Validators must not cast conflicting votes (two different prevotes or
//! precommits for the same height+round). This module detects and records
//! such violations for later slashing.

use {
    crate::types::ConsensusMessage,
    solana_hash::Hash,
    solana_pubkey::Pubkey,
    solana_signature::Signature,
    std::collections::HashMap,
};

/// Evidence of a validator double-signing: casting two conflicting votes
/// at the same (height, round, step).
#[derive(Debug, Clone)]
pub struct DoubleSignEvidence {
    /// The offending validator.
    pub validator: Pubkey,
    /// Block height at which the offense occurred.
    pub height: u64,
    /// Round at which the offense occurred.
    pub round: u32,
    /// The type of conflicting messages.
    pub kind: EvidenceKind,
    /// First vote (hash voted for, signature).
    pub vote_a: (Option<Hash>, Signature),
    /// Second conflicting vote (hash voted for, signature).
    pub vote_b: (Option<Hash>, Signature),
}

/// The type of double-sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    /// Two different prevotes in the same round.
    ConflictingPrevote,
    /// Two different precommits in the same round.
    ConflictingPrecommit,
}

impl std::fmt::Display for EvidenceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvidenceKind::ConflictingPrevote => write!(f, "ConflictingPrevote"),
            EvidenceKind::ConflictingPrecommit => write!(f, "ConflictingPrecommit"),
        }
    }
}

/// Key for tracking votes: (height, round, voter, vote_type).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct VoteKey {
    height: u64,
    round: u32,
    voter: Pubkey,
    kind: VoteType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum VoteType {
    Prevote,
    Precommit,
}

/// Stored vote: the hash that was voted for and the signature.
#[derive(Debug, Clone)]
struct StoredVote {
    block_hash: Option<Hash>,
    signature: Signature,
}

/// Collects and detects double-sign evidence.
///
/// Maintains a sliding window of votes to detect conflicting votes
/// from the same validator in the same (height, round).
pub struct EvidenceCollector {
    /// Map from vote key to the first vote seen.
    votes: HashMap<VoteKey, StoredVote>,
    /// Collected evidence of double-signing.
    evidence: Vec<DoubleSignEvidence>,
    /// Minimum height to track (older votes are pruned).
    min_height: u64,
}

impl EvidenceCollector {
    /// Create a new evidence collector.
    pub fn new() -> Self {
        Self {
            votes: HashMap::new(),
            evidence: Vec::new(),
            min_height: 0,
        }
    }

    /// Process a consensus message and check for double-signing.
    /// Returns `Some(evidence)` if a double-sign is detected, `None` otherwise.
    pub fn check_and_record(&mut self, msg: &ConsensusMessage) -> Option<DoubleSignEvidence> {
        let (key, current_vote) = match msg {
            ConsensusMessage::Prevote {
                height,
                round,
                block_hash,
                voter,
                signature,
            } => (
                VoteKey {
                    height: *height,
                    round: *round,
                    voter: *voter,
                    kind: VoteType::Prevote,
                },
                StoredVote {
                    block_hash: *block_hash,
                    signature: *signature,
                },
            ),
            ConsensusMessage::Precommit {
                height,
                round,
                block_hash,
                voter,
                signature,
            } => (
                VoteKey {
                    height: *height,
                    round: *round,
                    voter: *voter,
                    kind: VoteType::Precommit,
                },
                StoredVote {
                    block_hash: *block_hash,
                    signature: *signature,
                },
            ),
            // Proposals don't count as votes for double-sign purposes
            ConsensusMessage::Proposal { .. } => return None,
        };

        // Skip if below our tracking window
        if key.height < self.min_height {
            return None;
        }

        if let Some(existing) = self.votes.get(&key) {
            // Check if this is a conflicting vote
            if existing.block_hash != current_vote.block_hash {
                let evidence_kind = match key.kind {
                    VoteType::Prevote => EvidenceKind::ConflictingPrevote,
                    VoteType::Precommit => EvidenceKind::ConflictingPrecommit,
                };
                let ev = DoubleSignEvidence {
                    validator: key.voter,
                    height: key.height,
                    round: key.round,
                    kind: evidence_kind,
                    vote_a: (existing.block_hash, existing.signature),
                    vote_b: (current_vote.block_hash, current_vote.signature),
                };
                self.evidence.push(ev.clone());
                return Some(ev);
            }
            // Same vote (duplicate) — not evidence
            return None;
        }

        // First vote from this validator at this (height, round, type)
        self.votes.insert(key, current_vote);
        None
    }

    /// Prune votes older than the given height to bound memory.
    pub fn prune(&mut self, min_height: u64) {
        self.min_height = min_height;
        self.votes.retain(|k, _| k.height >= min_height);
        // Keep evidence — it needs to be submitted for slashing
    }

    /// Returns all collected evidence.
    pub fn evidence(&self) -> &[DoubleSignEvidence] {
        &self.evidence
    }

    /// Drain all evidence (for submitting to the slashing module).
    pub fn drain_evidence(&mut self) -> Vec<DoubleSignEvidence> {
        std::mem::take(&mut self.evidence)
    }

    /// Returns the number of tracked votes.
    pub fn tracked_votes(&self) -> usize {
        self.votes.len()
    }

    /// Check if a specific validator has any evidence against them.
    pub fn has_evidence_against(&self, validator: &Pubkey) -> bool {
        self.evidence.iter().any(|e| e.validator == *validator)
    }
}

impl Default for EvidenceCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_prevote(height: u64, round: u32, hash: Option<Hash>, voter: Pubkey) -> ConsensusMessage {
        ConsensusMessage::Prevote {
            height,
            round,
            block_hash: hash,
            voter,
            signature: Signature::default(),
        }
    }

    fn make_precommit(height: u64, round: u32, hash: Option<Hash>, voter: Pubkey) -> ConsensusMessage {
        ConsensusMessage::Precommit {
            height,
            round,
            block_hash: hash,
            voter,
            signature: Signature::default(),
        }
    }

    #[test]
    fn test_no_double_sign_same_vote() {
        let mut collector = EvidenceCollector::new();
        let voter = Pubkey::new_unique();
        let hash = Some(Hash::new_unique());

        let msg1 = make_prevote(1, 0, hash, voter);
        let msg2 = make_prevote(1, 0, hash, voter);

        assert!(collector.check_and_record(&msg1).is_none());
        assert!(collector.check_and_record(&msg2).is_none()); // duplicate, not conflicting
        assert!(collector.evidence().is_empty());
    }

    #[test]
    fn test_double_sign_prevote() {
        let mut collector = EvidenceCollector::new();
        let voter = Pubkey::new_unique();
        let hash_a = Some(Hash::new_unique());
        let hash_b = Some(Hash::new_unique());

        let msg1 = make_prevote(1, 0, hash_a, voter);
        let msg2 = make_prevote(1, 0, hash_b, voter);

        assert!(collector.check_and_record(&msg1).is_none());
        let evidence = collector.check_and_record(&msg2);
        assert!(evidence.is_some());

        let ev = evidence.unwrap();
        assert_eq!(ev.validator, voter);
        assert_eq!(ev.height, 1);
        assert_eq!(ev.round, 0);
        assert_eq!(ev.kind, EvidenceKind::ConflictingPrevote);
        assert_eq!(ev.vote_a.0, hash_a);
        assert_eq!(ev.vote_b.0, hash_b);
    }

    #[test]
    fn test_double_sign_precommit() {
        let mut collector = EvidenceCollector::new();
        let voter = Pubkey::new_unique();
        let hash_a = Some(Hash::new_unique());
        let hash_b = Some(Hash::new_unique());

        let msg1 = make_precommit(1, 0, hash_a, voter);
        let msg2 = make_precommit(1, 0, hash_b, voter);

        assert!(collector.check_and_record(&msg1).is_none());
        let evidence = collector.check_and_record(&msg2);
        assert!(evidence.is_some());
        assert_eq!(evidence.unwrap().kind, EvidenceKind::ConflictingPrecommit);
    }

    #[test]
    fn test_no_double_sign_different_rounds() {
        let mut collector = EvidenceCollector::new();
        let voter = Pubkey::new_unique();
        let hash_a = Some(Hash::new_unique());
        let hash_b = Some(Hash::new_unique());

        let msg1 = make_prevote(1, 0, hash_a, voter);
        let msg2 = make_prevote(1, 1, hash_b, voter); // different round

        assert!(collector.check_and_record(&msg1).is_none());
        assert!(collector.check_and_record(&msg2).is_none());
        assert!(collector.evidence().is_empty());
    }

    #[test]
    fn test_no_double_sign_different_heights() {
        let mut collector = EvidenceCollector::new();
        let voter = Pubkey::new_unique();
        let hash_a = Some(Hash::new_unique());
        let hash_b = Some(Hash::new_unique());

        let msg1 = make_prevote(1, 0, hash_a, voter);
        let msg2 = make_prevote(2, 0, hash_b, voter); // different height

        assert!(collector.check_and_record(&msg1).is_none());
        assert!(collector.check_and_record(&msg2).is_none());
        assert!(collector.evidence().is_empty());
    }

    #[test]
    fn test_no_double_sign_different_voters() {
        let mut collector = EvidenceCollector::new();
        let voter_a = Pubkey::new_unique();
        let voter_b = Pubkey::new_unique();
        let hash_a = Some(Hash::new_unique());
        let hash_b = Some(Hash::new_unique());

        let msg1 = make_prevote(1, 0, hash_a, voter_a);
        let msg2 = make_prevote(1, 0, hash_b, voter_b);

        assert!(collector.check_and_record(&msg1).is_none());
        assert!(collector.check_and_record(&msg2).is_none());
        assert!(collector.evidence().is_empty());
    }

    #[test]
    fn test_double_sign_nil_vs_value() {
        let mut collector = EvidenceCollector::new();
        let voter = Pubkey::new_unique();
        let hash = Some(Hash::new_unique());

        let msg1 = make_prevote(1, 0, None, voter); // nil
        let msg2 = make_prevote(1, 0, hash, voter); // value

        assert!(collector.check_and_record(&msg1).is_none());
        let evidence = collector.check_and_record(&msg2);
        assert!(evidence.is_some());
    }

    #[test]
    fn test_prune_removes_old_votes() {
        let mut collector = EvidenceCollector::new();
        let voter = Pubkey::new_unique();

        let msg = make_prevote(5, 0, Some(Hash::new_unique()), voter);
        collector.check_and_record(&msg);
        assert_eq!(collector.tracked_votes(), 1);

        collector.prune(10);
        assert_eq!(collector.tracked_votes(), 0);

        // New votes below min_height are ignored
        let msg = make_prevote(8, 0, Some(Hash::new_unique()), voter);
        assert!(collector.check_and_record(&msg).is_none());
        assert_eq!(collector.tracked_votes(), 0);
    }

    #[test]
    fn test_drain_evidence() {
        let mut collector = EvidenceCollector::new();
        let voter = Pubkey::new_unique();

        let msg1 = make_prevote(1, 0, Some(Hash::new_unique()), voter);
        let msg2 = make_prevote(1, 0, Some(Hash::new_unique()), voter);
        collector.check_and_record(&msg1);
        collector.check_and_record(&msg2);

        let drained = collector.drain_evidence();
        assert_eq!(drained.len(), 1);
        assert!(collector.evidence().is_empty());
    }

    #[test]
    fn test_has_evidence_against() {
        let mut collector = EvidenceCollector::new();
        let bad_voter = Pubkey::new_unique();
        let good_voter = Pubkey::new_unique();

        let msg1 = make_prevote(1, 0, Some(Hash::new_unique()), bad_voter);
        let msg2 = make_prevote(1, 0, Some(Hash::new_unique()), bad_voter);
        collector.check_and_record(&msg1);
        collector.check_and_record(&msg2);

        assert!(collector.has_evidence_against(&bad_voter));
        assert!(!collector.has_evidence_against(&good_voter));
    }

    #[test]
    fn test_proposal_not_tracked() {
        use crate::types::ProposedBlock;

        let mut collector = EvidenceCollector::new();
        let msg = ConsensusMessage::Proposal {
            height: 1,
            round: 0,
            block: ProposedBlock {
                parent_hash: Hash::default(),
                height: 1,
                timestamp: 1000,
                transactions: vec![],
                state_root: Hash::default(),
                proposer: Pubkey::default(),
            },
            proposer: Pubkey::default(),
            signature: Signature::default(),
            valid_round: None,
        };
        assert!(collector.check_and_record(&msg).is_none());
        assert_eq!(collector.tracked_votes(), 0);
    }
}

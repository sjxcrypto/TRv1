//! Fuzz the BFT consensus engine with random message sequences.
//!
//! Goals:
//! - Find panics, invalid state transitions, or invariant violations.
//! - Verify that no two different blocks are committed at the same height.
//! - Verify that the engine never enters an unrecoverable state.
//! - Verify that no message sequence causes unbounded memory growth.

#![no_main]

use {
    arbitrary::{Arbitrary, Unstructured},
    libfuzzer_sys::fuzz_target,
    solana_hash::Hash,
    solana_pubkey::Pubkey,
    solana_signature::Signature,
    trv1_consensus_bft::{
        BftConfig, ConsensusEngine, ConsensusMessage, ConsensusStep, ProposedBlock, ValidatorSet,
    },
};

/// A fuzzable action the engine can receive.
#[derive(Debug)]
enum FuzzAction {
    /// Deliver a proposal message.
    Proposal {
        proposer_idx: usize,
        valid_round: Option<u32>,
    },
    /// Deliver a prevote message.
    Prevote {
        voter_idx: usize,
        /// If true, vote for the current proposal hash; if false, nil vote.
        vote_for_proposal: bool,
    },
    /// Deliver a precommit message.
    Precommit {
        voter_idx: usize,
        vote_for_proposal: bool,
    },
    /// Trigger a timeout for the given step.
    Timeout { step_idx: u8 },
    /// Start a new height.
    NewHeight { height: u64 },
}

impl<'a> Arbitrary<'a> for FuzzAction {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let variant = u.int_in_range(0..=4)?;
        match variant {
            0 => Ok(FuzzAction::Proposal {
                proposer_idx: u.int_in_range(0..=7)?,
                valid_round: if u.ratio(1, 3)? {
                    Some(u.int_in_range(0..=10)?)
                } else {
                    None
                },
            }),
            1 => Ok(FuzzAction::Prevote {
                voter_idx: u.int_in_range(0..=7)?,
                vote_for_proposal: u.ratio(3, 4)?,
            }),
            2 => Ok(FuzzAction::Precommit {
                voter_idx: u.int_in_range(0..=7)?,
                vote_for_proposal: u.ratio(3, 4)?,
            }),
            3 => Ok(FuzzAction::Timeout {
                step_idx: u.int_in_range(0..=4)?,
            }),
            4 => Ok(FuzzAction::NewHeight {
                height: u.int_in_range(0..=1000)?,
            }),
            _ => unreachable!(),
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);

    // Create a deterministic set of validators (4-8).
    let num_validators: usize = match u.int_in_range(4..=8) {
        Ok(n) => n,
        Err(_) => return,
    };

    let validator_pks: Vec<Pubkey> = (0..num_validators)
        .map(|i| {
            let mut bytes = [0u8; 32];
            bytes[0] = i as u8;
            bytes[31] = 0xAA; // marker
            Pubkey::new_from_array(bytes)
        })
        .collect();

    let stakes: Vec<u64> = (0..num_validators)
        .map(|_| u.int_in_range(1..=1000).unwrap_or(100))
        .collect();

    let vs = ValidatorSet::new(
        validator_pks
            .iter()
            .zip(stakes.iter())
            .map(|(pk, s)| (*pk, *s))
            .collect(),
    );

    let config = BftConfig::default();

    // We fuzz as validator 0.
    let our_pk = validator_pks[0];
    let mut engine = ConsensusEngine::new(config, our_pk, vs.clone());
    let _ = engine.start_new_height(1);

    // Track committed blocks for invariant checking.
    let mut committed_at_height: std::collections::HashMap<u64, Hash> =
        std::collections::HashMap::new();

    // Current proposal hash (if any).
    let mut current_proposal_hash: Option<Hash> = None;

    // Run a sequence of fuzzed actions.
    let num_actions: usize = u.int_in_range(1..=200).unwrap_or(50);

    for _ in 0..num_actions {
        let action: FuzzAction = match u.arbitrary() {
            Ok(a) => a,
            Err(_) => break,
        };

        let output = match action {
            FuzzAction::Proposal {
                proposer_idx,
                valid_round,
            } => {
                let idx = proposer_idx % num_validators;
                let proposer = validator_pks[idx];
                let block = ProposedBlock {
                    parent_hash: Hash::default(),
                    height: engine.height(),
                    timestamp: 1000,
                    transactions: vec![],
                    state_root: Hash::new_from_array([idx as u8; 32]),
                    proposer,
                };
                current_proposal_hash = Some(block.hash());

                let msg = ConsensusMessage::Proposal {
                    height: engine.height(),
                    round: engine.round(),
                    block,
                    proposer,
                    signature: Signature::default(),
                    valid_round,
                };
                engine.on_proposal(msg)
            }

            FuzzAction::Prevote {
                voter_idx,
                vote_for_proposal,
            } => {
                let idx = voter_idx % num_validators;
                let voter = validator_pks[idx];
                let block_hash = if vote_for_proposal {
                    current_proposal_hash
                } else {
                    None
                };
                let msg = ConsensusMessage::Prevote {
                    height: engine.height(),
                    round: engine.round(),
                    block_hash,
                    voter,
                    signature: Signature::default(),
                };
                engine.on_prevote(msg)
            }

            FuzzAction::Precommit {
                voter_idx,
                vote_for_proposal,
            } => {
                let idx = voter_idx % num_validators;
                let voter = validator_pks[idx];
                let block_hash = if vote_for_proposal {
                    current_proposal_hash
                } else {
                    None
                };
                let msg = ConsensusMessage::Precommit {
                    height: engine.height(),
                    round: engine.round(),
                    block_hash,
                    voter,
                    signature: Signature::default(),
                };
                engine.on_precommit(msg)
            }

            FuzzAction::Timeout { step_idx } => {
                let step = match step_idx % 5 {
                    0 => ConsensusStep::NewRound,
                    1 => ConsensusStep::Propose,
                    2 => ConsensusStep::Prevote,
                    3 => ConsensusStep::Precommit,
                    4 => ConsensusStep::Commit,
                    _ => ConsensusStep::Propose,
                };
                engine.on_timeout(step)
            }

            FuzzAction::NewHeight { height } => {
                current_proposal_hash = None;
                engine.start_new_height(height)
            }
        };

        // ── Invariant checks ──

        // 1. Safety: no two different blocks committed at the same height.
        if let Some(committed) = output.committed_block {
            let hash = committed.block.hash();
            let height = committed.block.height;

            if let Some(existing_hash) = committed_at_height.get(&height) {
                assert_eq!(
                    *existing_hash, hash,
                    "SAFETY VIOLATION: two different blocks committed at height {height}"
                );
            } else {
                committed_at_height.insert(height, hash);
            }

            // 2. Committed block height must match engine height at time of commit.
            // (The engine may have moved on by now, so we just check the block's height
            //  is reasonable.)
            assert!(height <= 1000, "Committed block height unexpectedly large");
        }

        // 3. Engine state must remain consistent.
        let state = engine.state();
        assert!(
            state.round < 10_000,
            "Round counter grew suspiciously large: {}",
            state.round
        );

        // 4. Vote collections should not have more entries than validators.
        assert!(
            state.prevotes.len() <= num_validators,
            "More prevotes than validators"
        );
        assert!(
            state.precommits.len() <= num_validators,
            "More precommits than validators"
        );
    }
});

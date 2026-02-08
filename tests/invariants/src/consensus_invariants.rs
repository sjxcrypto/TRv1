//! Property-based tests for BFT consensus invariants.
//!
//! Properties tested:
//! 1. Safety: no two different blocks committed at the same height.
//! 2. Liveness: if 2/3+ honest validators, eventually commits.
//! 3. Validity: only proposed blocks can be committed.

#[cfg(test)]
mod tests {
    use {
        proptest::prelude::*,
        solana_hash::Hash,
        solana_pubkey::Pubkey,
        solana_signature::Signature,
        trv1_consensus_bft::{
            BftConfig, ConsensusEngine, ConsensusMessage, ProposedBlock,
            ValidatorSet, proposer_for_round,
        },
    };

    // ── Helpers ──

    fn make_validator_set(n: usize, stakes: &[u64]) -> (Vec<Pubkey>, ValidatorSet) {
        let pks: Vec<Pubkey> = (0..n)
            .map(|i| {
                let mut bytes = [0u8; 32];
                bytes[0] = i as u8;
                bytes[31] = 0xBB;
                Pubkey::new_from_array(bytes)
            })
            .collect();
        let vs = ValidatorSet::new(
            pks.iter()
                .zip(stakes.iter())
                .map(|(pk, s)| (*pk, *s))
                .collect(),
        );
        (pks, vs)
    }

    fn make_block(height: u64, proposer: Pubkey) -> ProposedBlock {
        // Use a deterministic state_root derived from height + proposer so all
        // engines produce the same block hash for the same (height, round).
        let mut state_root_bytes = [0u8; 32];
        state_root_bytes[..8].copy_from_slice(&height.to_le_bytes());
        state_root_bytes[8..32].copy_from_slice(&proposer.to_bytes()[..24]);
        ProposedBlock {
            parent_hash: Hash::default(),
            height,
            timestamp: 1000,
            transactions: vec![],
            state_root: Hash::new_from_array(state_root_bytes),
            proposer,
        }
    }

    fn make_proposal(
        height: u64,
        round: u32,
        block: &ProposedBlock,
        proposer: Pubkey,
    ) -> ConsensusMessage {
        ConsensusMessage::Proposal {
            height,
            round,
            block: block.clone(),
            proposer,
            signature: Signature::default(),
            valid_round: None,
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

    /// Run a complete happy-path round and return the committed block hash (if any).
    fn run_honest_round(
        engine: &mut ConsensusEngine,
        all_pks: &[Pubkey],
        vs: &ValidatorSet,
        height: u64,
        round: u32,
    ) -> Option<Hash> {
        let proposer_pk = match proposer_for_round(vs, height, round) {
            Some(pk) => pk,
            None => return None,
        };

        let block = make_block(height, proposer_pk);
        let block_hash = block.hash();

        // Deliver proposal.
        let proposal = make_proposal(height, round, &block, proposer_pk);
        let _ = engine.on_proposal(proposal);

        // Deliver prevotes from all validators (simulating honest network).
        for pk in all_pks {
            if *pk == engine.identity().clone() {
                continue; // Engine already voted.
            }
            let _ = engine.on_prevote(make_prevote_msg(height, round, Some(block_hash), *pk));
        }

        // Deliver precommits from all validators.
        for pk in all_pks {
            if *pk == engine.identity().clone() {
                continue;
            }
            let output =
                engine.on_precommit(make_precommit_msg(height, round, Some(block_hash), *pk));
            if let Some(committed) = output.committed_block {
                return Some(committed.block.hash());
            }
        }

        None
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 1. Safety: no two different blocks committed at the same height
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        /// Run multiple engines for the same height and verify they all commit
        /// the same block (or fail to commit).
        #[test]
        fn safety_same_height_same_block(
            num_validators in 4..=10usize,
            height in 1..=100u64,
        ) {
            let stakes: Vec<u64> = vec![100; num_validators];
            let (pks, vs) = make_validator_set(num_validators, &stakes);

            let config = BftConfig::default();
            let mut committed_hashes: Vec<Hash> = Vec::new();

            // Run separate engines for each validator.
            for i in 0..num_validators {
                let mut engine = ConsensusEngine::new(config.clone(), pks[i], vs.clone());
                engine.start_new_height(height);

                if let Some(hash) = run_honest_round(&mut engine, &pks, &vs, height, 0) {
                    committed_hashes.push(hash);
                }
            }

            // All committed hashes must be identical.
            if !committed_hashes.is_empty() {
                let first = committed_hashes[0];
                for (i, hash) in committed_hashes.iter().enumerate() {
                    prop_assert_eq!(
                        *hash, first,
                        "Validator {} committed a different block at height {}", i, height
                    );
                }
            }
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 2. Liveness: if 2/3+ honest, eventually commits
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// With all validators honest and delivering messages, consensus
        /// must always succeed within 1 round.
        #[test]
        fn liveness_all_honest_commits_round_0(
            num_validators in 4..=10usize,
            height in 1..=1000u64,
        ) {
            let stakes: Vec<u64> = vec![100; num_validators];
            let (pks, vs) = make_validator_set(num_validators, &stakes);

            let config = BftConfig::default();
            let mut engine = ConsensusEngine::new(config, pks[0], vs.clone());
            engine.start_new_height(height);

            let committed = run_honest_round(&mut engine, &pks, &vs, height, 0);
            prop_assert!(
                committed.is_some(),
                "Failed to commit with all honest validators at height {height}"
            );
        }

        /// Even with exactly 1/3 validators offline (not sending votes),
        /// if the remaining 2/3+ are honest, consensus should succeed.
        #[test]
        fn liveness_one_third_offline(
            num_validators in 4..=12usize,
            height in 1..=100u64,
        ) {
            let stakes: Vec<u64> = vec![100; num_validators];
            let (pks, vs) = make_validator_set(num_validators, &stakes);

            let config = BftConfig::default();
            let mut engine = ConsensusEngine::new(config, pks[0], vs.clone());
            engine.start_new_height(height);

            // Determine how many validators are online (2/3 + 1).
            let total_stake = num_validators as u64 * 100;
            let quorum_stake = (total_stake as f64 * 0.667).ceil() as u64;
            let online_count = ((quorum_stake + 99) / 100) as usize; // ceil division

            // Only use online validators' votes.
            let online_pks: Vec<Pubkey> = pks[..online_count.min(num_validators)].to_vec();

            let proposer_pk = proposer_for_round(&vs, height, 0).unwrap();
            let block = make_block(height, proposer_pk);
            let block_hash = block.hash();

            // Deliver proposal.
            let _ = engine.on_proposal(make_proposal(height, 0, &block, proposer_pk));

            // Deliver prevotes from online validators only.
            for pk in &online_pks {
                if *pk == pks[0] { continue; }
                let _ = engine.on_prevote(make_prevote_msg(height, 0, Some(block_hash), *pk));
            }

            // Deliver precommits from online validators only.
            let mut committed = false;
            for pk in &online_pks {
                if *pk == pks[0] { continue; }
                let output = engine.on_precommit(
                    make_precommit_msg(height, 0, Some(block_hash), *pk),
                );
                if output.committed_block.is_some() {
                    committed = true;
                    break;
                }
            }

            prop_assert!(
                committed,
                "Failed to commit with {online_count}/{num_validators} validators online"
            );
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 3. Validity: only proposed blocks can be committed
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        /// A block can only be committed if it was proposed. The committed
        /// block's hash must match the proposal.
        #[test]
        fn validity_committed_block_was_proposed(
            num_validators in 4..=8usize,
            height in 1..=500u64,
        ) {
            let stakes: Vec<u64> = vec![100; num_validators];
            let (pks, vs) = make_validator_set(num_validators, &stakes);
            let config = BftConfig::default();
            let mut engine = ConsensusEngine::new(config, pks[0], vs.clone());
            engine.start_new_height(height);

            let proposer_pk = proposer_for_round(&vs, height, 0).unwrap();
            let block = make_block(height, proposer_pk);
            let expected_hash = block.hash();

            // Deliver proposal.
            let _ = engine.on_proposal(make_proposal(height, 0, &block, proposer_pk));

            // Deliver votes.
            for pk in &pks[1..] {
                let _ = engine.on_prevote(make_prevote_msg(height, 0, Some(expected_hash), *pk));
            }
            for pk in &pks[1..] {
                let output = engine.on_precommit(
                    make_precommit_msg(height, 0, Some(expected_hash), *pk),
                );
                if let Some(committed) = output.committed_block {
                    // ── INVARIANT: committed block hash matches proposal ──
                    prop_assert_eq!(
                        committed.block.hash(),
                        expected_hash,
                        "Committed block hash doesn't match proposal"
                    );
                    prop_assert_eq!(
                        committed.block.height,
                        height,
                        "Committed block height doesn't match"
                    );
                    prop_assert_eq!(
                        committed.block.proposer,
                        proposer_pk,
                        "Committed block proposer doesn't match"
                    );
                    break;
                }
            }
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 4. Proposer selection determinism
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// Proposer selection must be deterministic given the same inputs.
        #[test]
        fn proposer_selection_deterministic(
            num_validators in 2..=10usize,
            height in 0..=10000u64,
            round in 0..=20u32,
        ) {
            let stakes: Vec<u64> = vec![100; num_validators];
            let (_, vs) = make_validator_set(num_validators, &stakes);

            let p1 = proposer_for_round(&vs, height, round);
            let p2 = proposer_for_round(&vs, height, round);
            prop_assert_eq!(p1, p2, "Proposer selection is non-deterministic");

            // Proposer must be in the validator set.
            if let Some(pk) = p1 {
                prop_assert!(vs.contains(&pk), "Proposer not in validator set");
            }
        }

        /// Proposer must rotate: over total_stake consecutive rounds, every
        /// validator should be selected at least once.
        #[test]
        fn proposer_rotates_over_stake(
            num_validators in 2..=6usize,
        ) {
            let stakes: Vec<u64> = vec![1; num_validators];
            let (_pks, vs) = make_validator_set(num_validators, &stakes);

            let total_stake = vs.total_stake();
            let mut selected: std::collections::HashSet<Pubkey> = std::collections::HashSet::new();

            for r in 0..total_stake as u32 {
                if let Some(pk) = proposer_for_round(&vs, 0, r) {
                    selected.insert(pk);
                }
            }

            // With equal stake of 1 each, all validators should be selected.
            prop_assert_eq!(
                selected.len(),
                num_validators,
                "Not all validators were selected as proposer within {} rounds",
                total_stake
            );
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // 5. Timeout monotonicity
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        /// Propose timeout must increase with round number.
        #[test]
        fn propose_timeout_monotonically_increasing(
            round_a in 0..=100u32,
            round_b in 0..=100u32,
        ) {
            let config = BftConfig::default();
            let ta = config.propose_timeout_ms(round_a);
            let tb = config.propose_timeout_ms(round_b);

            if round_a < round_b {
                prop_assert!(ta < tb, "Timeout should increase: r{}={} >= r{}={}", round_a, ta, round_b, tb);
            } else if round_a > round_b {
                prop_assert!(ta > tb, "Timeout should decrease: r{}={} <= r{}={}", round_a, ta, round_b, tb);
            } else {
                prop_assert_eq!(ta, tb);
            }
        }
    }
}

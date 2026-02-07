//! Leader / proposer selection.
//!
//! Implements deterministic, stake-weighted round-robin proposer selection
//! following the Tendermint approach. Every validator in the network must
//! agree on who the proposer is for any (height, round) pair.

use {
    crate::validator_set::ValidatorSet,
    solana_pubkey::Pubkey,
};

/// Deterministic proposer selection weighted by stake.
///
/// # Algorithm
///
/// 1. Compute a deterministic seed: `seed = height + round as u64`.
/// 2. Compute `target = seed % total_stake`.
/// 3. Walk through validators in canonical order (sorted by stake desc,
///    then pubkey asc), accumulating stake.
/// 4. The first validator whose cumulative stake exceeds `target` is the
///    proposer.
///
/// This gives each validator a probability of being selected proportional
/// to their stake, while remaining fully deterministic.
pub fn proposer_for_round(
    validator_set: &ValidatorSet,
    height: u64,
    round: u32,
) -> Option<Pubkey> {
    if validator_set.is_empty() || validator_set.total_stake() == 0 {
        return None;
    }

    let total_stake = validator_set.total_stake();
    let seed = height.wrapping_add(round as u64);
    let target = seed % total_stake;

    let mut accumulated: u64 = 0;
    for validator in validator_set.iter() {
        accumulated = accumulated.saturating_add(validator.stake);
        if accumulated > target {
            return Some(validator.pubkey);
        }
    }

    // Fallback (should never happen if total_stake > 0)
    validator_set.get(0).map(|v| v.pubkey)
}

/// Check if a specific validator is the proposer for a given (height, round).
pub fn is_proposer(
    validator_set: &ValidatorSet,
    identity: &Pubkey,
    height: u64,
    round: u32,
) -> bool {
    proposer_for_round(validator_set, height, round)
        .map(|p| p == *identity)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pubkeys(n: usize) -> Vec<Pubkey> {
        (0..n).map(|_| Pubkey::new_unique()).collect()
    }

    #[test]
    fn test_proposer_deterministic() {
        let pks = make_pubkeys(4);
        let vs = ValidatorSet::new(vec![
            (pks[0], 100),
            (pks[1], 200),
            (pks[2], 300),
            (pks[3], 400),
        ]);
        let p1 = proposer_for_round(&vs, 10, 0);
        let p2 = proposer_for_round(&vs, 10, 0);
        assert_eq!(p1, p2);
    }

    #[test]
    fn test_proposer_rotates_across_rounds() {
        // With equal stake of 1 each, total_stake=4, seed%4 gives 0,1,2,3
        let pks = make_pubkeys(4);
        let vs = ValidatorSet::new(vec![
            (pks[0], 1),
            (pks[1], 1),
            (pks[2], 1),
            (pks[3], 1),
        ]);
        // Each consecutive round should pick a different validator
        let mut proposers = Vec::new();
        for round in 0..4 {
            proposers.push(proposer_for_round(&vs, 0, round).unwrap());
        }
        // All 4 should be different (since equal weights with stake=1, perfect round-robin)
        proposers.sort();
        proposers.dedup();
        assert_eq!(proposers.len(), 4);
    }

    #[test]
    fn test_proposer_rotates_across_heights() {
        let pks = make_pubkeys(4);
        let vs = ValidatorSet::new(vec![
            (pks[0], 1),
            (pks[1], 1),
            (pks[2], 1),
            (pks[3], 1),
        ]);
        let mut proposers = Vec::new();
        for height in 0..4 {
            proposers.push(proposer_for_round(&vs, height, 0).unwrap());
        }
        proposers.sort();
        proposers.dedup();
        assert_eq!(proposers.len(), 4);
    }

    #[test]
    fn test_weighted_selection_favors_higher_stake() {
        let pks = make_pubkeys(2);
        let vs = ValidatorSet::new(vec![
            (pks[0], 900), // 90% stake
            (pks[1], 100), // 10% stake
        ]);
        // Over 1000 rounds, the high-stake validator should be selected ~900 times
        let mut high_count = 0;
        let high_stake_pk = vs.get(0).unwrap().pubkey; // pk with 900 stake
        for h in 0..1000u64 {
            if proposer_for_round(&vs, h, 0).unwrap() == high_stake_pk {
                high_count += 1;
            }
        }
        // Should be roughly 900 ± some tolerance
        assert!(
            high_count > 850 && high_count < 950,
            "Expected ~900, got {high_count}"
        );
    }

    #[test]
    fn test_single_validator() {
        let pk = Pubkey::new_unique();
        let vs = ValidatorSet::new(vec![(pk, 100)]);
        for h in 0..10 {
            for r in 0..5 {
                assert_eq!(proposer_for_round(&vs, h, r), Some(pk));
            }
        }
    }

    #[test]
    fn test_empty_validator_set() {
        let vs = ValidatorSet::new(vec![]);
        assert_eq!(proposer_for_round(&vs, 0, 0), None);
    }

    #[test]
    fn test_is_proposer() {
        let pks = make_pubkeys(2);
        let vs = ValidatorSet::new(vec![(pks[0], 500), (pks[1], 500)]);
        let proposer = proposer_for_round(&vs, 0, 0).unwrap();
        assert!(is_proposer(&vs, &proposer, 0, 0));

        let other = if proposer == pks[0] { &pks[1] } else { &pks[0] };
        assert!(!is_proposer(&vs, other, 0, 0));
    }

    #[test]
    fn test_proposer_consistency_across_validator_set_creation_order() {
        // Same validators, different input order — should produce same proposer
        let pks = make_pubkeys(3);
        let vs1 = ValidatorSet::new(vec![
            (pks[0], 100),
            (pks[1], 200),
            (pks[2], 300),
        ]);
        let vs2 = ValidatorSet::new(vec![
            (pks[2], 300),
            (pks[0], 100),
            (pks[1], 200),
        ]);
        for h in 0..20 {
            for r in 0..5 {
                assert_eq!(
                    proposer_for_round(&vs1, h, r),
                    proposer_for_round(&vs2, h, r),
                    "Mismatch at height={h}, round={r}"
                );
            }
        }
    }
}

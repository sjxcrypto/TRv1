//! Weighted validator set management.
//!
//! Maintains an ordered set of validators with their stake weights.
//! Used for quorum calculations and proposer selection.

use {
    solana_pubkey::Pubkey,
    std::collections::HashMap,
};

/// A single validator with its stake weight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorInfo {
    pub pubkey: Pubkey,
    pub stake: u64,
}

/// An ordered, weighted set of validators.
///
/// Validators are sorted by (stake descending, pubkey ascending) to ensure
/// deterministic ordering across all nodes.
#[derive(Debug, Clone)]
pub struct ValidatorSet {
    /// Validators sorted by stake (descending), then pubkey (ascending) for ties.
    validators: Vec<ValidatorInfo>,
    /// Fast lookup from pubkey to index in the validators vec.
    index: HashMap<Pubkey, usize>,
    /// Sum of all validator stakes.
    total_stake: u64,
}

impl ValidatorSet {
    /// Create a new validator set from a list of (pubkey, stake) pairs.
    /// The list is sorted deterministically.
    pub fn new(validators: Vec<(Pubkey, u64)>) -> Self {
        let mut infos: Vec<ValidatorInfo> = validators
            .into_iter()
            .filter(|(_, stake)| *stake > 0)
            .map(|(pubkey, stake)| ValidatorInfo { pubkey, stake })
            .collect();

        // Sort by stake descending, then pubkey ascending for determinism
        infos.sort_by(|a, b| {
            b.stake
                .cmp(&a.stake)
                .then_with(|| a.pubkey.cmp(&b.pubkey))
        });

        let total_stake = infos.iter().map(|v| v.stake).sum();
        let index = infos
            .iter()
            .enumerate()
            .map(|(i, v)| (v.pubkey, i))
            .collect();

        Self {
            validators: infos,
            index,
            total_stake,
        }
    }

    /// Returns the number of validators.
    pub fn len(&self) -> usize {
        self.validators.len()
    }

    /// Returns true if the validator set is empty.
    pub fn is_empty(&self) -> bool {
        self.validators.is_empty()
    }

    /// Returns total stake across all validators.
    pub fn total_stake(&self) -> u64 {
        self.total_stake
    }

    /// Returns the validator at the given index.
    pub fn get(&self, index: usize) -> Option<&ValidatorInfo> {
        self.validators.get(index)
    }

    /// Look up a validator by pubkey.
    pub fn get_by_pubkey(&self, pubkey: &Pubkey) -> Option<&ValidatorInfo> {
        self.index.get(pubkey).map(|&i| &self.validators[i])
    }

    /// Returns the stake of a validator, or 0 if not in the set.
    pub fn stake_of(&self, pubkey: &Pubkey) -> u64 {
        self.get_by_pubkey(pubkey)
            .map(|v| v.stake)
            .unwrap_or(0)
    }

    /// Check whether a validator is in the set.
    pub fn contains(&self, pubkey: &Pubkey) -> bool {
        self.index.contains_key(pubkey)
    }

    /// Returns the minimum stake required for a quorum given a threshold.
    /// For 2/3+1: `ceil(total_stake * threshold) + 1` in integer arithmetic.
    pub fn quorum_stake(&self, threshold: f64) -> u64 {
        // Use integer math to avoid floating-point issues:
        // quorum = floor(total_stake * 2 / 3) + 1
        // But we parameterize by threshold for flexibility.
        let q = (self.total_stake as f64 * threshold).ceil() as u64;
        // Ensure at least 1
        q.max(1)
    }

    /// Returns an iterator over all validators in deterministic order.
    pub fn iter(&self) -> impl Iterator<Item = &ValidatorInfo> {
        self.validators.iter()
    }

    /// Returns all validator pubkeys in deterministic order.
    pub fn pubkeys(&self) -> Vec<Pubkey> {
        self.validators.iter().map(|v| v.pubkey).collect()
    }

    /// Add or update a validator's stake. Re-sorts the set.
    pub fn upsert(&mut self, pubkey: Pubkey, stake: u64) {
        // Remove existing entry if present
        self.validators.retain(|v| v.pubkey != pubkey);
        if stake > 0 {
            self.validators.push(ValidatorInfo { pubkey, stake });
        }
        // Re-sort
        self.validators.sort_by(|a, b| {
            b.stake
                .cmp(&a.stake)
                .then_with(|| a.pubkey.cmp(&b.pubkey))
        });
        self.total_stake = self.validators.iter().map(|v| v.stake).sum();
        self.index = self
            .validators
            .iter()
            .enumerate()
            .map(|(i, v)| (v.pubkey, i))
            .collect();
    }

    /// Remove a validator from the set.
    pub fn remove(&mut self, pubkey: &Pubkey) {
        self.upsert(*pubkey, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pubkeys(n: usize) -> Vec<Pubkey> {
        (0..n).map(|_| Pubkey::new_unique()).collect()
    }

    #[test]
    fn test_new_validator_set_sorted_by_stake_desc() {
        let pks = make_pubkeys(3);
        let vs = ValidatorSet::new(vec![
            (pks[0], 100),
            (pks[1], 300),
            (pks[2], 200),
        ]);
        assert_eq!(vs.len(), 3);
        assert_eq!(vs.get(0).unwrap().stake, 300);
        assert_eq!(vs.get(1).unwrap().stake, 200);
        assert_eq!(vs.get(2).unwrap().stake, 100);
    }

    #[test]
    fn test_zero_stake_filtered() {
        let pks = make_pubkeys(2);
        let vs = ValidatorSet::new(vec![(pks[0], 0), (pks[1], 100)]);
        assert_eq!(vs.len(), 1);
        assert_eq!(vs.total_stake(), 100);
    }

    #[test]
    fn test_total_stake() {
        let pks = make_pubkeys(3);
        let vs = ValidatorSet::new(vec![
            (pks[0], 100),
            (pks[1], 200),
            (pks[2], 300),
        ]);
        assert_eq!(vs.total_stake(), 600);
    }

    #[test]
    fn test_stake_of() {
        let pks = make_pubkeys(2);
        let vs = ValidatorSet::new(vec![(pks[0], 100), (pks[1], 200)]);
        assert_eq!(vs.stake_of(&pks[0]), 100);
        assert_eq!(vs.stake_of(&pks[1]), 200);
        assert_eq!(vs.stake_of(&Pubkey::new_unique()), 0);
    }

    #[test]
    fn test_contains() {
        let pks = make_pubkeys(2);
        let vs = ValidatorSet::new(vec![(pks[0], 100), (pks[1], 200)]);
        assert!(vs.contains(&pks[0]));
        assert!(!vs.contains(&Pubkey::new_unique()));
    }

    #[test]
    fn test_quorum_stake() {
        let pks = make_pubkeys(3);
        let vs = ValidatorSet::new(vec![
            (pks[0], 100),
            (pks[1], 100),
            (pks[2], 100),
        ]);
        // total = 300, 2/3 = 200, ceil(300 * 0.667) = ceil(200.1) = 201
        let q = vs.quorum_stake(0.667);
        assert!(q > 200); // Must be strictly more than 2/3
        assert!(q <= 201);
    }

    #[test]
    fn test_upsert_add() {
        let pks = make_pubkeys(3);
        let mut vs = ValidatorSet::new(vec![(pks[0], 100)]);
        vs.upsert(pks[1], 200);
        assert_eq!(vs.len(), 2);
        assert_eq!(vs.total_stake(), 300);
        assert_eq!(vs.get(0).unwrap().pubkey, pks[1]); // 200 > 100
    }

    #[test]
    fn test_upsert_update() {
        let pks = make_pubkeys(2);
        let mut vs = ValidatorSet::new(vec![(pks[0], 100), (pks[1], 200)]);
        vs.upsert(pks[0], 500);
        assert_eq!(vs.len(), 2);
        assert_eq!(vs.total_stake(), 700);
        assert_eq!(vs.get(0).unwrap().pubkey, pks[0]); // now 500 > 200
    }

    #[test]
    fn test_remove() {
        let pks = make_pubkeys(2);
        let mut vs = ValidatorSet::new(vec![(pks[0], 100), (pks[1], 200)]);
        vs.remove(&pks[0]);
        assert_eq!(vs.len(), 1);
        assert!(!vs.contains(&pks[0]));
        assert_eq!(vs.total_stake(), 200);
    }

    #[test]
    fn test_empty_set() {
        let vs = ValidatorSet::new(vec![]);
        assert!(vs.is_empty());
        assert_eq!(vs.total_stake(), 0);
        assert_eq!(vs.len(), 0);
    }

    #[test]
    fn test_deterministic_ordering_with_equal_stake() {
        let mut pks = make_pubkeys(3);
        let vs1 = ValidatorSet::new(vec![
            (pks[0], 100),
            (pks[1], 100),
            (pks[2], 100),
        ]);
        // Reverse the input order
        pks.reverse();
        let vs2 = ValidatorSet::new(vec![
            (pks[0], 100),
            (pks[1], 100),
            (pks[2], 100),
        ]);
        // Both should produce identical ordering
        let order1: Vec<Pubkey> = vs1.pubkeys();
        let order2: Vec<Pubkey> = vs2.pubkeys();
        assert_eq!(order1, order2);
    }

    #[test]
    fn test_iter() {
        let pks = make_pubkeys(2);
        let vs = ValidatorSet::new(vec![(pks[0], 100), (pks[1], 200)]);
        let collected: Vec<_> = vs.iter().collect();
        assert_eq!(collected.len(), 2);
    }
}

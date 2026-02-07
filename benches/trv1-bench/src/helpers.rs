//! Shared helpers for TRv1 benchmarks.

use {
    solana_keypair::Keypair,
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    trv1_consensus_bft::ValidatorSet,
};

/// Create a validator set of `n` validators with equal stake.
pub fn make_validator_set(n: usize) -> (ValidatorSet, Vec<Keypair>) {
    let keypairs: Vec<Keypair> = (0..n).map(|_| Keypair::new()).collect();
    let validators: Vec<(Pubkey, u64)> = keypairs
        .iter()
        .map(|kp| (kp.pubkey(), 1_000_000))
        .collect();
    (ValidatorSet::new(validators), keypairs)
}

/// Create a validator set with weighted stakes.
pub fn make_weighted_validator_set(n: usize) -> (ValidatorSet, Vec<Keypair>) {
    let keypairs: Vec<Keypair> = (0..n).map(|_| Keypair::new()).collect();
    let validators: Vec<(Pubkey, u64)> = keypairs
        .iter()
        .enumerate()
        .map(|(i, kp)| {
            // Give descending stakes so the first validators have more weight
            let stake = ((n.saturating_sub(i)) as u64).saturating_mul(1_000_000);
            (kp.pubkey(), stake)
        })
        .collect();
    (ValidatorSet::new(validators), keypairs)
}

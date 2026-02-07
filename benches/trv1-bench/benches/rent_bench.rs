//! State rent expiry benchmarks.
//!
//! Measures:
//! - Archive throughput (moving accounts to cold storage)
//! - Revival throughput (restoring archived accounts)
//! - Merkle proof generation

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use solana_hash::Hash;
use solana_pubkey::Pubkey;
use solana_sha256_hasher::hashv;

// ---------------------------------------------------------------------------
// Simulated tiered storage types
// ---------------------------------------------------------------------------

/// Represents an account in hot storage.
#[derive(Clone)]
struct HotAccount {
    pubkey: Pubkey,
    lamports: u64,
    data: Vec<u8>,
    last_accessed_slot: u64,
}

/// Represents an archived (cold storage) account.
#[derive(Clone)]
struct ArchivedAccount {
    pubkey: Pubkey,
    lamports: u64,
    data_hash: Hash,
    data: Vec<u8>,
    archived_at_slot: u64,
    merkle_proof: Vec<Hash>,
}

/// Merkle tree node for proof generation.
struct MerkleTree {
    leaves: Vec<Hash>,
    levels: Vec<Vec<Hash>>,
}

impl MerkleTree {
    fn new(leaves: Vec<Hash>) -> Self {
        let mut tree = Self {
            leaves: leaves.clone(),
            levels: vec![leaves.clone()],
        };
        tree.build();
        tree
    }

    fn build(&mut self) {
        let mut current = self.leaves.clone();

        while current.len() > 1 {
            let mut next = Vec::with_capacity((current.len() + 1) / 2);
            for pair in current.chunks(2) {
                let hash = if pair.len() == 2 {
                    hashv(&[pair[0].as_ref(), pair[1].as_ref()])
                } else {
                    hashv(&[pair[0].as_ref(), pair[0].as_ref()])
                };
                next.push(hash);
            }
            self.levels.push(next.clone());
            current = next;
        }
    }

    fn root(&self) -> Hash {
        self.levels
            .last()
            .and_then(|l| l.first().copied())
            .unwrap_or_default()
    }

    fn generate_proof(&self, leaf_index: usize) -> Vec<Hash> {
        let mut proof = Vec::new();
        let mut idx = leaf_index;

        for level in &self.levels[..self.levels.len().saturating_sub(1)] {
            let sibling_idx = if idx % 2 == 0 { idx + 1 } else { idx - 1 };
            if sibling_idx < level.len() {
                proof.push(level[sibling_idx]);
            } else if !level.is_empty() {
                proof.push(level[idx]);
            }
            idx /= 2;
        }

        proof
    }
}

/// Simulate archiving an account (hot → cold).
fn archive_account(account: &HotAccount, slot: u64, proof: Vec<Hash>) -> ArchivedAccount {
    let data_hash = hashv(&[account.data.as_slice()]);
    ArchivedAccount {
        pubkey: account.pubkey,
        lamports: account.lamports,
        data_hash,
        data: account.data.clone(),
        archived_at_slot: slot,
        merkle_proof: proof,
    }
}

/// Simulate reviving an account (cold → hot).
fn revive_account(archived: &ArchivedAccount, slot: u64) -> HotAccount {
    // Verify data integrity
    let computed_hash = hashv(&[archived.data.as_slice()]);
    assert_eq!(computed_hash, archived.data_hash, "data integrity check");

    HotAccount {
        pubkey: archived.pubkey,
        lamports: archived.lamports,
        data: archived.data.clone(),
        last_accessed_slot: slot,
    }
}

/// Verify a Merkle proof.
fn verify_merkle_proof(leaf_hash: Hash, proof: &[Hash], root: Hash) -> bool {
    let mut current = leaf_hash;
    for sibling in proof {
        if current.as_ref() <= sibling.as_ref() {
            current = hashv(&[current.as_ref(), sibling.as_ref()]);
        } else {
            current = hashv(&[sibling.as_ref(), current.as_ref()]);
        }
    }
    current == root
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_archive_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("rent/archive_throughput");

    for &n_accounts in &[1_000usize, 10_000, 100_000] {
        group.throughput(Throughput::Elements(n_accounts as u64));
        group.sample_size(10);
        group.bench_with_input(
            BenchmarkId::new("accounts", n_accounts),
            &n_accounts,
            |b, &n| {
                let accounts: Vec<HotAccount> = (0..n)
                    .map(|_| HotAccount {
                        pubkey: Pubkey::new_unique(),
                        lamports: 1_000_000,
                        data: vec![42u8; 256],
                        last_accessed_slot: 0,
                    })
                    .collect();

                // Build merkle tree from account hashes
                let leaves: Vec<Hash> = accounts
                    .iter()
                    .map(|a| hashv(&[a.pubkey.as_ref(), &a.data]))
                    .collect();
                let tree = MerkleTree::new(leaves);

                b.iter(|| {
                    let mut archived = Vec::with_capacity(n);
                    for (i, acct) in accounts.iter().enumerate() {
                        let proof = tree.generate_proof(i);
                        archived.push(archive_account(acct, 1000, proof));
                    }
                    archived.len()
                });
            },
        );
    }
    group.finish();
}

fn bench_revival_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("rent/revival_throughput");

    for &n_accounts in &[1_000usize, 10_000, 100_000] {
        group.throughput(Throughput::Elements(n_accounts as u64));
        group.sample_size(10);
        group.bench_with_input(
            BenchmarkId::new("accounts", n_accounts),
            &n_accounts,
            |b, &n| {
                // Pre-archive accounts
                let accounts: Vec<HotAccount> = (0..n)
                    .map(|_| HotAccount {
                        pubkey: Pubkey::new_unique(),
                        lamports: 1_000_000,
                        data: vec![42u8; 256],
                        last_accessed_slot: 0,
                    })
                    .collect();

                let leaves: Vec<Hash> = accounts
                    .iter()
                    .map(|a| hashv(&[a.pubkey.as_ref(), &a.data]))
                    .collect();
                let tree = MerkleTree::new(leaves);

                let archived: Vec<ArchivedAccount> = accounts
                    .iter()
                    .enumerate()
                    .map(|(i, acct)| {
                        let proof = tree.generate_proof(i);
                        archive_account(acct, 1000, proof)
                    })
                    .collect();

                b.iter(|| {
                    let mut revived = Vec::with_capacity(n);
                    for arch in &archived {
                        revived.push(revive_account(arch, 2000));
                    }
                    revived.len()
                });
            },
        );
    }
    group.finish();
}

fn bench_merkle_proof_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("rent/merkle_proof_generation");

    for &n_leaves in &[1_000usize, 10_000, 100_000] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("tree_leaves", n_leaves),
            &n_leaves,
            |b, &n| {
                let leaves: Vec<Hash> = (0..n).map(|_| Hash::new_unique()).collect();
                let tree = MerkleTree::new(leaves);

                b.iter(|| {
                    // Generate proof for a middle leaf
                    tree.generate_proof(n / 2)
                });
            },
        );
    }
    group.finish();
}

fn bench_merkle_tree_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("rent/merkle_tree_construction");
    group.sample_size(10);

    for &n_leaves in &[1_000usize, 10_000, 100_000] {
        group.throughput(Throughput::Elements(n_leaves as u64));
        group.bench_with_input(
            BenchmarkId::new("leaves", n_leaves),
            &n_leaves,
            |b, &n| {
                let leaves: Vec<Hash> = (0..n).map(|_| Hash::new_unique()).collect();

                b.iter(|| MerkleTree::new(leaves.clone()));
            },
        );
    }
    group.finish();
}

fn bench_merkle_proof_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("rent/merkle_proof_verification");

    for &n_leaves in &[1_000usize, 10_000, 100_000] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("tree_leaves", n_leaves),
            &n_leaves,
            |b, &n| {
                let leaves: Vec<Hash> = (0..n).map(|_| Hash::new_unique()).collect();
                let tree = MerkleTree::new(leaves.clone());
                let root = tree.root();
                let idx = n / 2;
                let proof = tree.generate_proof(idx);
                let leaf = leaves[idx];

                b.iter(|| verify_merkle_proof(leaf, &proof, root));
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_archive_throughput,
    bench_revival_throughput,
    bench_merkle_proof_generation,
    bench_merkle_tree_construction,
    bench_merkle_proof_verification,
);
criterion_main!(benches);

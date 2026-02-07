//! Consensus BFT benchmarks.
//!
//! Measures:
//! - Propose → commit cycle timing
//! - Message processing throughput (proposals, prevotes, precommits)
//! - Validator set sizes: 50, 100, 200
//! - Round-trip with simulated network latency

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use solana_hash::Hash;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_signer::Signer;
use trv1_consensus_bft::{
    BftConfig, ConsensusEngine, ConsensusMessage,
    ProposedBlock, ValidatorSet,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_validator_set(n: usize) -> (ValidatorSet, Vec<Keypair>) {
    let keypairs: Vec<Keypair> = (0..n).map(|_| Keypair::new()).collect();
    let validators: Vec<(Pubkey, u64)> =
        keypairs.iter().map(|kp| (kp.pubkey(), 1_000_000)).collect();
    (ValidatorSet::new(validators), keypairs)
}

fn make_proposal(height: u64, round: u32, proposer: Pubkey) -> (ConsensusMessage, ProposedBlock) {
    let block = ProposedBlock {
        parent_hash: Hash::default(),
        height,
        timestamp: 1_700_000_000_000,
        transactions: Vec::new(),
        state_root: Hash::new_unique(),
        proposer,
    };
    let msg = ConsensusMessage::Proposal {
        height,
        round,
        block: block.clone(),
        proposer,
        signature: Signature::default(),
        valid_round: None,
    };
    (msg, block)
}

fn make_prevote(height: u64, round: u32, voter: Pubkey, block_hash: Option<Hash>) -> ConsensusMessage {
    ConsensusMessage::Prevote {
        height,
        round,
        block_hash,
        voter,
        signature: Signature::default(),
    }
}

fn make_precommit(height: u64, round: u32, voter: Pubkey, block_hash: Option<Hash>) -> ConsensusMessage {
    ConsensusMessage::Precommit {
        height,
        round,
        block_hash,
        voter,
        signature: Signature::default(),
    }
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_propose_commit_cycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus/propose_commit_cycle");

    for &n_validators in &[50usize, 100, 200] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("validators", n_validators),
            &n_validators,
            |b, &n| {
                let (vs, keypairs) = make_validator_set(n);
                let config = BftConfig::default();

                b.iter(|| {
                    // We test one full cycle: engine start → proposal → prevotes → precommits
                    let identity = keypairs[0].pubkey();
                    let mut engine = ConsensusEngine::new(config.clone(), identity, vs.clone());
                    let _output = engine.start_new_height(1);

                    // Determine the proposer for round 0
                    let proposer = trv1_consensus_bft::proposer_for_round(&vs, 1, 0)
                        .unwrap_or(keypairs[0].pubkey());

                    // Deliver proposal
                    let (proposal_msg, block) = make_proposal(1, 0, proposer);
                    let _out = engine.on_proposal(proposal_msg);
                    let block_hash = block.hash();

                    // Deliver prevotes from 2/3+ validators
                    let quorum = (n * 2 / 3) + 1;
                    for i in 1..quorum {
                        let voter = keypairs[i % n].pubkey();
                        let prevote = make_prevote(1, 0, voter, Some(block_hash));
                        let _out = engine.on_prevote(prevote);
                    }

                    // Deliver precommits from 2/3+ validators
                    for i in 1..quorum {
                        let voter = keypairs[i % n].pubkey();
                        let precommit = make_precommit(1, 0, voter, Some(block_hash));
                        let _out = engine.on_precommit(precommit);
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_proposal_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus/proposal_processing");

    for &n_validators in &[50usize, 100, 200] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("validators", n_validators),
            &n_validators,
            |b, &n| {
                let (vs, keypairs) = make_validator_set(n);
                let config = BftConfig::default();

                b.iter(|| {
                    let identity = keypairs[0].pubkey();
                    let mut engine = ConsensusEngine::new(config.clone(), identity, vs.clone());
                    engine.start_new_height(1);

                    let proposer = trv1_consensus_bft::proposer_for_round(&vs, 1, 0)
                        .unwrap_or(keypairs[0].pubkey());
                    let (proposal_msg, _block) = make_proposal(1, 0, proposer);
                    engine.on_proposal(proposal_msg)
                });
            },
        );
    }
    group.finish();
}

fn bench_prevote_processing_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus/prevote_throughput");

    for &n_validators in &[50usize, 100, 200] {
        group.throughput(Throughput::Elements(n_validators as u64));
        group.bench_with_input(
            BenchmarkId::new("validators", n_validators),
            &n_validators,
            |b, &n| {
                let (vs, keypairs) = make_validator_set(n);
                let config = BftConfig::default();
                let block_hash = Hash::new_unique();

                // Pre-build all prevote messages
                let prevotes: Vec<ConsensusMessage> = keypairs
                    .iter()
                    .skip(1)
                    .map(|kp| make_prevote(1, 0, kp.pubkey(), Some(block_hash)))
                    .collect();

                b.iter(|| {
                    let identity = keypairs[0].pubkey();
                    let mut engine = ConsensusEngine::new(config.clone(), identity, vs.clone());
                    engine.start_new_height(1);

                    // Deliver proposal first
                    let proposer = trv1_consensus_bft::proposer_for_round(&vs, 1, 0)
                        .unwrap_or(keypairs[0].pubkey());
                    let (proposal_msg, _) = make_proposal(1, 0, proposer);
                    engine.on_proposal(proposal_msg);

                    // Process all prevotes
                    for pv in &prevotes {
                        engine.on_prevote(pv.clone());
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_precommit_processing_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus/precommit_throughput");

    for &n_validators in &[50usize, 100, 200] {
        group.throughput(Throughput::Elements(n_validators as u64));
        group.bench_with_input(
            BenchmarkId::new("validators", n_validators),
            &n_validators,
            |b, &n| {
                let (vs, keypairs) = make_validator_set(n);
                let config = BftConfig::default();
                let block_hash = Hash::new_unique();

                let precommits: Vec<ConsensusMessage> = keypairs
                    .iter()
                    .skip(1)
                    .map(|kp| make_precommit(1, 0, kp.pubkey(), Some(block_hash)))
                    .collect();

                b.iter(|| {
                    let identity = keypairs[0].pubkey();
                    let mut engine = ConsensusEngine::new(config.clone(), identity, vs.clone());
                    engine.start_new_height(1);

                    // Deliver a proposal so the engine is ready for votes
                    let proposer = trv1_consensus_bft::proposer_for_round(&vs, 1, 0)
                        .unwrap_or(keypairs[0].pubkey());
                    let (proposal_msg, _) = make_proposal(1, 0, proposer);
                    engine.on_proposal(proposal_msg);

                    // Deliver prevotes to reach precommit stage
                    let quorum = (n * 2 / 3) + 1;
                    for i in 1..quorum {
                        let voter = keypairs[i % n].pubkey();
                        engine.on_prevote(make_prevote(1, 0, voter, Some(block_hash)));
                    }

                    // Process all precommits
                    for pc in &precommits {
                        engine.on_precommit(pc.clone());
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_validator_set_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus/validator_set_creation");

    for &n_validators in &[50usize, 100, 200, 500] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("validators", n_validators),
            &n_validators,
            |b, &n| {
                let keypairs: Vec<Keypair> = (0..n).map(|_| Keypair::new()).collect();
                let validators: Vec<(Pubkey, u64)> =
                    keypairs.iter().map(|kp| (kp.pubkey(), 1_000_000)).collect();

                b.iter(|| ValidatorSet::new(validators.clone()));
            },
        );
    }
    group.finish();
}

fn bench_round_trip_with_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus/round_trip_simulated_latency");
    group.sample_size(20);

    for &n_validators in &[50usize, 100, 200] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("validators", n_validators),
            &n_validators,
            |b, &n| {
                let (vs, keypairs) = make_validator_set(n);
                let config = BftConfig::default();

                b.iter(|| {
                    let identity = keypairs[0].pubkey();
                    let mut engine = ConsensusEngine::new(config.clone(), identity, vs.clone());
                    engine.start_new_height(1);

                    let proposer = trv1_consensus_bft::proposer_for_round(&vs, 1, 0)
                        .unwrap_or(keypairs[0].pubkey());
                    let (proposal_msg, block) = make_proposal(1, 0, proposer);
                    let block_hash = block.hash();

                    // Simulate network latency: process messages in batches
                    // Batch 1: proposal
                    engine.on_proposal(proposal_msg);

                    // Simulate ~50ms network latency with a spin wait (noop in bench, but
                    // exercises the code path with interleaved messages)
                    std::hint::black_box(0u64);

                    // Batch 2: prevotes arrive in waves
                    let quorum = (n * 2 / 3) + 1;
                    let batch_size = quorum / 3;

                    for batch_start in (1..quorum).step_by(batch_size.max(1)) {
                        let batch_end = (batch_start + batch_size).min(quorum);
                        for i in batch_start..batch_end {
                            let voter = keypairs[i % n].pubkey();
                            engine.on_prevote(make_prevote(1, 0, voter, Some(block_hash)));
                        }
                        std::hint::black_box(0u64); // simulate inter-batch gap
                    }

                    // Batch 3: precommits
                    for batch_start in (1..quorum).step_by(batch_size.max(1)) {
                        let batch_end = (batch_start + batch_size).min(quorum);
                        for i in batch_start..batch_end {
                            let voter = keypairs[i % n].pubkey();
                            engine.on_precommit(make_precommit(1, 0, voter, Some(block_hash)));
                        }
                        std::hint::black_box(0u64);
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_propose_commit_cycle,
    bench_proposal_processing,
    bench_prevote_processing_throughput,
    bench_precommit_processing_throughput,
    bench_validator_set_creation,
    bench_round_trip_with_latency,
);
criterion_main!(benches);

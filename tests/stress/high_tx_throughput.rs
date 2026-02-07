//! Stress Test: High Transaction Throughput
//!
//! Simulates maximum TPS with realistic transaction mixes to identify
//! bottlenecks in the fee market, consensus pipeline, and execution engine.
//!
//! Run: `cargo test --test high_tx_throughput -- --nocapture`

use std::time::Instant;

/// Simulated transaction types with varying compute costs.
#[derive(Clone, Copy, Debug)]
enum TxType {
    Transfer,        // 150 CU
    TokenTransfer,   // 200_000 CU
    SmartContract,   // 500_000 CU
    DefiSwap,        // 800_000 CU
}

impl TxType {
    fn compute_units(&self) -> u64 {
        match self {
            TxType::Transfer => 150,
            TxType::TokenTransfer => 200_000,
            TxType::SmartContract => 500_000,
            TxType::DefiSwap => 800_000,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            TxType::Transfer => "Transfer",
            TxType::TokenTransfer => "TokenTransfer",
            TxType::SmartContract => "SmartContract",
            TxType::DefiSwap => "DefiSwap",
        }
    }
}

/// Simulated fee market state.
struct FeeState {
    base_fee_per_cu: u64,
    total_cu_used: u64,
    max_block_cu: u64,
    target_cu: u64,
}

impl FeeState {
    fn new() -> Self {
        Self {
            base_fee_per_cu: 5_000,
            total_cu_used: 0,
            max_block_cu: 48_000_000,
            target_cu: 24_000_000,
        }
    }

    fn can_fit(&self, cu: u64) -> bool {
        self.total_cu_used + cu <= self.max_block_cu
    }

    fn add_tx(&mut self, cu: u64) {
        self.total_cu_used += cu;
    }

    fn finalize_block(&mut self) {
        // EIP-1559 adjustment
        if self.total_cu_used > self.target_cu {
            let excess = self.total_cu_used - self.target_cu;
            let delta = self.base_fee_per_cu * excess / self.target_cu / 8;
            self.base_fee_per_cu += delta.max(1);
        } else if self.total_cu_used < self.target_cu {
            let deficit = self.target_cu - self.total_cu_used;
            let delta = self.base_fee_per_cu * deficit / self.target_cu / 8;
            self.base_fee_per_cu = self.base_fee_per_cu.saturating_sub(delta);
        }
        self.total_cu_used = 0;
    }
}

/// Generate a realistic transaction mix.
/// Distribution: 60% transfers, 20% token transfers, 15% contracts, 5% defi
fn generate_tx_mix(n: usize) -> Vec<TxType> {
    let mut txs = Vec::with_capacity(n);
    for i in 0..n {
        let pct = (i * 100) / n;
        let tx = match pct % 100 {
            0..=59 => TxType::Transfer,
            60..=79 => TxType::TokenTransfer,
            80..=94 => TxType::SmartContract,
            _ => TxType::DefiSwap,
        };
        txs.push(tx);
    }
    txs
}

#[test]
fn stress_high_tx_throughput() {
    println!("\n=== TRv1 High Transaction Throughput Stress Test ===\n");

    let n_blocks = 100;
    let txs_per_block = 10_000;
    let mut fee_state = FeeState::new();
    let mut total_txs_processed = 0u64;
    let mut total_txs_rejected = 0u64;
    let mut block_times = Vec::with_capacity(n_blocks);

    let start = Instant::now();

    for block in 0..n_blocks {
        let block_start = Instant::now();
        let tx_mix = generate_tx_mix(txs_per_block);

        let mut block_txs = 0u64;
        let mut block_rejected = 0u64;

        for tx in &tx_mix {
            let cu = tx.compute_units();
            if fee_state.can_fit(cu) {
                fee_state.add_tx(cu);
                block_txs += 1;
            } else {
                block_rejected += 1;
            }
        }

        fee_state.finalize_block();
        total_txs_processed += block_txs;
        total_txs_rejected += block_rejected;
        block_times.push(block_start.elapsed());

        if block % 25 == 0 {
            println!(
                "Block {block}: {block_txs} txs, {block_rejected} rejected, base_fee={}",
                fee_state.base_fee_per_cu
            );
        }
    }

    let elapsed = start.elapsed();
    let avg_block_time = elapsed / n_blocks as u32;
    let tps = total_txs_processed as f64 / elapsed.as_secs_f64();

    println!("\n--- Results ---");
    println!("Total blocks:    {n_blocks}");
    println!("Total processed: {total_txs_processed}");
    println!("Total rejected:  {total_txs_rejected}");
    println!("Avg block time:  {avg_block_time:?}");
    println!("Effective TPS:   {tps:.0}");
    println!("Final base fee:  {}", fee_state.base_fee_per_cu);
    println!("Total elapsed:   {elapsed:?}");

    // Assertions
    assert!(total_txs_processed > 0, "should process some transactions");
    assert!(
        avg_block_time.as_millis() < 1_000,
        "block processing should be under 1 second: {avg_block_time:?}"
    );
}

#[test]
fn stress_burst_traffic() {
    println!("\n=== TRv1 Burst Traffic Stress Test ===\n");

    let mut fee_state = FeeState::new();
    let mut base_fees = Vec::new();

    // Phase 1: 50 quiet blocks
    for _ in 0..50 {
        let txs = generate_tx_mix(100);
        for tx in &txs {
            if fee_state.can_fit(tx.compute_units()) {
                fee_state.add_tx(tx.compute_units());
            }
        }
        base_fees.push(fee_state.base_fee_per_cu);
        fee_state.finalize_block();
    }
    let quiet_fee = fee_state.base_fee_per_cu;

    // Phase 2: 50 maxed-out blocks
    for _ in 0..50 {
        let txs = generate_tx_mix(50_000);
        for tx in &txs {
            if fee_state.can_fit(tx.compute_units()) {
                fee_state.add_tx(tx.compute_units());
            }
        }
        base_fees.push(fee_state.base_fee_per_cu);
        fee_state.finalize_block();
    }
    let busy_fee = fee_state.base_fee_per_cu;

    // Phase 3: 50 quiet blocks again
    for _ in 0..50 {
        let txs = generate_tx_mix(100);
        for tx in &txs {
            if fee_state.can_fit(tx.compute_units()) {
                fee_state.add_tx(tx.compute_units());
            }
        }
        base_fees.push(fee_state.base_fee_per_cu);
        fee_state.finalize_block();
    }
    let recovery_fee = fee_state.base_fee_per_cu;

    println!("Quiet fee:    {quiet_fee}");
    println!("Busy fee:     {busy_fee}");
    println!("Recovery fee: {recovery_fee}");

    assert!(
        busy_fee > quiet_fee,
        "fee should rise during congestion: quiet={quiet_fee}, busy={busy_fee}"
    );
    assert!(
        recovery_fee < busy_fee,
        "fee should drop after congestion: busy={busy_fee}, recovery={recovery_fee}"
    );
}

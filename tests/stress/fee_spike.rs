//! Stress Test: Fee Spike
//!
//! Simulates a sudden demand spike (e.g., NFT mint, token launch) to verify
//! the EIP-1559 fee market adjusts correctly — rising under load and
//! recovering gracefully afterward.
//!
//! Run: `cargo test --test fee_spike -- --nocapture`

use std::time::Instant;

/// Simulated fee market (mirrors TRv1's EIP-1559 implementation).
struct FeeMarket {
    base_fee_per_cu: u64,
    min_base_fee: u64,
    max_base_fee: u64,
    max_block_cu: u64,
    target_cu: u64,
    denominator: u64,
}

impl FeeMarket {
    fn default_config() -> Self {
        Self {
            base_fee_per_cu: 5_000,
            min_base_fee: 5_000,
            max_base_fee: 50_000_000,
            max_block_cu: 48_000_000,
            target_cu: 24_000_000,
            denominator: 8,
        }
    }

    fn adjust_base_fee(&mut self, gas_used: u64) {
        if gas_used > self.target_cu {
            let excess = gas_used - self.target_cu;
            let delta = (self.base_fee_per_cu as u128 * excess as u128
                / self.target_cu as u128
                / self.denominator as u128) as u64;
            self.base_fee_per_cu = self
                .base_fee_per_cu
                .saturating_add(delta.max(1))
                .min(self.max_base_fee);
        } else {
            let deficit = self.target_cu - gas_used;
            let delta = (self.base_fee_per_cu as u128 * deficit as u128
                / self.target_cu as u128
                / self.denominator as u128) as u64;
            self.base_fee_per_cu = self
                .base_fee_per_cu
                .saturating_sub(delta)
                .max(self.min_base_fee);
        }
    }
}

/// Describes a demand phase.
struct DemandPhase {
    name: &'static str,
    blocks: u64,
    utilization_pct: u64, // 0-200 (can exceed 100% up to max block CU)
}

#[test]
fn stress_fee_spike_and_recovery() {
    println!("\n=== TRv1 Fee Spike Stress Test ===\n");

    let mut market = FeeMarket::default_config();

    let phases = vec![
        DemandPhase { name: "Quiet",          blocks: 100, utilization_pct: 30 },
        DemandPhase { name: "Ramp-up",        blocks: 20,  utilization_pct: 80 },
        DemandPhase { name: "Spike (NFT mint)", blocks: 50, utilization_pct: 200 }, // full blocks
        DemandPhase { name: "Sustained high", blocks: 100, utilization_pct: 150 },
        DemandPhase { name: "Cool-down",      blocks: 30,  utilization_pct: 60 },
        DemandPhase { name: "Recovery",       blocks: 200, utilization_pct: 40 },
    ];

    let start = Instant::now();
    let mut history: Vec<(String, u64, u64)> = Vec::new(); // (phase, block, base_fee)
    let mut block_num = 0u64;

    let mut pre_spike_fee = market.base_fee_per_cu;
    let mut peak_fee = 0u64;

    for phase in &phases {
        println!("Phase: {} ({} blocks, {}% util)", phase.name, phase.blocks, phase.utilization_pct);
        let phase_start_fee = market.base_fee_per_cu;

        for _ in 0..phase.blocks {
            let gas_used = (market.max_block_cu * phase.utilization_pct / 100)
                .min(market.max_block_cu);

            market.adjust_base_fee(gas_used);
            peak_fee = peak_fee.max(market.base_fee_per_cu);

            history.push((phase.name.to_string(), block_num, market.base_fee_per_cu));
            block_num += 1;
        }

        let phase_end_fee = market.base_fee_per_cu;
        println!(
            "  base_fee: {} → {} ({})",
            phase_start_fee,
            phase_end_fee,
            if phase_end_fee > phase_start_fee { "↑" } else { "↓" }
        );

        if phase.name == "Quiet" {
            pre_spike_fee = phase_end_fee;
        }
    }

    let final_fee = market.base_fee_per_cu;
    let elapsed = start.elapsed();

    println!("\n--- Results ---");
    println!("Total blocks simulated: {block_num}");
    println!("Pre-spike base fee:     {pre_spike_fee}");
    println!("Peak base fee:          {peak_fee}");
    println!("Final base fee:         {final_fee}");
    println!("Peak / Pre-spike ratio: {:.1}x", peak_fee as f64 / pre_spike_fee as f64);
    println!("Recovery ratio:         {:.1}x above pre-spike", final_fee as f64 / pre_spike_fee as f64);
    println!("Elapsed:                {elapsed:?}");

    // Assertions
    assert!(
        peak_fee > pre_spike_fee * 2,
        "fee should at least double during spike: peak={peak_fee}, pre_spike={pre_spike_fee}"
    );
    assert!(
        final_fee < peak_fee,
        "fee should recover below peak: final={final_fee}, peak={peak_fee}"
    );
    assert!(
        market.base_fee_per_cu >= market.min_base_fee,
        "fee should not go below minimum"
    );
    assert!(
        market.base_fee_per_cu <= market.max_base_fee,
        "fee should not exceed maximum"
    );
}

#[test]
fn stress_fee_oscillation_stability() {
    println!("\n=== TRv1 Fee Oscillation Stability Test ===\n");

    let mut market = FeeMarket::default_config();

    // Alternate between full and empty blocks for 1000 blocks
    let mut fees = Vec::with_capacity(1000);
    for i in 0..1000u64 {
        let gas_used = if i % 2 == 0 {
            market.max_block_cu // full block
        } else {
            0 // empty block
        };

        market.adjust_base_fee(gas_used);
        fees.push(market.base_fee_per_cu);
    }

    // Check that the fee doesn't oscillate wildly
    let last_100: &[u64] = &fees[900..];
    let max_fee = *last_100.iter().max().unwrap();
    let min_fee = *last_100.iter().min().unwrap();
    let ratio = max_fee as f64 / min_fee as f64;

    println!("After 1000 oscillating blocks:");
    println!("  Min fee (last 100): {min_fee}");
    println!("  Max fee (last 100): {max_fee}");
    println!("  Oscillation ratio:  {ratio:.2}x");

    // The fee should stabilize to some degree (not oscillate more than 3x)
    assert!(
        ratio < 3.0,
        "fee oscillation is too volatile: ratio={ratio:.2}x"
    );
}

#[test]
fn stress_max_fee_ceiling() {
    println!("\n=== TRv1 Max Fee Ceiling Test ===\n");

    let mut market = FeeMarket::default_config();

    // Sustained 100% utilization for 10,000 blocks
    for _ in 0..10_000 {
        market.adjust_base_fee(market.max_block_cu);
    }

    println!("After 10,000 full blocks: base_fee = {}", market.base_fee_per_cu);
    assert_eq!(
        market.base_fee_per_cu, market.max_base_fee,
        "fee should hit ceiling after sustained congestion"
    );

    // Now recover with 0 utilization
    let mut blocks_to_floor = 0u64;
    while market.base_fee_per_cu > market.min_base_fee {
        market.adjust_base_fee(0);
        blocks_to_floor += 1;
        if blocks_to_floor > 100_000 {
            break;
        }
    }

    println!("Blocks to recover from ceiling to floor: {blocks_to_floor}");
    assert!(
        blocks_to_floor < 100_000,
        "recovery should complete within reasonable time"
    );
}

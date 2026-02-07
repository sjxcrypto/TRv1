//! Prometheus text format exporter for TRv1 metrics.
//!
//! Encodes a [`MetricsSnapshot`] into the [Prometheus exposition format](https://prometheus.io/docs/instrumenting/exposition_formats/)
//! (text/plain; version=0.0.4).
//!
//! ## Usage
//!
//! ```rust
//! use trv1_monitoring::{TRv1Metrics, prometheus};
//!
//! let metrics = TRv1Metrics::new();
//! metrics.blocks_produced.add(42);
//! metrics.current_base_fee.set(5_000);
//!
//! let snapshot = metrics.snapshot();
//! let text = prometheus::encode(&snapshot);
//! // Serve `text` on /metrics endpoint
//! ```

use crate::MetricsSnapshot;

/// Encode a metrics snapshot into Prometheus text exposition format.
pub fn encode(snap: &MetricsSnapshot) -> String {
    let mut out = String::with_capacity(4096);

    // -----------------------------------------------------------------------
    // Consensus
    // -----------------------------------------------------------------------
    write_counter(&mut out, "trv1_blocks_produced_total",
        "Total number of blocks produced by this validator",
        snap.blocks_produced);

    write_histogram(&mut out, "trv1_consensus_rounds",
        "Number of consensus rounds needed to finalize a block",
        &snap.consensus_rounds_buckets, snap.consensus_rounds_sum, snap.consensus_rounds_count);

    write_histogram(&mut out, "trv1_finality_time_ms",
        "Time from proposal to commit in milliseconds",
        &snap.finality_time_buckets, snap.finality_time_sum, snap.finality_time_count);

    write_counter(&mut out, "trv1_missed_proposals_total",
        "Total number of missed block proposals",
        snap.missed_proposals);

    // -----------------------------------------------------------------------
    // Fee Market
    // -----------------------------------------------------------------------
    write_gauge(&mut out, "trv1_current_base_fee",
        "Current base fee per compute unit in lamports",
        snap.current_base_fee);

    write_gauge(&mut out, "trv1_block_utilization_bps",
        "Current block utilization in basis points (10000 = 100%)",
        snap.block_utilization);

    write_counter(&mut out, "trv1_fees_burned_total",
        "Total fees burned (lamports)", snap.total_fees_burned);

    write_counter(&mut out, "trv1_fees_treasury_total",
        "Total fees sent to treasury (lamports)", snap.total_fees_treasury);

    write_counter(&mut out, "trv1_fees_dev_total",
        "Total fees sent to developer fund (lamports)", snap.total_fees_dev);

    write_counter(&mut out, "trv1_fees_validator_total",
        "Total fees distributed to validators (lamports)", snap.total_fees_validator);

    // -----------------------------------------------------------------------
    // Storage
    // -----------------------------------------------------------------------
    write_gauge(&mut out, "trv1_hot_cache_size_bytes",
        "Size of the hot (in-memory) account cache in bytes",
        snap.hot_cache_size);

    write_gauge(&mut out, "trv1_warm_storage_size_bytes",
        "Size of warm (SSD) storage in bytes",
        snap.warm_storage_size);

    write_gauge(&mut out, "trv1_cold_storage_size_bytes",
        "Size of cold (archival) storage in bytes",
        snap.cold_storage_size);

    write_gauge(&mut out, "trv1_cache_hit_rate_bps",
        "Account cache hit rate in basis points (10000 = 100%)",
        snap.cache_hit_rate);

    write_counter(&mut out, "trv1_cache_evictions_total",
        "Total number of cache evictions",
        snap.cache_evictions);

    // -----------------------------------------------------------------------
    // Staking
    // -----------------------------------------------------------------------
    write_gauge(&mut out, "trv1_total_staked_lamports",
        "Total lamports staked across all validators",
        snap.total_staked);

    write_gauge(&mut out, "trv1_staking_participation_rate_bps",
        "Staking participation rate in basis points",
        snap.staking_participation_rate);

    write_gauge(&mut out, "trv1_active_validators",
        "Number of active validators in the current set",
        snap.active_validators);

    write_gauge(&mut out, "trv1_standby_validators",
        "Number of standby validators",
        snap.standby_validators);

    write_gauge(&mut out, "trv1_jailed_validators",
        "Number of jailed validators",
        snap.jailed_validators);

    // -----------------------------------------------------------------------
    // Passive Staking
    // -----------------------------------------------------------------------
    write_gauge(&mut out, "trv1_passive_stake_total_lamports",
        "Total lamports in passive staking",
        snap.passive_stake_total);

    let tier_names = ["no_lock", "30d", "90d", "180d", "360d", "permanent"];
    for (i, &name) in tier_names.iter().enumerate() {
        let metric_name = format!("trv1_passive_stake_tier_{name}_lamports");
        let help = format!("Passive stake in {name} tier (lamports)");
        write_gauge(&mut out, &metric_name, &help, snap.passive_stake_by_tier[i]);
    }

    out
}

// ---------------------------------------------------------------------------
// Helper writers
// ---------------------------------------------------------------------------

fn write_counter(out: &mut String, name: &str, help: &str, value: u64) {
    out.push_str(&format!("# HELP {name} {help}\n"));
    out.push_str(&format!("# TYPE {name} counter\n"));
    out.push_str(&format!("{name} {value}\n\n"));
}

fn write_gauge(out: &mut String, name: &str, help: &str, value: i64) {
    out.push_str(&format!("# HELP {name} {help}\n"));
    out.push_str(&format!("# TYPE {name} gauge\n"));
    out.push_str(&format!("{name} {value}\n\n"));
}

fn write_histogram(
    out: &mut String,
    name: &str,
    help: &str,
    buckets: &[(f64, u64)],
    sum: f64,
    count: u64,
) {
    out.push_str(&format!("# HELP {name} {help}\n"));
    out.push_str(&format!("# TYPE {name} histogram\n"));

    for (bound, cumulative_count) in buckets {
        if bound.is_infinite() {
            out.push_str(&format!("{name}_bucket{{le=\"+Inf\"}} {cumulative_count}\n"));
        } else {
            out.push_str(&format!("{name}_bucket{{le=\"{bound}\"}} {cumulative_count}\n"));
        }
    }
    // Always include +Inf bucket
    out.push_str(&format!("{name}_bucket{{le=\"+Inf\"}} {count}\n"));
    out.push_str(&format!("{name}_sum {sum}\n"));
    out.push_str(&format!("{name}_count {count}\n\n"));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TRv1Metrics;

    #[test]
    fn test_encode_produces_valid_output() {
        let metrics = TRv1Metrics::new();
        metrics.blocks_produced.add(42);
        metrics.current_base_fee.set(5_000);
        metrics.active_validators.set(100);
        metrics.consensus_rounds.observe(1.0);
        metrics.consensus_rounds.observe(2.0);
        metrics.finality_time_ms.observe(1200.0);
        metrics.passive_stake_tier_0.set(500_000_000);

        let snap = metrics.snapshot();
        let text = encode(&snap);

        // Verify key lines are present
        assert!(text.contains("# TYPE trv1_blocks_produced_total counter"));
        assert!(text.contains("trv1_blocks_produced_total 42"));
        assert!(text.contains("# TYPE trv1_current_base_fee gauge"));
        assert!(text.contains("trv1_current_base_fee 5000"));
        assert!(text.contains("# TYPE trv1_consensus_rounds histogram"));
        assert!(text.contains("trv1_consensus_rounds_count 2"));
        assert!(text.contains("trv1_finality_time_ms_count 1"));
        assert!(text.contains("trv1_active_validators 100"));
        assert!(text.contains("trv1_passive_stake_tier_no_lock_lamports 500000000"));
    }

    #[test]
    fn test_encode_all_metrics_present() {
        let metrics = TRv1Metrics::new();
        let snap = metrics.snapshot();
        let text = encode(&snap);

        // All metric families should have HELP and TYPE lines
        let expected_metrics = [
            "trv1_blocks_produced_total",
            "trv1_consensus_rounds",
            "trv1_finality_time_ms",
            "trv1_missed_proposals_total",
            "trv1_current_base_fee",
            "trv1_block_utilization_bps",
            "trv1_fees_burned_total",
            "trv1_fees_treasury_total",
            "trv1_fees_dev_total",
            "trv1_fees_validator_total",
            "trv1_hot_cache_size_bytes",
            "trv1_warm_storage_size_bytes",
            "trv1_cold_storage_size_bytes",
            "trv1_cache_hit_rate_bps",
            "trv1_cache_evictions_total",
            "trv1_total_staked_lamports",
            "trv1_staking_participation_rate_bps",
            "trv1_active_validators",
            "trv1_standby_validators",
            "trv1_jailed_validators",
            "trv1_passive_stake_total_lamports",
            "trv1_passive_stake_tier_no_lock_lamports",
            "trv1_passive_stake_tier_30d_lamports",
            "trv1_passive_stake_tier_90d_lamports",
            "trv1_passive_stake_tier_180d_lamports",
            "trv1_passive_stake_tier_360d_lamports",
            "trv1_passive_stake_tier_permanent_lamports",
        ];

        for metric in &expected_metrics {
            assert!(
                text.contains(&format!("# HELP {metric}")),
                "Missing HELP for {metric}"
            );
        }
    }

    #[test]
    fn test_histogram_buckets_format() {
        let metrics = TRv1Metrics::new();
        metrics.consensus_rounds.observe(1.0);
        metrics.consensus_rounds.observe(3.0);
        metrics.consensus_rounds.observe(5.0);

        let snap = metrics.snapshot();
        let text = encode(&snap);

        assert!(text.contains("trv1_consensus_rounds_bucket{le=\"1\"} 1"));
        assert!(text.contains("trv1_consensus_rounds_bucket{le=\"3\"} 2"));
        assert!(text.contains("trv1_consensus_rounds_bucket{le=\"5\"} 3"));
        assert!(text.contains("trv1_consensus_rounds_sum 9"));
        assert!(text.contains("trv1_consensus_rounds_count 3"));
    }
}

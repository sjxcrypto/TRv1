//! Configuration for the consensus networking layer.

use std::net::SocketAddr;

/// Configuration for the consensus P2P network.
///
/// Controls connection limits, timeouts, and transport behavior for
/// validator-to-validator consensus message propagation.
#[derive(Debug, Clone)]
pub struct ConsensusNetConfig {
    /// Local address to bind the consensus listener on.
    /// Default: `0.0.0.0:8900`
    pub bind_addr: SocketAddr,

    /// Port for consensus P2P messages.
    /// This is the canonical port advertised to peers via gossip.
    pub consensus_port: u16,

    /// Maximum number of peers to maintain connections to.
    /// Matches TRv1's active validator cap of 200.
    pub max_peers: usize,

    /// How long to wait for a message send/recv before considering it failed (ms).
    pub message_timeout_ms: u64,

    /// Interval between heartbeat pings to connected peers (ms).
    /// Used to detect dead connections and measure latency.
    pub heartbeat_interval_ms: u64,

    /// Maximum size of a single serialized message in bytes.
    /// Consensus votes are small (~200 bytes), but blocks can be up to 1 MB.
    pub max_message_size: usize,

    /// Whether to prefer QUIC transport over TCP.
    /// QUIC provides better multiplexing and connection migration.
    pub prefer_quic: bool,

    /// Number of seconds a peer can be silent before being considered dead.
    pub peer_timeout_secs: u64,

    /// Maximum number of concurrent block sync requests.
    pub max_sync_requests: usize,

    /// Size of the internal message channel buffer.
    pub channel_buffer_size: usize,
}

impl Default for ConsensusNetConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:8900".parse().expect("valid default bind addr"),
            consensus_port: 8900,
            max_peers: 200,
            message_timeout_ms: 5_000,
            heartbeat_interval_ms: 500,
            max_message_size: 1_048_576, // 1 MB
            prefer_quic: true,
            peer_timeout_secs: 30,
            max_sync_requests: 16,
            channel_buffer_size: 10_000,
        }
    }
}

impl ConsensusNetConfig {
    /// Create a config suitable for local testing with shorter timeouts.
    #[cfg(any(test, feature = "dev-context-only-utils"))]
    pub fn dev_default() -> Self {
        Self {
            bind_addr: "127.0.0.1:0".parse().expect("valid dev bind addr"),
            consensus_port: 0,
            max_peers: 10,
            message_timeout_ms: 1_000,
            heartbeat_interval_ms: 200,
            max_message_size: 1_048_576,
            prefer_quic: false,
            peer_timeout_secs: 5,
            max_sync_requests: 4,
            channel_buffer_size: 1_000,
        }
    }
}

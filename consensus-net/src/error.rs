//! Error types for the consensus networking layer.

use thiserror::Error;

/// Errors that can occur in the consensus networking layer.
#[derive(Error, Debug)]
pub enum ConsensusNetError {
    /// Failed to serialize a message.
    #[error("serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    /// Message exceeds the maximum allowed size.
    #[error("message too large: {size} bytes (max {max} bytes)")]
    MessageTooLarge {
        /// Actual message size.
        size: usize,
        /// Configured maximum.
        max: usize,
    },

    /// The peer is not known to the peer manager.
    #[error("unknown peer: {0}")]
    UnknownPeer(solana_pubkey::Pubkey),

    /// The peer is already connected.
    #[error("peer already connected: {0}")]
    PeerAlreadyConnected(solana_pubkey::Pubkey),

    /// Maximum peer count has been reached.
    #[error("maximum peers reached: {0}")]
    MaxPeersReached(usize),

    /// Transport-level I/O error.
    #[error("transport error: {0}")]
    Transport(#[from] std::io::Error),

    /// A message timed out waiting for delivery or response.
    #[error("message timeout after {0}ms")]
    Timeout(u64),

    /// The requested block height is not available.
    #[error("block not available at height {0}")]
    BlockNotAvailable(u64),

    /// The channel used to deliver messages to the consensus engine is closed.
    #[error("consensus channel closed")]
    ChannelClosed,

    /// The peer sent an invalid or corrupt message.
    #[error("invalid message from peer: {0}")]
    InvalidMessage(String),

    /// Connection to a peer was refused or dropped.
    #[error("connection failed to {0}: {1}")]
    ConnectionFailed(std::net::SocketAddr, String),
}

/// Convenience result type for consensus networking operations.
pub type Result<T> = std::result::Result<T, ConsensusNetError>;

//! Message types and serialization for consensus network communication.
//!
//! All messages are serialized with bincode for compact wire representation.
//! The [`ConsensusNetMessage`] enum is the top-level wire type — every byte
//! sequence on the consensus channel is a length-prefixed bincode encoding
//! of this enum.

use {
    crate::error::{ConsensusNetError, Result},
    agave_votor_messages::consensus_message::{Certificate, ConsensusMessage, VoteMessage},
    serde::{Deserialize, Serialize},
    solana_clock::Slot,
    solana_hash::Hash,
    solana_pubkey::Pubkey,
    std::net::SocketAddr,
};

// ── Peer and validator info ─────────────────────────────────────────────────

/// Information about a peer on the consensus network.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeerInfo {
    /// The peer's identity public key.
    pub pubkey: Pubkey,
    /// Network address the peer is reachable on.
    pub addr: SocketAddr,
    /// Stake weight (lamports) — used for peer prioritisation.
    pub stake_weight: u64,
    /// Whether this peer is an active validator in the current epoch.
    pub is_active_validator: bool,
}

/// Compact validator identity used in validator-set updates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidatorInfo {
    /// Validator identity public key.
    pub pubkey: Pubkey,
    /// Stake weight for the upcoming epoch.
    pub stake_weight: u64,
    /// Consensus network address.
    pub consensus_addr: SocketAddr,
}

// ── Block data ──────────────────────────────────────────────────────────────

/// Serialised block payload sent over the wire.
///
/// Contains the slot, parent linkage, transaction data, and a state root
/// for independent verification by receiving validators.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlockData {
    /// Slot number for this block.
    pub slot: Slot,
    /// Hash of the parent block.
    pub parent_hash: Hash,
    /// Block content hash (covers transactions + state root).
    pub block_hash: Hash,
    /// Serialised transactions (opaque bytes — the runtime layer decodes these).
    pub transactions: Vec<Vec<u8>>,
    /// Post-execution state root.
    pub state_root: Hash,
    /// Merkle proof of block validity (root of transaction Merkle tree).
    pub merkle_root: Hash,
    /// Individual Merkle proof nodes for verifying `merkle_root`.
    pub merkle_proof: Vec<Hash>,
    /// Identity of the proposer.
    pub proposer: Pubkey,
}

// ── Wire message ────────────────────────────────────────────────────────────

/// Top-level consensus network message.
///
/// Every datagram / stream frame on the consensus channel carries exactly one
/// of these variants, length-prefixed and bincode-encoded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConsensusNetMessage {
    /// A BFT consensus message (vote or certificate) from `votor`.
    Consensus(ConsensusMessage),

    /// A full block broadcast by the proposer.
    BlockData(BlockData),

    /// Peer discovery announcement.
    PeerAnnounce(PeerInfo),

    /// Request a block at a given height (used during catch-up sync).
    BlockRequest {
        /// Slot being requested.
        height: u64,
    },

    /// Response carrying the requested block.
    BlockResponse {
        /// Slot of the returned block.
        height: u64,
        /// The block payload.
        block: BlockData,
    },

    /// Notification that the active validator set changed at an epoch boundary.
    ValidatorSetUpdate {
        /// Epoch the new set becomes active.
        epoch: u64,
        /// Full validator list for the epoch.
        validators: Vec<ValidatorInfo>,
    },

    /// Lightweight heartbeat / keep-alive ping.
    Heartbeat {
        /// Sender identity.
        pubkey: Pubkey,
        /// Sender's current highest committed slot.
        latest_slot: Slot,
    },

    /// Response to a heartbeat.
    HeartbeatAck {
        /// Responder identity.
        pubkey: Pubkey,
        /// Responder's current highest committed slot.
        latest_slot: Slot,
    },
}

// ── Serialisation helpers ───────────────────────────────────────────────────

impl ConsensusNetMessage {
    /// Serialize this message to bytes using bincode.
    pub fn serialize(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(ConsensusNetError::Serialization)
    }

    /// Deserialize a message from bytes.
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(ConsensusNetError::Serialization)
    }

    /// Serialize with a 4-byte little-endian length prefix.
    ///
    /// Wire format: `[len: u32-le][payload: len bytes]`
    pub fn serialize_framed(&self, max_size: usize) -> Result<Vec<u8>> {
        let payload = self.serialize()?;
        if payload.len() > max_size {
            return Err(ConsensusNetError::MessageTooLarge {
                size: payload.len(),
                max: max_size,
            });
        }
        let len = payload.len() as u32;
        let mut buf = Vec::with_capacity(4usize.saturating_add(payload.len()));
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(&payload);
        Ok(buf)
    }

    /// Read the length prefix from a 4-byte buffer.
    pub fn read_frame_len(header: &[u8; 4]) -> usize {
        u32::from_le_bytes(*header) as usize
    }

    /// Return a human-readable tag for logging.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Consensus(ConsensusMessage::Vote(_)) => "vote",
            Self::Consensus(ConsensusMessage::Certificate(_)) => "certificate",
            Self::BlockData(_) => "block_data",
            Self::PeerAnnounce(_) => "peer_announce",
            Self::BlockRequest { .. } => "block_request",
            Self::BlockResponse { .. } => "block_response",
            Self::ValidatorSetUpdate { .. } => "validator_set_update",
            Self::Heartbeat { .. } => "heartbeat",
            Self::HeartbeatAck { .. } => "heartbeat_ack",
        }
    }

    /// Helper: wrap a votor [`VoteMessage`] for transmission.
    pub fn from_vote(vote: VoteMessage) -> Self {
        Self::Consensus(ConsensusMessage::Vote(vote))
    }

    /// Helper: wrap a votor [`Certificate`] for transmission.
    pub fn from_certificate(cert: Certificate) -> Self {
        Self::Consensus(ConsensusMessage::Certificate(cert))
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_heartbeat() {
        let msg = ConsensusNetMessage::Heartbeat {
            pubkey: Pubkey::new_unique(),
            latest_slot: 42,
        };
        let bytes = msg.serialize().unwrap();
        let decoded = ConsensusNetMessage::deserialize(&bytes).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_framed_roundtrip() {
        let msg = ConsensusNetMessage::BlockRequest { height: 100 };
        let framed = msg.serialize_framed(1_048_576).unwrap();
        let len = ConsensusNetMessage::read_frame_len(framed[..4].try_into().unwrap());
        let decoded = ConsensusNetMessage::deserialize(&framed[4..4usize.saturating_add(len)]).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_message_too_large() {
        let msg = ConsensusNetMessage::BlockRequest { height: 1 };
        let result = msg.serialize_framed(1); // absurdly small limit
        assert!(result.is_err());
    }

    #[test]
    fn test_kind_tags() {
        let msg = ConsensusNetMessage::BlockRequest { height: 0 };
        assert_eq!(msg.kind(), "block_request");
    }
}

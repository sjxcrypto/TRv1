//! Peer tracking and connection management for the consensus network.
//!
//! The [`PeerManager`] maintains the set of known peers, their connection
//! state, and liveness metadata.  It is the single source of truth for
//! "who are we talking to?" — the transport layer consults it before
//! sending and updates it on every received message.

use {
    crate::{
        config::ConsensusNetConfig,
        error::{ConsensusNetError, Result},
        message::PeerInfo,
    },
    log::{debug, info, warn},
    solana_pubkey::Pubkey,
    std::{
        collections::{HashMap, HashSet},
        time::Instant,
    },
};

/// Per-peer connection bookkeeping.
#[derive(Debug, Clone)]
pub struct PeerConnection {
    /// Static identity and network address.
    pub info: PeerInfo,
    /// Wall-clock time of the last message received from this peer.
    pub last_seen: Instant,
    /// Counter: messages we have sent *to* this peer.
    pub messages_sent: u64,
    /// Counter: messages we have received *from* this peer.
    pub messages_received: u64,
    /// Exponentially-weighted moving average of round-trip latency (ms).
    pub latency_ms: f64,
    /// Whether we believe the connection is currently alive.
    pub is_connected: bool,
    /// Peer score — higher is better. Starts at 100.
    /// Future: decayed on misbehaviour, boosted on timely delivery.
    pub score: f64,
}

impl PeerConnection {
    /// Create a new connection record for a freshly-discovered peer.
    pub fn new(info: PeerInfo) -> Self {
        Self {
            info,
            last_seen: Instant::now(),
            messages_sent: 0,
            messages_received: 0,
            latency_ms: 0.0,
            is_connected: false,
            score: 100.0,
        }
    }

    /// Record that we received a message from this peer.
    pub fn record_received(&mut self) {
        self.messages_received = self.messages_received.saturating_add(1);
        self.last_seen = Instant::now();
    }

    /// Record that we sent a message to this peer.
    pub fn record_sent(&mut self) {
        self.messages_sent = self.messages_sent.saturating_add(1);
    }

    /// Update the EWMA latency with a new sample.
    pub fn update_latency(&mut self, sample_ms: f64) {
        const ALPHA: f64 = 0.3;
        if self.latency_ms == 0.0 {
            self.latency_ms = sample_ms;
        } else {
            self.latency_ms = ALPHA * sample_ms + (1.0 - ALPHA) * self.latency_ms;
        }
    }

    /// Returns how many seconds since we last heard from this peer.
    pub fn silence_secs(&self) -> u64 {
        self.last_seen.elapsed().as_secs()
    }
}

/// Manages the set of peers on the consensus P2P network.
///
/// Thread-safety note: `PeerManager` is designed to be used behind an
/// `Arc<Mutex<_>>` or equivalent.  The transport layer is responsible for
/// synchronisation.
#[derive(Debug)]
pub struct PeerManager {
    /// Map from validator identity → connection state.
    pub peers: HashMap<Pubkey, PeerConnection>,
    /// The set of validators that are active in the current epoch.
    /// A subset of `peers.keys()`.
    pub active_validators: HashSet<Pubkey>,
    /// Network configuration.
    pub config: ConsensusNetConfig,
}

impl PeerManager {
    /// Create a new, empty peer manager.
    pub fn new(config: ConsensusNetConfig) -> Self {
        Self {
            peers: HashMap::new(),
            active_validators: HashSet::new(),
            config,
        }
    }

    /// Total number of known peers (connected or not).
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Number of peers with `is_connected == true`.
    pub fn connected_count(&self) -> usize {
        self.peers.values().filter(|p| p.is_connected).count()
    }

    // ── Peer lifecycle ──────────────────────────────────────────────────

    /// Register a newly-discovered peer.
    ///
    /// Returns `Err` if we are already at the connection limit.
    /// If the peer is already known, its info is updated in place.
    pub fn add_peer(&mut self, info: PeerInfo) -> Result<()> {
        if let Some(existing) = self.peers.get_mut(&info.pubkey) {
            // Update address / stake if the peer re-announced.
            debug!("updating existing peer {}", info.pubkey);
            existing.info = info;
            return Ok(());
        }

        if self.peers.len() >= self.config.max_peers {
            return Err(ConsensusNetError::MaxPeersReached(self.config.max_peers));
        }

        let pubkey = info.pubkey;
        let is_validator = info.is_active_validator;
        info!("adding peer {} (validator={})", pubkey, is_validator);
        self.peers.insert(pubkey, PeerConnection::new(info));
        if is_validator {
            self.active_validators.insert(pubkey);
        }
        Ok(())
    }

    /// Remove a peer entirely.
    pub fn remove_peer(&mut self, pubkey: &Pubkey) {
        if self.peers.remove(pubkey).is_some() {
            self.active_validators.remove(pubkey);
            info!("removed peer {}", pubkey);
        }
    }

    /// Mark a peer as connected.
    pub fn mark_connected(&mut self, pubkey: &Pubkey) -> Result<()> {
        let conn = self
            .peers
            .get_mut(pubkey)
            .ok_or(ConsensusNetError::UnknownPeer(*pubkey))?;
        conn.is_connected = true;
        conn.last_seen = Instant::now();
        Ok(())
    }

    /// Mark a peer as disconnected.
    pub fn mark_disconnected(&mut self, pubkey: &Pubkey) {
        if let Some(conn) = self.peers.get_mut(pubkey) {
            conn.is_connected = false;
        }
    }

    // ── Queries ─────────────────────────────────────────────────────────

    /// Get an immutable reference to a peer's state.
    pub fn get_peer(&self, pubkey: &Pubkey) -> Option<&PeerConnection> {
        self.peers.get(pubkey)
    }

    /// Get a mutable reference to a peer's state.
    pub fn get_peer_mut(&mut self, pubkey: &Pubkey) -> Option<&mut PeerConnection> {
        self.peers.get_mut(pubkey)
    }

    /// Iterator over connected active validators.
    pub fn connected_validators(&self) -> impl Iterator<Item = (&Pubkey, &PeerConnection)> {
        self.peers
            .iter()
            .filter(|(k, v)| v.is_connected && self.active_validators.contains(k))
    }

    /// Iterator over all connected peers.
    pub fn connected_peers(&self) -> impl Iterator<Item = (&Pubkey, &PeerConnection)> {
        self.peers.iter().filter(|(_, v)| v.is_connected)
    }

    /// Return the pubkeys of all connected active validators.
    pub fn connected_validator_keys(&self) -> Vec<Pubkey> {
        self.connected_validators().map(|(k, _)| *k).collect()
    }

    // ── Validator set management ────────────────────────────────────────

    /// Replace the active validator set (called at epoch boundaries).
    pub fn update_active_validators(&mut self, validators: HashSet<Pubkey>) {
        info!(
            "validator set update: {} → {} validators",
            self.active_validators.len(),
            validators.len()
        );
        self.active_validators = validators;
    }

    // ── Liveness / garbage collection ───────────────────────────────────

    /// Evict peers that have been silent for longer than `peer_timeout_secs`.
    ///
    /// Returns the pubkeys of peers that were disconnected.
    pub fn evict_stale_peers(&mut self) -> Vec<Pubkey> {
        let timeout = self.config.peer_timeout_secs;
        let stale: Vec<Pubkey> = self
            .peers
            .iter()
            .filter(|(_, v)| v.is_connected && v.silence_secs() > timeout)
            .map(|(k, _)| *k)
            .collect();

        for pubkey in &stale {
            warn!("evicting stale peer {} (silent >{}s)", pubkey, timeout);
            if let Some(conn) = self.peers.get_mut(pubkey) {
                conn.is_connected = false;
            }
        }
        stale
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::config::ConsensusNetConfig,
        std::net::SocketAddr,
    };

    fn test_peer(n: u8) -> PeerInfo {
        PeerInfo {
            pubkey: Pubkey::new_unique(),
            addr: SocketAddr::from(([127, 0, 0, n], 8900)),
            stake_weight: 1_000_000,
            is_active_validator: true,
        }
    }

    #[test]
    fn test_add_and_query_peer() {
        let mut pm = PeerManager::new(ConsensusNetConfig::dev_default());
        let info = test_peer(1);
        let pk = info.pubkey;
        pm.add_peer(info).unwrap();
        assert_eq!(pm.peer_count(), 1);
        assert!(pm.get_peer(&pk).is_some());
        assert!(pm.active_validators.contains(&pk));
    }

    #[test]
    fn test_max_peers_enforced() {
        let mut cfg = ConsensusNetConfig::dev_default();
        cfg.max_peers = 2;
        let mut pm = PeerManager::new(cfg);
        pm.add_peer(test_peer(1)).unwrap();
        pm.add_peer(test_peer(2)).unwrap();
        assert!(pm.add_peer(test_peer(3)).is_err());
    }

    #[test]
    fn test_remove_peer() {
        let mut pm = PeerManager::new(ConsensusNetConfig::dev_default());
        let info = test_peer(1);
        let pk = info.pubkey;
        pm.add_peer(info).unwrap();
        pm.remove_peer(&pk);
        assert_eq!(pm.peer_count(), 0);
        assert!(!pm.active_validators.contains(&pk));
    }

    #[test]
    fn test_connected_validators_filter() {
        let mut pm = PeerManager::new(ConsensusNetConfig::dev_default());
        let p1 = test_peer(1);
        let p2 = test_peer(2);
        let pk1 = p1.pubkey;
        let pk2 = p2.pubkey;
        pm.add_peer(p1).unwrap();
        pm.add_peer(p2).unwrap();
        pm.mark_connected(&pk1).unwrap();
        // pk2 stays disconnected
        let connected: Vec<_> = pm.connected_validator_keys();
        assert_eq!(connected.len(), 1);
        assert_eq!(connected[0], pk1);
        let _ = pk2;
    }

    #[test]
    fn test_latency_ewma() {
        let mut conn = PeerConnection::new(test_peer(1));
        conn.update_latency(100.0);
        assert!((conn.latency_ms - 100.0).abs() < f64::EPSILON);
        conn.update_latency(200.0);
        // 0.3 * 200 + 0.7 * 100 = 130
        assert!((conn.latency_ms - 130.0).abs() < f64::EPSILON);
    }
}

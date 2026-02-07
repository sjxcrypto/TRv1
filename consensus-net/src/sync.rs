//! Block sync / catch-up protocol.
//!
//! When a validator restarts or falls behind, it needs to fetch blocks it
//! missed from its peers.  This module implements a simple request/response
//! protocol on top of the consensus transport:
//!
//! 1. **Gap detection** — the validator compares its local tip with the
//!    latest slot heard on heartbeats and identifies missing heights.
//! 2. **Request dispatch** — for each missing height a `BlockRequest` is
//!    sent to a randomly-chosen connected peer (with bounded concurrency).
//! 3. **Response handling** — `BlockResponse` messages are matched to
//!    pending requests and handed to the ledger layer for validation and
//!    replay.
//! 4. **Retry / failover** — if a response doesn't arrive within the
//!    configured timeout the request is retried against a different peer.

use {
    crate::{
        config::ConsensusNetConfig,
        error::{ConsensusNetError, Result},
        message::{BlockData, ConsensusNetMessage},
        peer_manager::PeerManager,
        transport,
    },
    log::{debug, info, warn},
    solana_pubkey::Pubkey,
    std::{
        collections::{HashMap, HashSet},
        net::SocketAddr,
        sync::{Arc, Mutex},
        time::Instant,
    },
};

/// Tracks an outstanding block-sync request.
#[derive(Debug, Clone)]
pub struct PendingSyncRequest {
    /// The height being requested.
    pub height: u64,
    /// The peer we sent the request to.
    pub peer: Pubkey,
    /// The peer's socket address (needed for retries).
    pub addr: SocketAddr,
    /// When the request was dispatched.
    pub sent_at: Instant,
    /// How many times we have tried this height.
    pub attempts: u32,
}

/// Orchestrates block catch-up across the peer set.
pub struct BlockSyncer {
    /// Shared peer manager — used to pick targets.
    peer_manager: Arc<Mutex<PeerManager>>,
    /// Config (timeouts, concurrency).
    config: ConsensusNetConfig,
    /// Heights we are currently waiting for a response on.
    pending: HashMap<u64, PendingSyncRequest>,
    /// Heights we have already received and don't need to request again.
    completed: HashSet<u64>,
    /// Maximum retry attempts per height before giving up.
    max_retries: u32,
}

impl BlockSyncer {
    /// Create a new syncer.
    pub fn new(peer_manager: Arc<Mutex<PeerManager>>, config: ConsensusNetConfig) -> Self {
        Self {
            peer_manager,
            config,
            pending: HashMap::new(),
            completed: HashSet::new(),
            max_retries: 5,
        }
    }

    /// The number of requests currently in flight.
    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }

    /// Whether a height is already completed (no need to request).
    pub fn is_completed(&self, height: u64) -> bool {
        self.completed.contains(&height)
    }

    /// Request a range of missing blocks.
    ///
    /// Skips heights that are already pending or completed.  Returns the
    /// number of new requests actually dispatched.
    pub async fn request_range(&mut self, from: u64, to: u64) -> usize {
        let mut dispatched = 0usize;
        for height in from..=to {
            if self.completed.contains(&height) || self.pending.contains_key(&height) {
                continue;
            }
            if self.pending.len() >= self.config.max_sync_requests {
                debug!(
                    "sync concurrency limit reached ({}), deferring height {}",
                    self.config.max_sync_requests, height
                );
                break;
            }
            if self.dispatch_request(height).await.is_ok() {
                dispatched = dispatched.saturating_add(1);
            }
        }
        if dispatched > 0 {
            info!("dispatched {} block-sync requests ({} → {})", dispatched, from, to);
        }
        dispatched
    }

    /// Pick a peer and send a `BlockRequest` for the given height.
    async fn dispatch_request(&mut self, height: u64) -> Result<()> {
        let (peer, addr) = self.pick_peer()?;

        let msg = ConsensusNetMessage::BlockRequest { height };
        transport::send_message(addr, &msg, self.config.max_message_size).await?;

        self.pending.insert(
            height,
            PendingSyncRequest {
                height,
                peer,
                addr,
                sent_at: Instant::now(),
                attempts: 1,
            },
        );

        debug!("requested block {} from {} ({})", height, peer, addr);
        Ok(())
    }

    /// Called when a `BlockResponse` is received.
    ///
    /// Returns `Some(BlockData)` if the response matches a pending request,
    /// `None` if it was unsolicited.
    pub fn handle_response(&mut self, height: u64, block: BlockData) -> Option<BlockData> {
        if let Some(_req) = self.pending.remove(&height) {
            self.completed.insert(height);
            debug!("received block {} — sync complete for height", height);
            Some(block)
        } else {
            warn!("unsolicited block response for height {}", height);
            None
        }
    }

    /// Check for timed-out requests and retry them against different peers.
    ///
    /// Returns heights that have permanently failed (exceeded `max_retries`).
    pub async fn retry_timed_out(&mut self) -> Vec<u64> {
        let timeout_ms = self.config.message_timeout_ms;
        let timed_out: Vec<PendingSyncRequest> = self
            .pending
            .values()
            .filter(|r| r.sent_at.elapsed().as_millis() as u64 > timeout_ms)
            .cloned()
            .collect();

        let mut permanently_failed = Vec::new();

        for req in timed_out {
            self.pending.remove(&req.height);

            if req.attempts >= self.max_retries {
                warn!(
                    "giving up on block {} after {} attempts",
                    req.height, req.attempts
                );
                permanently_failed.push(req.height);
                continue;
            }

            // Retry with a (possibly different) peer.
            match self.pick_peer() {
                Ok((peer, addr)) => {
                    let msg = ConsensusNetMessage::BlockRequest {
                        height: req.height,
                    };
                    if transport::send_message(addr, &msg, self.config.max_message_size)
                        .await
                        .is_ok()
                    {
                        self.pending.insert(
                            req.height,
                            PendingSyncRequest {
                                height: req.height,
                                peer,
                                addr,
                                sent_at: Instant::now(),
                                attempts: req.attempts.saturating_add(1),
                            },
                        );
                        debug!(
                            "retried block {} with peer {} (attempt {})",
                            req.height,
                            peer,
                            req.attempts.saturating_add(1)
                        );
                    } else {
                        permanently_failed.push(req.height);
                    }
                }
                Err(_) => {
                    warn!("no peers available for retry of block {}", req.height);
                    permanently_failed.push(req.height);
                }
            }
        }

        permanently_failed
    }

    /// Reset state for a new sync session (e.g. after a restart).
    pub fn reset(&mut self) {
        self.pending.clear();
        self.completed.clear();
    }

    // ── Internals ───────────────────────────────────────────────────────

    /// Choose a random connected peer to send a request to.
    fn pick_peer(&self) -> Result<(Pubkey, SocketAddr)> {
        let pm = self
            .peer_manager
            .lock()
            .map_err(|_| ConsensusNetError::ChannelClosed)?;

        // Prefer active validators, fall back to any connected peer.
        let candidates: Vec<_> = pm.connected_validators().collect();
        if candidates.is_empty() {
            let all_connected: Vec<_> = pm.connected_peers().collect();
            if all_connected.is_empty() {
                return Err(ConsensusNetError::InvalidMessage(
                    "no connected peers for sync".into(),
                ));
            }
            // Simple round-robin via index; in production, weight by latency.
            let idx = (Instant::now().elapsed().subsec_nanos() as usize) % all_connected.len();
            let (pk, conn) = all_connected[idx];
            return Ok((*pk, conn.info.addr));
        }

        let idx = (Instant::now().elapsed().subsec_nanos() as usize) % candidates.len();
        let (pk, conn) = candidates[idx];
        Ok((*pk, conn.info.addr))
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{config::ConsensusNetConfig, peer_manager::PeerManager},
        solana_hash::Hash,
    };

    fn make_block_data(slot: u64) -> BlockData {
        BlockData {
            slot,
            parent_hash: Hash::default(),
            block_hash: Hash::new_unique(),
            transactions: vec![],
            state_root: Hash::default(),
            merkle_root: Hash::default(),
            merkle_proof: vec![],
            proposer: Pubkey::new_unique(),
        }
    }

    #[test]
    fn test_handle_response_completes() {
        let pm = Arc::new(Mutex::new(PeerManager::new(ConsensusNetConfig::dev_default())));
        let mut syncer = BlockSyncer::new(pm, ConsensusNetConfig::dev_default());

        // Simulate a pending request.
        let peer = Pubkey::new_unique();
        syncer.pending.insert(
            10,
            PendingSyncRequest {
                height: 10,
                peer,
                addr: "127.0.0.1:8900".parse().unwrap(),
                sent_at: Instant::now(),
                attempts: 1,
            },
        );

        let block = make_block_data(10);
        let result = syncer.handle_response(10, block);
        assert!(result.is_some());
        assert!(syncer.is_completed(10));
        assert_eq!(syncer.in_flight(), 0);
    }

    #[test]
    fn test_unsolicited_response_ignored() {
        let pm = Arc::new(Mutex::new(PeerManager::new(ConsensusNetConfig::dev_default())));
        let mut syncer = BlockSyncer::new(pm, ConsensusNetConfig::dev_default());

        let block = make_block_data(99);
        assert!(syncer.handle_response(99, block).is_none());
    }
}

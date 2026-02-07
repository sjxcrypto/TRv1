# TRv1 Consensus Networking Layer

## Overview

TRv1 replaces Solana's PoH-based Turbine block propagation with a dedicated
consensus networking layer designed for Tendermint-style BFT. The `consensus-net`
crate provides all peer-to-peer communication for:

- BFT consensus messages (votes and certificates)
- Block propagation from proposers to validators
- Peer discovery and connection management
- Block catch-up / sync for validators that fall behind

The gossip protocol (`solana-gossip`) is retained for cluster-wide metadata
dissemination (contact info, version, epoch schedules), but consensus-critical
messages travel on a separate, dedicated channel.

---

## Network Topology

### Full Mesh (Active Validators)

TRv1 targets **200 active validators** per epoch. All active validators
maintain direct connections to each other, forming a full mesh:

```
V1 ←→ V2 ←→ V3
 ↕    ↗↘    ↕
V4 ←→ V5 ←→ V6
       …
     (200 nodes)
```

**Why full mesh?**  With 200 validators, full mesh requires 200 × 199 / 2 =
19,900 connections — manageable with modern QUIC/TCP stacks. The benefits:

- **Minimal latency** — every consensus message is one hop.
- **No relay bottleneck** — no single node is on the critical path.
- **Simplicity** — no tree construction or routing tables needed for
  consensus messages.

Non-validator nodes (RPC, watchers) connect to a subset of validators and
receive block data passively. They do not participate in the consensus mesh.

### Gossip Layer (Retained)

The existing Solana gossip protocol continues to handle:

| Data | Protocol | Reason |
|------|----------|--------|
| Contact info (IP, ports) | Gossip | Cluster-wide, eventually-consistent |
| Validator identity / version | Gossip | Non-time-critical |
| Epoch schedule | Gossip | Disseminated before boundary |
| **Consensus votes / certs** | **consensus-net** | **Latency-critical, BFT** |
| **Block data** | **consensus-net** | **Large payload, proposer→all** |

---

## Message Types and Sizes

All messages are serialized with **bincode** and framed with a 4-byte
little-endian length prefix:

```
[4 bytes: u32-le payload length][N bytes: bincode payload]
```

| Message | Typical Size | Direction | Frequency |
|---------|-------------|-----------|-----------|
| `Vote` (single BFT vote) | ~200 B | Validator → All | Every slot |
| `Certificate` (aggregated) | ~500 B – 2 KB | Aggregator → All | Per certificate |
| `BlockData` | 10 KB – 1 MB | Proposer → All | Per slot |
| `PeerAnnounce` | ~100 B | Any → Any | On connect / epoch |
| `BlockRequest` | ~16 B | Syncing node → Peer | During catch-up |
| `BlockResponse` | 10 KB – 1 MB | Peer → Syncing node | During catch-up |
| `ValidatorSetUpdate` | ~20 KB | Epoch boundary | Once per epoch |
| `Heartbeat` / `HeartbeatAck` | ~48 B | Bidirectional | Every 500 ms |

**Maximum message size: 1 MB** (configurable via `ConsensusNetConfig::max_message_size`).

### ConsensusNetMessage Enum

```rust
pub enum ConsensusNetMessage {
    Consensus(ConsensusMessage),      // votes + certificates from votor
    BlockData(BlockData),              // full block from proposer
    PeerAnnounce(PeerInfo),            // peer discovery
    BlockRequest { height: u64 },      // catch-up request
    BlockResponse { height: u64, block: BlockData },
    ValidatorSetUpdate { epoch: u64, validators: Vec<ValidatorInfo> },
    Heartbeat { pubkey, latest_slot },
    HeartbeatAck { pubkey, latest_slot },
}
```

---

## Transport

### Primary: TCP (current implementation)

The initial implementation uses **TCP with length-prefixed framing**:

- One TCP connection per peer (persistent, bidirectional).
- Messages are serialized, length-prefixed (4 bytes LE), and written to the stream.
- The receiver reads the 4-byte header, validates the length, reads the payload, and deserializes.

### Planned: QUIC

QUIC is the preferred long-term transport for several reasons:

| Feature | TCP | QUIC |
|---------|-----|------|
| Connection setup | 1-3 RTT (+ TLS) | 0-1 RTT |
| Head-of-line blocking | Yes | No (stream multiplexing) |
| Connection migration | No | Yes (IP changes) |
| Built-in encryption | No (needs TLS wrapper) | Yes |
| Congestion control | Per-connection | Per-stream |

The `ConsensusNetConfig::prefer_quic` flag enables QUIC when available.
The `quinn` crate (already in the workspace) will be used.

### Port Allocation

| Service | Default Port | Protocol |
|---------|-------------|----------|
| Consensus P2P | 8900 | TCP / QUIC |
| Gossip | 8000 | UDP |
| RPC | 8899 | HTTP |
| TPU | 8004 | QUIC / UDP |

---

## Block Propagation Protocol

### Normal Operation (Proposer Broadcast)

```
Proposer                    Validators (V1…V200)
   │                              │
   │  BlockData(slot, txs,        │
   │  state_root, merkle_proof)   │
   ├─────────────────────────────→│  (broadcast to all active validators)
   │                              │
   │                              │── validate merkle_root
   │                              │── replay transactions
   │                              │── vote (Notarize/Finalize)
   │                              │
   │←─────────────────────────────┤  Vote(slot, block_id)
```

The proposer sends `BlockData` containing:
- Slot number and parent hash
- Serialized transactions
- Post-execution state root
- Merkle root over transactions
- Merkle proof nodes for independent verification

### Block Validation

Receiving validators verify the block before voting:

1. **Merkle proof** — recompute the transaction Merkle root from the proof nodes
   and verify it matches `merkle_root` in the `BlockData`.
2. **Parent linkage** — confirm `parent_hash` matches a known notarized block.
3. **Replay** — execute transactions against local state and compare the resulting
   state root with the `state_root` in the block.

---

## Block Sync Protocol

When a validator is behind (restart, network partition, etc.), the `BlockSyncer`
handles catch-up:

```
Syncing Node                    Peer
     │                            │
     │  1. Detect gap via         │
     │     heartbeat.latest_slot  │
     │                            │
     │  BlockRequest { height }   │
     ├───────────────────────────→│
     │                            │
     │  BlockResponse { height,   │
     │    block: BlockData }      │
     │←───────────────────────────┤
     │                            │
     │  2. Validate + replay      │
     │  3. Request next height    │
     │     (bounded concurrency)  │
```

### Sync Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `max_sync_requests` | 16 | Max concurrent in-flight requests |
| `message_timeout_ms` | 5,000 | Timeout before retry |
| `max_retries` | 5 | Attempts per height before giving up |

### Retry Logic

1. If a `BlockResponse` is not received within `message_timeout_ms`, the
   request is re-dispatched to a **different** randomly-chosen peer.
2. After `max_retries` (5) failures for a single height, the height is
   reported as permanently failed and the syncer moves on.
3. Completed heights are tracked to avoid duplicate requests.

---

## Peer Management

### PeerManager

The `PeerManager` is the single source of truth for connection state:

```rust
pub struct PeerManager {
    peers: HashMap<Pubkey, PeerConnection>,
    active_validators: HashSet<Pubkey>,
    config: ConsensusNetConfig,
}
```

### Peer Lifecycle

1. **Discovery** — peers are discovered via gossip `PeerAnnounce` messages
   or validator-set updates at epoch boundaries.
2. **Connection** — the transport layer establishes a TCP/QUIC connection
   and calls `mark_connected()`.
3. **Heartbeat** — every 500 ms, a `Heartbeat` is sent; receiving a
   `HeartbeatAck` updates `last_seen` and latency.
4. **Eviction** — peers silent for > `peer_timeout_secs` (30s) are marked
   disconnected by `evict_stale_peers()`.

### Connection Metrics (per peer)

| Metric | Type | Purpose |
|--------|------|---------|
| `messages_sent` | Counter | Total messages sent to peer |
| `messages_received` | Counter | Total messages received from peer |
| `latency_ms` | EWMA (α=0.3) | Round-trip latency |
| `last_seen` | Timestamp | Liveness detection |
| `score` | Float (0–100) | Reputation (future use) |

---

## Peer Scoring (Future Reputation System)

The `score` field in `PeerConnection` is a placeholder for a future
reputation system. The planned design:

### Score Dimensions

| Dimension | Weight | How It's Measured |
|-----------|--------|-------------------|
| **Liveness** | 30% | Heartbeat response rate |
| **Latency** | 20% | EWMA round-trip time |
| **Block delivery** | 25% | % of blocks delivered promptly |
| **Vote delivery** | 15% | % of votes received within timeout |
| **Misbehaviour** | 10% | Invalid messages, equivocation |

### Score Actions

- **Score > 80**: Preferred peer for sync requests.
- **Score 50–80**: Normal peer, no special treatment.
- **Score < 50**: Deprioritised — sync requests avoid this peer.
- **Score < 20**: Disconnected and blacklisted for the epoch.

### Decay

Scores decay toward 50 (neutral) over time to allow recovery:

```
score = score * 0.99 + 50 * 0.01   // per heartbeat interval
```

---

## Security Considerations

1. **Message authentication** — consensus messages (votes, certificates)
   carry BLS signatures verified by the votor engine. The networking
   layer does not re-verify signatures; it trusts the consensus engine.

2. **DoS protection** — `max_message_size` (1 MB) prevents memory
   exhaustion. The `max_peers` limit (200) caps connection count.

3. **Peer identity** — peers are identified by their Ed25519 pubkey
   (same as their validator identity). In the future, TLS/QUIC mutual
   authentication will bind the transport-level identity to the
   validator identity.

4. **Eclipse attacks** — the full-mesh topology makes eclipse attacks
   impractical for the active validator set. An attacker would need to
   compromise the gossip layer AND control > 2/3 of connections.

---

## Module Structure

```
consensus-net/
├── Cargo.toml
└── src/
    ├── lib.rs           — Crate root, module declarations, architecture docs
    ├── config.rs        — ConsensusNetConfig with defaults and dev overrides
    ├── error.rs         — ConsensusNetError enum (thiserror)
    ├── message.rs       — Wire types, bincode ser/de, framing helpers
    ├── peer_manager.rs  — Peer lifecycle, liveness, validator-set tracking
    ├── transport.rs     — TCP listener, send/broadcast helpers
    └── sync.rs          — Block catch-up request/response protocol
```

---

## Future Work

- [ ] **QUIC transport** — replace TCP with quinn-based QUIC for 0-RTT,
      stream multiplexing, and built-in encryption.
- [ ] **Persistent connections** — connection pool with automatic reconnect
      instead of per-message TCP connections for outbound sends.
- [ ] **Peer scoring** — implement the reputation system described above.
- [ ] **Bandwidth accounting** — track bytes sent/received per peer for
      fair resource allocation.
- [ ] **Erasure coding for blocks** — large blocks could be split into
      erasure-coded shards (like Turbine) for more efficient propagation.
- [ ] **Message batching** — batch multiple small messages (votes) into
      a single frame to reduce syscall overhead.
- [ ] **Priority queues** — consensus votes should preempt block data
      on congested links.

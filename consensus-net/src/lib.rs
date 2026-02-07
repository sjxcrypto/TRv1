#![cfg(feature = "agave-unstable-api")]
//! TRv1 Consensus Networking Layer
//!
//! This crate provides the peer-to-peer networking infrastructure for
//! TRv1's Tendermint-style BFT consensus.  It handles:
//!
//! - **Consensus message propagation** — votes, certificates, and
//!   validator-set updates are serialized with bincode and delivered over
//!   length-prefixed TCP streams (QUIC upgrade path planned).
//! - **Block propagation** — the proposer broadcasts committed blocks to
//!   all active validators; peers can also request blocks they missed.
//! - **Peer management** — connection tracking, heartbeats, liveness
//!   detection, and epoch-boundary validator-set updates.
//! - **Block sync** — a catch-up protocol that requests missing blocks
//!   from peers with bounded concurrency and automatic retry.
//!
//! ## Architecture
//!
//! ```text
//!  ┌─────────────────────────────────────────────────┐
//!  │  Votor (BFT consensus engine)                   │
//!  │  ← ConsensusMessage (votes, certs)              │
//!  │  → ConsensusMessage (votes, certs)              │
//!  └──────────────┬──────────────────────────────────┘
//!                 │  crossbeam channels
//!  ┌──────────────▼──────────────────────────────────┐
//!  │  Message Router (lib.rs / future router module) │
//!  │  • dispatches inbound messages by type          │
//!  │  • wraps outbound consensus msgs for transport  │
//!  └──────┬───────────────────┬──────────────────────┘
//!         │                   │
//!  ┌──────▼──────┐     ┌─────▼──────┐
//!  │ PeerManager │     │ BlockSyncer│
//!  │ (peers, HB) │     │ (catch-up) │
//!  └──────┬──────┘     └─────┬──────┘
//!         │                   │
//!  ┌──────▼───────────────────▼──────────────────────┐
//!  │  Transport (TCP, length-prefixed frames)        │
//!  │  • TransportListener — accepts inbound          │
//!  │  • send_message / broadcast_message — outbound  │
//!  └─────────────────────────────────────────────────┘
//! ```
//!
//! ## Crate modules
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`config`]       | `ConsensusNetConfig` defaults and dev overrides |
//! | [`message`]      | Wire types, bincode ser/de, framing helpers |
//! | [`peer_manager`] | Peer lifecycle, liveness, validator-set tracking |
//! | [`transport`]    | TCP listener, send/broadcast helpers |
//! | [`sync`]         | Block catch-up request/response protocol |
//! | [`error`]        | Crate-wide error enum |

pub mod config;
pub mod error;
pub mod message;
pub mod peer_manager;
pub mod sync;
pub mod transport;

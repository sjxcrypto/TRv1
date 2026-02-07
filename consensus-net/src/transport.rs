//! TCP/QUIC transport layer for consensus message delivery.
//!
//! This module provides the low-level plumbing for sending and receiving
//! [`ConsensusNetMessage`]s between validators.  The design favours TCP for
//! the initial implementation (simpler, debuggable) with a QUIC upgrade path.
//!
//! ## Wire format
//!
//! Every message on the wire is length-prefixed:
//!
//! ```text
//! [4 bytes: payload length (u32-le)] [N bytes: bincode payload]
//! ```
//!
//! The transport reads the 4-byte header, validates the length against
//! `max_message_size`, then reads exactly that many bytes and hands the
//! resulting [`ConsensusNetMessage`] to the message router.

use {
    crate::{
        config::ConsensusNetConfig,
        error::{ConsensusNetError, Result},
        message::ConsensusNetMessage,
    },
    log::{debug, error, info, warn},
    std::net::SocketAddr,
    tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::mpsc,
    },
};

/// A received message together with the address it came from.
#[derive(Debug)]
pub struct InboundMessage {
    /// The decoded message.
    pub message: ConsensusNetMessage,
    /// The remote socket address that sent it.
    pub from: SocketAddr,
}

/// Handle returned by [`TransportListener::start`] to control the listener.
pub struct TransportHandle {
    /// Channel that delivers every inbound message to the router.
    pub inbound_rx: mpsc::Receiver<InboundMessage>,
    /// The local address the listener is bound to (useful when port = 0).
    pub local_addr: SocketAddr,
}

/// Listens for inbound TCP connections and delivers decoded messages.
pub struct TransportListener {
    config: ConsensusNetConfig,
}

impl TransportListener {
    /// Create a new transport listener with the given config.
    pub fn new(config: ConsensusNetConfig) -> Self {
        Self { config }
    }

    /// Bind and start accepting connections.
    ///
    /// Returns a [`TransportHandle`] whose `inbound_rx` yields every
    /// successfully decoded message.  Spawns a Tokio task per accepted
    /// connection.
    pub async fn start(self) -> Result<TransportHandle> {
        let listener = TcpListener::bind(self.config.bind_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("consensus transport listening on {}", local_addr);

        let (tx, rx) = mpsc::channel::<InboundMessage>(self.config.channel_buffer_size);
        let max_msg = self.config.max_message_size;

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        debug!("accepted consensus connection from {}", addr);
                        let tx = tx.clone();
                        tokio::spawn(Self::handle_connection(stream, addr, tx, max_msg));
                    }
                    Err(e) => {
                        error!("accept error: {}", e);
                    }
                }
            }
        });

        Ok(TransportHandle {
            inbound_rx: rx,
            local_addr,
        })
    }

    /// Read length-prefixed messages from `stream` until EOF or error.
    async fn handle_connection(
        mut stream: TcpStream,
        addr: SocketAddr,
        tx: mpsc::Sender<InboundMessage>,
        max_message_size: usize,
    ) {
        let mut header_buf = [0u8; 4];

        loop {
            // 1. Read the 4-byte length prefix.
            if let Err(e) = stream.read_exact(&mut header_buf).await {
                if e.kind() != std::io::ErrorKind::UnexpectedEof {
                    warn!("header read error from {}: {}", addr, e);
                }
                break;
            }

            let len = ConsensusNetMessage::read_frame_len(&header_buf);
            if len > max_message_size {
                warn!(
                    "peer {} sent oversized frame ({} > {}), dropping connection",
                    addr, len, max_message_size
                );
                break;
            }

            // 2. Read the payload.
            let mut payload = vec![0u8; len];
            if let Err(e) = stream.read_exact(&mut payload).await {
                warn!("payload read error from {}: {}", addr, e);
                break;
            }

            // 3. Deserialize.
            match ConsensusNetMessage::deserialize(&payload) {
                Ok(message) => {
                    debug!("received {} from {}", message.kind(), addr);
                    if tx
                        .send(InboundMessage {
                            message,
                            from: addr,
                        })
                        .await
                        .is_err()
                    {
                        // Router dropped — shut down gracefully.
                        info!("inbound channel closed, stopping reader for {}", addr);
                        break;
                    }
                }
                Err(e) => {
                    warn!("deserialization error from {}: {}", addr, e);
                    // Skip this message but keep the connection alive — the
                    // peer may be running a slightly different version.
                }
            }
        }

        debug!("connection to {} closed", addr);
    }
}

// ── Outbound sending ────────────────────────────────────────────────────────

/// Send a single framed message to the given address over a new TCP connection.
///
/// For production use the caller should maintain persistent connections;
/// this helper is useful for one-shot sends and tests.
pub async fn send_message(
    addr: SocketAddr,
    msg: &ConsensusNetMessage,
    max_message_size: usize,
) -> Result<()> {
    let frame = msg.serialize_framed(max_message_size)?;
    let mut stream = TcpStream::connect(addr).await?;
    stream.write_all(&frame).await?;
    stream.flush().await?;
    Ok(())
}

/// Send a framed message over an *existing* TCP stream.
pub async fn send_on_stream(
    stream: &mut TcpStream,
    msg: &ConsensusNetMessage,
    max_message_size: usize,
) -> Result<()> {
    let frame = msg.serialize_framed(max_message_size)?;
    stream.write_all(&frame).await?;
    stream.flush().await?;
    Ok(())
}

/// Broadcast a message to multiple addresses concurrently.
///
/// Returns the list of addresses where sending failed.
pub async fn broadcast_message(
    addrs: &[SocketAddr],
    msg: &ConsensusNetMessage,
    max_message_size: usize,
) -> Vec<(SocketAddr, ConsensusNetError)> {
    let frame = match msg.serialize_framed(max_message_size) {
        Ok(f) => f,
        Err(e) => {
            // If we can't even serialize, return an error for every target.
            return addrs
                .iter()
                .map(|a| {
                    (
                        *a,
                        ConsensusNetError::InvalidMessage(format!("serialize failed: {e}")),
                    )
                })
                .collect();
        }
    };

    let mut handles = Vec::with_capacity(addrs.len());
    for &addr in addrs {
        let frame = frame.clone();
        handles.push(tokio::spawn(async move {
            let result = async {
                let mut stream = TcpStream::connect(addr).await?;
                stream.write_all(&frame).await?;
                stream.flush().await?;
                Ok::<(), std::io::Error>(())
            }
            .await;
            (addr, result)
        }));
    }

    let mut failures = Vec::new();
    for handle in handles {
        if let Ok((addr, Err(e))) = handle.await {
            failures.push((addr, ConsensusNetError::Transport(e)));
        }
    }
    failures
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use {super::*, crate::config::ConsensusNetConfig, solana_pubkey::Pubkey};

    #[tokio::test]
    async fn test_listener_and_send() {
        let cfg = ConsensusNetConfig::dev_default();
        let listener = TransportListener::new(cfg.clone());
        let mut handle = listener.start().await.unwrap();
        let addr = handle.local_addr;

        let msg = ConsensusNetMessage::Heartbeat {
            pubkey: Pubkey::new_unique(),
            latest_slot: 99,
        };

        send_message(addr, &msg, cfg.max_message_size).await.unwrap();

        let received = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            handle.inbound_rx.recv(),
        )
        .await
        .expect("timeout waiting for message")
        .expect("channel empty");

        assert_eq!(received.message, msg);
    }

    #[tokio::test]
    async fn test_broadcast() {
        let cfg = ConsensusNetConfig::dev_default();
        let listener = TransportListener::new(cfg.clone());
        let mut handle = listener.start().await.unwrap();
        let addr = handle.local_addr;

        let msg = ConsensusNetMessage::BlockRequest { height: 42 };

        let failures = broadcast_message(&[addr], &msg, cfg.max_message_size).await;
        assert!(failures.is_empty(), "broadcast had failures: {:?}", failures);

        let received = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            handle.inbound_rx.recv(),
        )
        .await
        .expect("timeout")
        .expect("empty");
        assert_eq!(received.message, msg);
    }
}

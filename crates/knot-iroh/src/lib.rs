//! Isolated Iroh transport for the knot-sync protocol.
use async_trait::async_trait;
pub use iroh::SecretKey;
use iroh::{Endpoint, EndpointAddr, EndpointId};
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;
use tokio::{
    io::AsyncReadExt,
    sync::{Mutex, mpsc},
};

pub const ALPN: &[u8] = b"knot-sync/1";
pub const PROTOCOL_VERSION: u8 = 1;
pub const MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

/// Generate a cryptographically random identity for first-run persistence.
pub fn generate_secret_key() -> SecretKey {
    SecretKey::generate()
}

/// Serialize an identity for storage in an application-private binary file.
///
/// These bytes are secret key material and must not be logged or stored in a
/// world-readable location.
pub fn secret_key_to_bytes(key: &SecretKey) -> [u8; 32] {
    key.to_bytes()
}

/// Restore an identity from its exact 32-byte persisted representation.
pub fn secret_key_from_bytes(bytes: &[u8]) -> Result<SecretKey, iroh::KeyParsingError> {
    SecretKey::try_from(bytes)
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("message exceeds {MAX_MESSAGE_SIZE} bytes")]
    MessageTooLarge,
    #[error("malformed frame: {0}")]
    Malformed(String),
    #[error("peer disconnected")]
    Disconnected,
    #[error("iroh: {0}")]
    Iroh(#[from] iroh::endpoint::ConnectError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    pub peer: EndpointId,
    pub payload: Vec<u8>,
}

/// Stable boundary used by knot-sync; no Iroh types need escape this crate in production use.
#[async_trait]
pub trait RawPeerTransport: Send + Sync {
    async fn connect(&self, peer: EndpointAddr) -> Result<EndpointId, TransportError>;
    async fn send(&self, peer: EndpointId, payload: &[u8]) -> Result<(), TransportError>;
    async fn recv(&self) -> Result<Received, TransportError>;
}
type PeerReceiver = mpsc::UnboundedReceiver<Result<Vec<u8>, TransportError>>;

type IncomingReceiver = mpsc::Receiver<Result<Received, TransportError>>;
type IncomingSender = mpsc::Sender<Result<Received, TransportError>>;

struct Inner {
    endpoint: Endpoint,
    peers: Mutex<HashMap<EndpointId, Arc<iroh::endpoint::Connection>>>,
    incoming_tx: IncomingSender,
    incoming: Mutex<IncomingReceiver>,
    peer_receivers: Mutex<HashMap<EndpointId, PeerReceiver>>,
    accepted: Mutex<mpsc::Receiver<EndpointId>>,
}
#[derive(Clone)]
pub struct IrohTransport {
    inner: Arc<Inner>,
}

impl IrohTransport {
    /// Bind with Iroh's recommended production configuration.
    ///
    /// The N0 preset enables relay discovery/fallback as well as direct address
    /// connectivity, allowing peers behind NATs or on different networks to connect.
    pub async fn bind() -> Result<Self, iroh::endpoint::BindError> {
        let ep = Endpoint::builder(iroh::endpoint::presets::N0)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await?;
        Ok(Self::from_endpoint(ep))
    }

    /// Bind the production transport with a persisted identity.
    ///
    /// Reusing the same secret key preserves the endpoint ID across restarts.
    pub async fn bind_with_secret_key(
        secret_key: SecretKey,
    ) -> Result<Self, iroh::endpoint::BindError> {
        let ep = Endpoint::builder(iroh::endpoint::presets::N0)
            .secret_key(secret_key)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await?;
        Ok(Self::from_endpoint(ep))
    }

    /// Bind without relays for deterministic LAN and same-machine operation.
    ///
    /// This avoids contacting relay infrastructure and is therefore appropriate for
    /// tests, but it must not be used when cross-network reachability is required.
    pub async fn bind_local() -> Result<Self, iroh::endpoint::BindError> {
        let ep = Endpoint::builder(iroh::endpoint::presets::N0DisableRelay)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await?;
        Ok(Self::from_endpoint(ep))
    }

    /// Bind locally with a persisted identity. Primarily useful for deterministic tests.
    pub async fn bind_local_with_secret_key(
        secret_key: SecretKey,
    ) -> Result<Self, iroh::endpoint::BindError> {
        let ep = Endpoint::builder(iroh::endpoint::presets::N0DisableRelay)
            .secret_key(secret_key)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await?;
        Ok(Self::from_endpoint(ep))
    }
    pub fn from_endpoint(endpoint: Endpoint) -> Self {
        let (tx, rx) = mpsc::channel(64);
        let (accepted_tx, accepted_rx) = mpsc::channel(16);
        let inner = Arc::new(Inner {
            endpoint,
            peers: Mutex::new(HashMap::new()),
            incoming_tx: tx.clone(),
            incoming: Mutex::new(rx),
            peer_receivers: Mutex::new(HashMap::new()),
            accepted: Mutex::new(accepted_rx),
        });
        let accept_inner = inner.clone();
        tokio::spawn(async move {
            while let Some(incoming) = accept_inner.endpoint.accept().await {
                let tx = tx.clone();
                let accepted_tx = accepted_tx.clone();
                let connection_inner = accept_inner.clone();
                tokio::spawn(async move {
                    let conn = match incoming.await {
                        Ok(c) => Arc::new(c),
                        Err(_) => {
                            let _ = tx.send(Err(TransportError::Disconnected)).await;
                            return;
                        }
                    };
                    let peer = conn.remote_id();
                    // The first stream is always a versioned HELLO; reject the connection
                    // before exposing any application data if it is absent or malformed.
                    let (_w, mut r) = match conn.accept_bi().await {
                        Ok(x) => x,
                        Err(_) => {
                            let _ = tx.send(Err(TransportError::Disconnected)).await;
                            return;
                        }
                    };
                    let first = read_frame(&mut r).await.and_then(|p| validate_hello(&p));
                    if let Err(error) = first {
                        let _ = tx.send(Err(error)).await;
                        return;
                    }
                    // Publish only authenticated peers that completed our transport HELLO.
                    let (peer_tx, peer_rx) = mpsc::unbounded_channel();
                    connection_inner
                        .peers
                        .lock()
                        .await
                        .insert(peer, conn.clone());
                    connection_inner
                        .peer_receivers
                        .lock()
                        .await
                        .insert(peer, peer_rx);
                    if accepted_tx.send(peer).await.is_err() {
                        return;
                    }
                    loop {
                        let (_w, mut r) = match conn.accept_bi().await {
                            Ok(x) => x,
                            Err(_) => {
                                let _ = dispatch_received(
                                    &tx,
                                    &peer_tx,
                                    peer,
                                    Err(TransportError::Disconnected),
                                );
                                break;
                            }
                        };
                        if !dispatch_received(&tx, &peer_tx, peer, read_frame(&mut r).await) {
                            break;
                        }
                    }
                });
            }
        });
        Self { inner }
    }
    pub fn endpoint(&self) -> &Endpoint {
        &self.inner.endpoint
    }
    pub fn endpoint_id(&self) -> EndpointId {
        self.inner.endpoint.id()
    }

    /// Wait for an incoming peer to finish the transport HELLO without consuming
    /// its first application-level wire message.
    pub async fn accept_connected(&self) -> Result<ConnectedIrohTransport, TransportError> {
        let peer = self
            .inner
            .accepted
            .lock()
            .await
            .recv()
            .await
            .ok_or(TransportError::Disconnected)?;
        ConnectedIrohTransport::from_connected_peer(self.clone(), peer).await
    }
}
fn hello() -> Vec<u8> {
    vec![PROTOCOL_VERSION, b'H', b'E', b'L', b'L', b'O']
}
pub fn validate_hello(p: &[u8]) -> Result<(), TransportError> {
    if p == hello() {
        Ok(())
    } else {
        Err(TransportError::Malformed("invalid hello/version".into()))
    }
}
async fn write_frame(
    w: &mut iroh::endpoint::SendStream,
    payload: &[u8],
) -> Result<(), TransportError> {
    if payload.len() > MAX_MESSAGE_SIZE {
        return Err(TransportError::MessageTooLarge);
    }
    w.write_all(&(payload.len() as u32).to_be_bytes())
        .await
        .map_err(|_| TransportError::Disconnected)?;
    w.write_all(payload)
        .await
        .map_err(|_| TransportError::Disconnected)?;
    w.finish().map_err(|_| TransportError::Disconnected)?;
    Ok(())
}
async fn read_frame(r: &mut iroh::endpoint::RecvStream) -> Result<Vec<u8>, TransportError> {
    let n = r
        .read_u32()
        .await
        .map_err(|_| TransportError::Disconnected)? as usize;
    if n > MAX_MESSAGE_SIZE {
        return Err(TransportError::MessageTooLarge);
    }
    let mut p = vec![0; n];
    r.read_exact(&mut p)
        .await
        .map_err(|_| TransportError::Disconnected)?;
    Ok(p)
}

fn copy_stream_error(error: &TransportError) -> TransportError {
    match error {
        TransportError::MessageTooLarge => TransportError::MessageTooLarge,
        TransportError::Malformed(message) => TransportError::Malformed(message.clone()),
        TransportError::Disconnected | TransportError::Iroh(_) | TransportError::Io(_) => {
            TransportError::Disconnected
        }
    }
}

fn dispatch_received(
    global: &IncomingSender,
    peer_queue: &mpsc::UnboundedSender<Result<Vec<u8>, TransportError>>,
    peer: EndpointId,
    result: Result<Vec<u8>, TransportError>,
) -> bool {
    match result {
        Ok(payload) => {
            let peer_open = peer_queue.send(Ok(payload.clone())).is_ok();
            let global_open = global.try_send(Ok(Received { peer, payload })).is_ok();
            peer_open || global_open
        }
        Err(error) => {
            let peer_open = peer_queue.send(Err(copy_stream_error(&error))).is_ok();
            let global_open = global.try_send(Err(error)).is_ok();
            peer_open || global_open
        }
    }
}

#[async_trait]
impl RawPeerTransport for IrohTransport {
    async fn connect(&self, addr: EndpointAddr) -> Result<EndpointId, TransportError> {
        let id = addr.id;
        let conn = Arc::new(self.inner.endpoint.connect(addr.clone(), ALPN).await?);
        let (mut w, _r) = conn
            .open_bi()
            .await
            .map_err(|_| TransportError::Disconnected)?;
        write_frame(&mut w, &hello()).await?;
        self.inner.peers.lock().await.insert(id, conn.clone());
        let (peer_tx, peer_rx) = mpsc::unbounded_channel();
        self.inner.peer_receivers.lock().await.insert(id, peer_rx);
        let tx = self.inner.incoming_tx.clone();
        tokio::spawn(async move {
            loop {
                let (_w, mut r) = match conn.accept_bi().await {
                    Ok(streams) => streams,
                    Err(_) => {
                        let _ =
                            dispatch_received(&tx, &peer_tx, id, Err(TransportError::Disconnected));
                        break;
                    }
                };
                if !dispatch_received(&tx, &peer_tx, id, read_frame(&mut r).await) {
                    break;
                }
            }
        });
        Ok(id)
    }
    async fn send(&self, peer: EndpointId, payload: &[u8]) -> Result<(), TransportError> {
        if payload.len() > MAX_MESSAGE_SIZE {
            return Err(TransportError::MessageTooLarge);
        }
        let conn = {
            let peers = self.inner.peers.lock().await;
            peers.get(&peer).cloned()
        }
        .ok_or(TransportError::Disconnected)?;
        let (mut w, _r) = conn
            .open_bi()
            .await
            .map_err(|_| TransportError::Disconnected)?;
        write_frame(&mut w, payload).await
    }
    async fn recv(&self) -> Result<Received, TransportError> {
        self.inner
            .incoming
            .lock()
            .await
            .recv()
            .await
            .unwrap_or(Err(TransportError::Disconnected))
    }
}
/// Connected adapter implementing the transport contract from knot-sync.
pub struct ConnectedIrohTransport {
    transport: IrohTransport,
    peer: EndpointId,
    incoming: mpsc::UnboundedReceiver<Result<Vec<u8>, TransportError>>,
}
impl ConnectedIrohTransport {
    pub async fn connect(
        transport: IrohTransport,
        addr: EndpointAddr,
    ) -> Result<Self, TransportError> {
        let peer = transport.connect(addr).await?;
        Self::from_connected_peer(transport, peer).await
    }

    /// Wrap a peer whose connection has already completed the transport HELLO.
    /// Returns Disconnected if the peer is not currently registered.
    pub async fn from_connected_peer(
        transport: IrohTransport,
        peer: EndpointId,
    ) -> Result<Self, TransportError> {
        if !transport.inner.peers.lock().await.contains_key(&peer) {
            return Err(TransportError::Disconnected);
        }
        let incoming = transport
            .inner
            .peer_receivers
            .lock()
            .await
            .remove(&peer)
            .ok_or(TransportError::Disconnected)?;
        Ok(Self {
            transport,
            peer,
            incoming,
        })
    }

    pub fn peer(&self) -> EndpointId {
        self.peer
    }
}
fn map_error(e: TransportError) -> knot_sync::TransportError {
    match e {
        TransportError::MessageTooLarge => {
            knot_sync::TransportError::Protocol(knot_sync::ProtocolError::TooLarge)
        }
        TransportError::Malformed(s) => {
            knot_sync::TransportError::Protocol(knot_sync::ProtocolError::Invalid(s))
        }
        TransportError::Disconnected | TransportError::Io(_) | TransportError::Iroh(_) => {
            knot_sync::TransportError::Closed
        }
    }
}
#[async_trait]
impl knot_sync::PeerTransport for ConnectedIrohTransport {
    async fn send(
        &mut self,
        message: knot_sync::WireMessage,
    ) -> Result<(), knot_sync::TransportError> {
        let bytes = message.encode()?;
        self.transport
            .send(self.peer, &bytes)
            .await
            .map_err(map_error)
    }
    async fn recv(&mut self) -> Result<knot_sync::WireMessage, knot_sync::TransportError> {
        let payload = self
            .incoming
            .recv()
            .await
            .ok_or(knot_sync::TransportError::Closed)?
            .map_err(map_error)?;
        knot_sync::WireMessage::decode(&payload).map_err(knot_sync::TransportError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use knot_sync::{Hello, Message, PeerTransport as SyncPeerTransport, WireMessage};
    use tokio::time::{Duration, timeout};

    #[test]
    fn rejects_bad_hello() {
        assert!(validate_hello(b"bad").is_err());
    }

    #[test]
    fn validates_version() {
        assert!(validate_hello(&hello()).is_ok());
    }

    #[test]
    fn uses_knot_sync_protocol() {
        assert_eq!(ALPN, b"knot-sync/1");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn same_machine_wire_adapter_and_rejection() {
        let a = IrohTransport::bind_local().await.expect("bind peer A");
        let b = IrohTransport::bind_local().await.expect("bind peer B");
        let a_id = a.endpoint_id();

        let mut a_wire = timeout(
            Duration::from_secs(10),
            ConnectedIrohTransport::connect(a.clone(), b.endpoint().addr()),
        )
        .await
        .expect("local connect timed out")
        .expect("connect peer A to B");
        let mut b_wire = timeout(Duration::from_secs(5), b.accept_connected())
            .await
            .expect("accept notification timed out")
            .expect("accepted peer adapter");
        assert_eq!(b_wire.peer(), a_id);

        let hello = WireMessage::new(Message::Hello(Hello {
            device_id: "peer-a".into(),
            endpoint_id: a_id.to_string(),
        }));
        timeout(
            Duration::from_secs(5),
            SyncPeerTransport::send(&mut a_wire, hello.clone()),
        )
        .await
        .expect("wire send timed out")
        .expect("wire send");
        let received = timeout(Duration::from_secs(5), SyncPeerTransport::recv(&mut b_wire))
            .await
            .expect("wire receive timed out")
            .expect("wire receive");
        assert_eq!(received, hello);

        let reply = WireMessage::new(Message::Hello(Hello {
            device_id: "peer-b".into(),
            endpoint_id: b.endpoint_id().to_string(),
        }));
        SyncPeerTransport::send(&mut b_wire, reply.clone())
            .await
            .expect("responder reply");
        let reply_received = timeout(Duration::from_secs(5), SyncPeerTransport::recv(&mut a_wire))
            .await
            .expect("reply receive timed out")
            .expect("reply receive");
        assert_eq!(reply_received, reply);

        // Framing accepts arbitrary bytes, but the sync adapter must reject malformed JSON.
        a.send(a_wire.peer(), b"not-json").await.expect("raw send");
        let malformed = timeout(Duration::from_secs(5), SyncPeerTransport::recv(&mut b_wire))
            .await
            .expect("malformed receive timed out");
        assert!(matches!(
            malformed,
            Err(knot_sync::TransportError::Protocol(_))
        ));

        let oversized = vec![0_u8; MAX_MESSAGE_SIZE + 1];
        assert!(matches!(
            a.send(a_wire.peer(), &oversized).await,
            Err(TransportError::MessageTooLarge)
        ));

        a.endpoint().close().await;
        b.endpoint().close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn connected_adapters_isolate_concurrent_peers() {
        let server = IrohTransport::bind_local().await.expect("bind server");
        let a = IrohTransport::bind_local().await.expect("bind A");
        let c = IrohTransport::bind_local().await.expect("bind C");
        let a_id = a.endpoint_id();
        let c_id = c.endpoint_id();

        let mut a_wire = ConnectedIrohTransport::connect(a.clone(), server.endpoint().addr())
            .await
            .expect("connect A");
        let mut c_wire = ConnectedIrohTransport::connect(c.clone(), server.endpoint().addr())
            .await
            .expect("connect C");
        let first = timeout(Duration::from_secs(5), server.accept_connected())
            .await
            .expect("accept A/C timeout")
            .expect("accept A/C");
        let second = timeout(Duration::from_secs(5), server.accept_connected())
            .await
            .expect("accept A/C timeout")
            .expect("accept A/C");
        let (mut server_a, mut server_c) = if first.peer() == a_id {
            (first, second)
        } else {
            (second, first)
        };
        assert_eq!(server_a.peer(), a_id);
        assert_eq!(server_c.peer(), c_id);

        let from_a = WireMessage::new(Message::Hello(Hello {
            device_id: "a".into(),
            endpoint_id: a_id.to_string(),
        }));
        let from_c = WireMessage::new(Message::Hello(Hello {
            device_id: "c".into(),
            endpoint_id: c_id.to_string(),
        }));
        // Deliberately send C first, then wait on A first: a shared queue would cross-wire.
        SyncPeerTransport::send(&mut c_wire, from_c.clone())
            .await
            .expect("send C");
        SyncPeerTransport::send(&mut a_wire, from_a.clone())
            .await
            .expect("send A");
        let got_a = timeout(
            Duration::from_secs(5),
            SyncPeerTransport::recv(&mut server_a),
        )
        .await
        .expect("receive A timeout")
        .expect("receive A");
        let got_c = timeout(
            Duration::from_secs(5),
            SyncPeerTransport::recv(&mut server_c),
        )
        .await
        .expect("receive C timeout")
        .expect("receive C");
        assert_eq!(got_a, from_a);
        assert_eq!(got_c, from_c);

        a.endpoint().close().await;
        c.endpoint().close().await;
        server.endpoint().close().await;
    }

    #[tokio::test]
    async fn persisted_secret_preserves_endpoint_id() {
        let generated = generate_secret_key();
        let persisted = secret_key_to_bytes(&generated);
        let restored = secret_key_from_bytes(&persisted).expect("restore 32-byte secret");
        assert_eq!(secret_key_to_bytes(&restored), persisted);
        assert!(secret_key_from_bytes(&persisted[..31]).is_err());

        let first = IrohTransport::bind_local_with_secret_key(restored)
            .await
            .expect("bind first endpoint");
        let expected_id = first.endpoint_id();
        first.endpoint().close().await;

        let second = IrohTransport::bind_local_with_secret_key(
            secret_key_from_bytes(&persisted).expect("restore persisted secret again"),
        )
        .await
        .expect("bind restarted endpoint");
        assert_eq!(second.endpoint_id(), expected_id);
        second.endpoint().close().await;
    }
}
